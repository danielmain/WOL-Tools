# iOS App Plan (OfflineWOL via Tauri 2 Mobile)

> Goal: Ship an iOS/iPadOS version of **OfflineWOL** by reusing `wol-core`, `wol-desktop`, `wol-desktop/ui/`, and `wol-highlights/` with minimal platform-specific glue — no rewrite.

---

### Local commands (root `package.json` → `scripts/ios.js`)

```bash
npm run ios              # cargo tauri ios dev (prefers the most recent iPhone; --device <udid|name>)
npm run ios:sim          # dev on a simulator (needs the matching iphonesimulator runtime)
npm run ios:build        # signed release .ipa → wol-desktop/gen/apple/build/arm64/
npm run ios:build:debug  # debug .ipa
npm run ios:install      # install the newest .ipa via devicectl (no rebuild)
npm run ios:launch       # launch com.offlinewol.desktop (fails if the iPhone is locked)
npm run ios:run          # build + install + launch
npm run ios:xcode        # `tauri ios dev --open` → Xcode, CLI stays attached (see below)
npm run ios:init         # regenerate gen/apple
```

Two traps this script exists to paper over:

1. **A build launched inside Xcode.app needs a running Tauri CLI.** Xcode's `Build Rust Code`
   phase runs `cargo tauri ios xcode-script`, which reads
   `gen/apple/.tauri/cli-options-server.json`; without a live `tauri ios dev` / `ios build` that
   file is absent and the phase fails with *"the `tauri ios dev` or `tauri ios build` command must
   be running while Xcode builds the app"*. So `npm run ios:xcode` starts
   `tauri ios dev --open` (CLI + Xcode) instead of merely opening the project file.
2. **The device must be unlocked and reachable.** `tauri ios dev` passes
   `-destination id=<udid>` to `xcodebuild`; a locked iPad dies after ~30 s with *"Timed out
   waiting for all destinations matching the provided destination specifier"*. `scripts/ios.js`
   orders candidates iPhone-first / most-recently-connected (devicectl lists arbitrarily) and logs
   the device it picked — override with `--device <udid|name>`.

`scripts/ios.js` cleans the environment (`/usr/bin/clang`, no `NIX_*`, rustup toolchain
before Homebrew `cargo`), because a Nix devshell breaks iOS cross-compilation. It also refuses
to build while another build holds Xcode's `DerivedData/…/XCBuildData/build.db` — Xcode allows
one writer, and the GUI build never spawns an `xcodebuild` process, so the lock is probed with
`lsof`.

---

## 1. Why Tauri 2 Mobile Fits This Repo

- **`wol-core/` is already GUI-agnostic** (zero Tauri or desktop dependencies). SQLite metadata + disk blob caching, `wol://` protocol handling, HTML URL rewriting, `.jwlibrary` import/export, tokenization, and sync all compile for `aarch64-apple-ios`.
- **`wol-desktop/` already uses Tauri 2**, whose custom URI scheme protocol (`register_asynchronous_uri_scheme_protocol("wol", ...)`) is backed by `WKURLSchemeHandler` inside `WKWebView` on iOS.
- **`wol-highlights/` JS & `wol-desktop/ui/`** (`tokenize.js`, `highlight.js`, `backup.js`, `content.js`, `content.css`, `setup.html`, `manager.html`) run inside `WKWebView` without Safari WebExtension limitations.

---

## 2. Architecture & Code Changes Needed

### Phase 1 — Tauri 2 Mobile Project & Crate Setup
1. **Library crate type (`wol-desktop/Cargo.toml`)**:
   - Add `[lib]` with `crate-type = ["staticlib", "cdylib", "rlib"]` so Cargo builds the static library linked by Xcode on iOS while keeping `wol-desktop` usable as a normal Rust lib for tests.
2. **Mobile entry point (`wol-desktop/src/lib.rs`)**:
   - Annotate `run_app()` with `#[cfg_attr(mobile, tauri::mobile_entry_point)]`.
3. **Initialize iOS target**:
   - Install Rust iOS targets (`rustup target add aarch64-apple-ios aarch64-apple-ios-sim`).
   - Run `cargo tauri ios init` inside `wol-desktop/` to generate `wol-desktop/gen/apple/`.
   - Configure bundle identifier, app icons, and signing team in `wol-desktop/tauri.conf.json` / Xcode.

### Phase 2 — Sandbox-Safe App Data Directory
- Currently `wol-core/src/dirs.rs` (`app_data_dir()`) constructs `HOME/Library/Application Support/com.offlinewol.desktop`.
- **Change**:
  - Allow `run_app()` to resolve the data directory from Tauri's `app.path().app_data_dir()` on mobile (`#[cfg(mobile)]`) — or pass an explicit `PathBuf` into `StateManager` / `DiskCache` — instead of reading `HOME` directly.
  - Keep `wol-core::dirs::app_data_dir_or_env()` untouched for desktop & `wol-cli` so desktop/CLI continue sharing the same folder.

### Phase 3 — Gate Desktop-Only APIs (`#[cfg(desktop)]` / `#[cfg(mobile)]`)
In `wol-desktop/src/lib.rs` and `wol-desktop/src/commands.rs`:
1. **Menu bar**:
   - Gate `install_menu(app)` and `tauri::menu::*` behind `#[cfg(desktop)]` (iOS has no desktop menu bar).
2. **Window builder options**:
   - Gate `.inner_size(1200.0, 800.0)` and `.theme(...)` in `build_main_window` behind `#[cfg(desktop)]`.
3. **Single-window Manager & Navigation**:
   - `open_manager_window` currently spawns a second `WebviewWindow` (`"manager"`), which is unsupported on iOS.
   - On `#[cfg(mobile)]`:
     - Open Manager/Settings inside the `"main"` webview (either via an in-page drawer/modal injected by `bridge.rs`, or by navigating `"main"` to `manager.html` with a Back button to return to the last `wol://` URL).
     - Inject a lightweight mobile navigation bar (Back, Forward, Home, Manager) in `WKWebView` since there is no desktop History/App menu or keyboard shortcuts (`Cmd+[`, `Cmd+Shift+H`).

### Phase 4 — Touch & Mobile WebView UX (`wol-highlights/` & `wol-desktop/ui/`)
The main reason Safari iOS feels poor is touch text-selection and viewport ergonomics:
1. **Touch-friendly highlight toolbar (`content.js` / `content.css`)**:
   - On iOS `WKWebView`, the native Copy / Look Up callout overlaps selections and `mouseup` fires inconsistently with touch handles.
   - Listen to `selectionchange` (debounced) on touch devices and anchor the 1–6 color picker + note/delete actions to a **fixed bottom (or top) bar** rather than floating right above the selection.
   - Respect `env(safe-area-inset-top)` / `env(safe-area-inset-bottom)` so UI elements clear the Dynamic Island / notch and home indicator.
2. **Responsive Manager & Setup UI (`wol-desktop/ui/`)**:
   - Audit `setup.html` / `manager.html` / `manager.css` for narrow portrait screens (375px+), larger tap targets (44×44pt minimum), and avoiding hover-only interactions.
3. **File Import / Export on iOS**:
   - Verify `tauri-plugin-dialog` save/open flows on iOS (Files app / iCloud Drive picker), or pair `export_marks_to_file` with the iOS Share Sheet / `<input type="file">` fallback in `manager.js` so `.jwlibrary` backups round-trip cleanly with the JW Library iOS app.

### Phase 5 — Cloud Sync & OAuth on iOS (`wol-core/src/sync.rs`)
- Standard WebDAV / Nextcloud app-password sync (`reqwest`) works on iOS without changes.
- **Browser OAuth caveats in `wol-core/src/sync.rs`**:
  - `open_system_browser()` currently spawns `std::process::Command::new("open")` / `xdg-open` / `cmd`, which does not work in an iOS sandbox.
  - `connect_google_drive()` binds `TcpListener::bind("127.0.0.1:0")` and waits for a localhost redirect; iOS suspends background sockets when switching to Safari.
  - **Fix**:
    - Replace `open_system_browser` with a host-provided opener callback (using `tauri-plugin-opener` on both desktop and mobile).
    - For Nextcloud browser flow (`connect_nextcloud_browser`), polling works once the external browser opens via `tauri-plugin-opener` (or `ASWebAuthenticationSession`).
    - For Google Drive on iOS, use a custom URL scheme redirect (`tauri-plugin-deep-link`) or `ASWebAuthenticationSession` instead of `127.0.0.1`.

---

## 3. Progress & Session Handoff

### Completed (Rust / Tauri 2 Core Preparation)
- **Phase 1 (Crate & Entry Point — partial)**:
  - `wol-desktop/Cargo.toml`: Added `[lib]` with `name = "wol_desktop"` and `crate-type = ["staticlib", "cdylib", "rlib"]`.
  - `wol-desktop/src/lib.rs`: Annotated `run_app()` with `#[cfg_attr(mobile, tauri::mobile_entry_point)]`.
- **Phase 2 (Sandbox-Safe App Data Directory)**:
  - `wol-core/src/dirs.rs`: Added `set_app_data_dir_override(path: PathBuf)` backed by `OnceLock<PathBuf>` and consulted in `app_data_dir_or_env()` (after `WOL_DATA_DIR`, before `app_data_dir()`), so `publications.rs`, `StateManager`, and `DiskCache` all honor the mobile sandbox path.
  - `wol-desktop/src/lib.rs`: Moved `StateManager` / `DiskCache` / `Crawler` / `SyncManager` / `ProtocolHandler` initialization into `.setup(|app| ...)` using `resolve_app_data_dir(app)` (which resolves `app.path().app_data_dir()` on `#[cfg(mobile)]` and sets the `wol-core` override), with an `Arc<OnceLock<Arc<ProtocolHandler>>>` for the `wol://` protocol closure.
- **Phase 3 (Gate Desktop-Only APIs & Single-Window Manager)**:
  - `wol-desktop/src/lib.rs`: Gated `tauri::menu::*`, `install_menu(app)`, `.title(...)`, `.inner_size(1200.0, 800.0)`, and `.theme(...)` behind `#[cfg(desktop)]`.
  - `wol-desktop/src/commands.rs`: Gated `WebviewUrl`, `WebviewWindowBuilder`, `theme_to_tauri`, and `set_theme(...)` calls behind `#[cfg(desktop)]`; updated `open_manager_window` so `#[cfg(mobile)]` navigates the `"main"` webview to `tauri://localhost/index.html[#tab]` instead of spawning a second window.
- **Verification**:
  - `cargo test --workspace` passed (`wol-core`: 76 unit + 2 parity tests; `wol-desktop`: 5 unit + 13 integration tests across 5 suites; `wol-cli`: passed).

### Completed (Session 2 — Mobile Navigation, Touch UX & Responsive UI)
- **Phase 3.3 (Mobile Back-from-Manager UX)**:
  - `wol-desktop/ui/index.html` & `wol-desktop/ui/manager.js`: Added `#mobile-back-btn` ("← Back") in `.app-header`, automatically shown when `index.html` is loaded inside the `"main"` window (`window.__TAURI_INTERNALS__?.metadata?.currentWindow?.label === "main"`), returning via `window.history.back()` with `navigate_home` fallback.
- **Phase 4.1 (Touch-Friendly Highlight Toolbar & Nav Pill)**:
  - `wol-highlights/content.js`: Added `isTouchDevice()` detection, `pointerdown` `preventDefault()` on `.wol-hl-toolbar` so tapping color buttons doesn't collapse iOS selections, and a debounced (`180ms`) `selectionchange` listener for touch selection handles.
  - `wol-highlights/content.css`: Added `.wol-hl-toolbar.wol-hl-toolbar-mobile` fixed bottom bar (`bottom: calc(18px + env(safe-area-inset-bottom, 0px))`) with 34×34px touch buttons and `@media (pointer: coarse)` hover-scale suppression.
  - `wol-desktop/src/bridge.rs`: Updated `#wol-desktop-nav-pill` with `@media (pointer: coarse)` rules to clear `env(safe-area-inset-top)` / `env(safe-area-inset-left)` and enlarge tap targets to 38×38px.
- **Phase 4.2 (Responsive Manager & Setup UI)**:
  - `wol-desktop/ui/index.html` & `wol-desktop/ui/setup.html`: Added `<meta name="viewport" content="width=device-width, initial-scale=1.0, viewport-fit=cover">`.
  - `wol-desktop/ui/setup.html`: Updated `body` for `100dvh` + `env(safe-area-inset-*)` padding, and added 44px min-height / 16px input font-size on `@media (max-width: 480px), (pointer: coarse)`.
  - `wol-desktop/ui/manager.css`: Added `.btn-back` styling, `env(safe-area-inset-*)` body padding, horizontally scrollable `.tabs`, stacked action rows/grids under `@media (max-width: 600px), (pointer: coarse)`, 44px min-height tap targets, and 16px input font-size.

### Completed (Session 3 — Mobile Export Fallback & iOS Sync/OAuth Prep)
- **Phase 4.3 (Mobile `.jwlibrary` Export)**:
  - `wol-core/src/jwlibrary_export.rs`: Extracted `write_jwlibrary_zip<W: Write + Seek>` and added `export_marks_to_jwlibrary_bytes` to build `.jwlibrary` archives in memory without a filesystem save dialog.
  - `wol-desktop/src/commands.rs` & `wol-desktop/src/lib.rs`: Gated `tauri_plugin_dialog::DialogExt` / `save_file` inside `export_marks_to_file` behind `#[cfg(desktop)]` (returning an explicit mobile-unsupported error on `#[cfg(mobile)]`), and added + registered the `export_marks_to_bytes` Tauri command.
  - `wol-desktop/ui/manager.js`: Added a fallback in the Export handler that calls `export_marks_to_bytes`, wraps the archive in a `File`, and invokes `navigator.share({ files: [file], title })` (iOS Share Sheet) with an `<a download>` blob fallback.
- **Phase 5 (iOS-Safe `sync.rs` & Host Browser Opener Hook — partial)**:
  - `wol-core/src/sync.rs`: Added `BrowserOpener` (`SyncManager::set_browser_opener` / `open_browser`), added a non-desktop fallback branch in `open_system_browser` so `wol-core` compiles on `target_os = "ios"`, switched `connect_nextcloud_browser` and `connect_google_drive` to `self.open_browser(...)`, and gated `connect_google_drive`'s `127.0.0.1` localhost flow on `#[cfg(not(target_os = "ios"))]` with an actionable error message on iOS.

### Completed (Session 4 — iOS Toolchain/Project Init & OAuth Wiring)
- **Phase 1.3 (iOS Toolchain & Project Init)**:
  - Added `aarch64-apple-ios` and `aarch64-apple-ios-sim` targets to the existing `~/.rustup/toolchains/stable-aarch64-apple-darwin` toolchain (via `nix shell nixpkgs#rustup -c rustup target add aarch64-apple-ios aarch64-apple-ios-sim`).
  - Installed `cocoapods` via Homebrew and ran `cargo tauri ios init --ci --skip-targets-install` in `wol-desktop/`, generating `wol-desktop/gen/apple/wol-desktop.xcodeproj`.
  - **Nix cc-wrapper note**: Nix's wrapped `clang` injects `-mmacos-version-min=14.0`, which breaks iOS cross-compilation. Always pass `PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH"`, `CC_aarch64_apple_ios_sim=/usr/bin/clang`, `CXX_aarch64_apple_ios_sim=/usr/bin/clang++`, `CC_aarch64_apple_ios=/usr/bin/clang`, `CXX_aarch64_apple_ios=/usr/bin/clang++` when building iOS targets.
  - Verified `cargo check --target aarch64-apple-ios-sim -p wol-desktop` and `cargo check --target aarch64-apple-ios -p wol-desktop` both pass cleanly.
- **Phase 5 (`tauri-plugin-opener` & Google Drive OAuth on iOS)**:
  - `wol-desktop/Cargo.toml`: Added `tauri-plugin-opener = "2"` and `cc = "1"` (build-dep).
  - `wol-core/src/sync.rs`: Added `OAuthSessionOpener` (`SyncManager::set_oauth_session_opener`), `build_google_auth_url`, `google_ios_callback_scheme`, and `extract_oauth_code_from_url`; updated `google_credentials()` and `ensure_google_token()` so iOS OAuth clients work without a `client_secret`, and wired `connect_google_drive` on `target_os = "ios"` to `ASWebAuthenticationSession` via `open_oauth_session`.
  - `wol-desktop/src/ios_auth.m` & `wol-desktop/build.rs`: Added an Objective-C `ASWebAuthenticationSession` bridge (`wol_ios_start_oauth_session`) compiled with `-fobjc-arc` and linked against `AuthenticationServices` and `UIKit` on `target_os = "ios"`.
  - `wol-desktop/src/lib.rs`: Registered `tauri_plugin_opener::init()`, wired `sync_mgr.set_browser_opener(...)` via `OpenerExt`, and wired `sync_mgr.set_oauth_session_opener(...)` on `target_os = "ios"`.

### Completed (Session 5 — Signing, Icons & End-to-End iOS Device Build)
- **Phase 1.3 (Signing & Icons)**:
  - `wol-desktop/tauri.conf.json`: Set `bundle.iOS.developmentTeam` to `"7H7P2FNJK6"` (matching `safari/WOL Highlights/WOL Highlights.xcodeproj/project.pbxproj`).
  - `wol-desktop/gen/apple/Assets.xcassets/AppIcon.appiconset/`: Copied the OfflineWOL iOS icon set from `wol-desktop/icons/ios/*.png` (replacing the default Tauri placeholder icons generated by `tauri ios init`).
- **Phase 1.3 (Xcode `Build Rust Code` Script & Nix Cross-Compile Fixes)**:
  - `wol-desktop/gen/apple/project.yml` & `wol-desktop/gen/apple/wol-desktop.xcodeproj/project.pbxproj`:
    - Switched `npm run -- tauri ios xcode-script` to `cargo tauri ios xcode-script` (since root `package.json` has no `tauri` script).
    - Exported `CC=/usr/bin/clang`, `CXX=/usr/bin/clang++`, `CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER=/usr/bin/clang`, `CARGO_TARGET_AARCH64_APPLE_IOS_LINKER=/usr/bin/clang`, `CARGO_TARGET_AARCH64_APPLE_IOS_SIM_LINKER=/usr/bin/clang`, and `CC_aarch64_apple_ios*` / `CXX_aarch64_apple_ios*` inside the Xcode build phase so Nix's `cc-wrapper` does not inject `-mmacos-version-min=14.0` or fail to find `-liconv` when `xcodebuild` sanitizes `NIX_LDFLAGS`.
    - Captured `IOS_SDK_ROOT="${SDKROOT:?}"`, ran `unset SDKROOT` so host build scripts (`tauri` `build.rs`) link against the macOS SDK, and passed `-C link-arg=-isysroot -C link-arg=$IOS_SDK_ROOT` via `RUSTFLAGS` for the iOS target link.
- **Verification**:
  - `cd wol-desktop && PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH" cargo tauri ios build --debug --target aarch64 --ci --no-sign` succeeded and produced `wol-desktop/gen/apple/build/arm64/OfflineWOL.ipa`.
  - `cargo test --workspace` passed (`wol-core`: 78 unit + 2 parity; `wol-desktop`: 5 unit + 14 integration tests across 5 suites; `wol-cli`: passed).
- **Simulator note**:
  - `cargo tauri ios build --target aarch64-sim` currently stops at `Xcode Simulator SDK 27.2 is not installed` because Xcode 27.2 has `iphonesimulator27.2` SDK while `xcrun simctl list runtimes` has `iOS 27.0` (27.2 runtime not yet downloaded).

### Completed (Session 6 — Signed Device Build & Installation)- **Signed `.ipa` Build**:
  - Verified `cd wol-desktop && PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH" cargo tauri ios build --debug --target aarch64 --ci` builds and signs `wol-desktop/gen/apple/build/arm64/OfflineWOL.ipa` cleanly with team `7H7P2FNJK6`.
- **Device Installation**:
  - Installed `OfflineWOL.app` (`com.offlinewol.desktop`) onto the connected physical device `iPhone Air - Dani` (`00008150-000E41E12E62401C`) via `xcrun devicectl device install app --device 00008150-000E41E12E62401C wol-desktop/gen/apple/build/arm64/OfflineWOL.ipa`.
  - Attempted `xcrun devicectl device process launch --device 00008150-000E41E12E62401C com.offlinewol.desktop`; blocked only because the device screen is locked (`FBSOpenApplicationErrorDomain error 7: Locked`).

### Remaining for Next Session
1. **Physical Device Verification (Phase 4.3 & Section 4)**:
   - Unlock `iPhone Air - Dani` and open **OfflineWOL** (or run `xcrun devicectl device process launch --device 00008150-000E41E12E62401C com.offlinewol.desktop` / `cargo tauri ios dev` while unlocked).
   - Verify setup screen, offline caching (Airplane Mode), touch text selection + highlight toolbar, `<input type="file">` import, `navigator.share` `.jwlibrary` export, and Nextcloud / Google Drive OAuth on device.
   - (Optional) Download the iOS 27.2 Simulator runtime in Xcode if you also want `cargo tauri ios dev` in Simulator.

---

## 4. Verification Checklist

1. **Desktop & CLI stay green**:
   - `cargo test --workspace` (`npm run desktop:test`) passes with no regressions.
2. **iOS Simulator build & smoke test**:
   - `cargo tauri ios dev` launches OfflineWOL in the iOS Simulator.
   - Setup screen saves language/region, loads `wol://.../wol/h/...`, and caches pages offline (verify with Airplane Mode / offline toggle).
   - Touch text selection opens the mobile highlight bar, creates/edits/deletes highlights and notes, and persists across app restarts.
   - Importing and exporting a `.jwlibrary` backup via the iOS Files app works round-trip.
3. **Physical iPhone / iPad test**:
   - Verify safe-area insets, dark mode, smooth scrolling, and memory/cache cap behavior on device.
