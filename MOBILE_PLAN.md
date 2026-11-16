# Mobile Port Plan — OfflineWOL (iOS & Android)

> **Goal**: Port `wol-desktop/` (**OfflineWOL**) to **iOS** and **Android** using **Tauri 2** mobile targets while keeping the desktop app and shared `wol-highlights/` JS engine intact.

---

## 0. Architecture Fit & Strategy

`wol-desktop/` already runs on **Tauri 2** with a webview frontend, and every GUI-agnostic
piece now lives in **`wol-core/`** (Cargo workspace: `wol-core`, `wol-desktop`, `wol-cli`):
cache, protocol/handler, `url_map`, crawler, state/`wolMarks`, sync, publications,
`.jwlibrary` import/export, tokenization and the DOM model. `wol-core` has **no Tauri, clap or
terminal dependencies**, so a mobile port wraps it with a thin Tauri shell instead of forking
logic. Rather than creating a separate codebase, this plan adapts `wol-desktop/` (plus the
shared `wol-core/`) to compile for both desktop and mobile (`#[cfg(desktop)]` / `#[cfg(mobile)]`).

> **Already done (do not re-do):** the Tauri decoupling that used to block mobile —
> `url_map` uses `url::Url`, `handler` speaks `http_types::{Response, Method, StatusCode,
> Headers}`, `sync` reports through a `SyncListener` callback and is driven by
> `SyncManager::new_shared()` instead of an `AppHandle`, and `publications` no longer exposes
> Tauri commands. Paths below name `wol-core/src/...` where that is where the code now lives.

---

## Phase 1 — Rust & Tauri 2 Mobile Foundation

**Outcome**: The app compiles and boots on iOS Simulator and Android Emulator, loading `setup.html` and `wol://` pages.

- [ ] **1.1 Crate & entry point (`wol-desktop/Cargo.toml`, `wol-desktop/src/lib.rs`)**
  - Add `[lib]` with `crate-type = ["staticlib", "cdylib", "rlib"]` to `wol-desktop/Cargo.toml` so Tauri mobile can link the Rust library into Xcode/Gradle projects.
  - Annotate `run_app()` in `wol-desktop/src/lib.rs` with `#[cfg_attr(mobile, tauri::mobile_entry_point)]`.
- [ ] **1.2 TLS backend (`wol-desktop/Cargo.toml`)**
  - Configure `reqwest` with `default-features = false` and `features = ["rustls-tls", ...]` (instead of platform `native-tls` / OpenSSL) to avoid cross-compilation failures on Android NDK and iOS.
- [ ] **1.3 Platform-correct app data directory (`wol-desktop/src/lib.rs`, `wol-core/src/publications.rs`, `wol-core/src/dirs.rs`)**
  - Replace the `wol-core/src/dirs.rs` fallback (which currently hardcodes `$HOME/Library/Application Support/com.offlinewol.desktop`) with `app.path().app_data_dir()` inside `.setup(...)`, and pass the path into `wol-core`.
  - Keep the legacy macOS migration (`com.wolhighlights.desktop` → `com.offlinewol.desktop`) behind `#[cfg(target_os = "macos")]`.
  - Pass the resolved `PathBuf` into `wol-core::publications` instead of letting it call `dirs` directly.
- [ ] **1.4 Gate desktop-only window & menu APIs (`wol-desktop/src/lib.rs`)**
  - Wrap `Menu`, `Submenu`, `MenuItem`, `PredefinedMenuItem`, `app.set_menu(...)`, and `app.on_menu_event(...)` in `#[cfg(desktop)]`.
  - Gate `.inner_size(1200.0, 800.0)` and `.theme(...)` on `WebviewWindowBuilder` with `#[cfg(desktop)]` where unsupported on mobile.
- [ ] **1.5 Initialize Tauri mobile projects**
  - Install prerequisites: Xcode + iOS SDK (`rustup target add aarch64-apple-ios aarch64-apple-ios-sim`), Android SDK/NDK (`rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android`).
  - Run `cargo tauri ios init` and `cargo tauri android init` inside `wol-desktop/` to generate `gen/apple` and `gen/android`.
  - Verify `wol://` custom protocol responses in both `WKWebView` (iOS) and `Android WebView` (confirming CSS/JS/subresource loading and cookie/header handling).

---

## Phase 2 — Single-Window UI & Mobile Navigation

**Outcome**: Users can access Highlights, Backup Manager, Offline Cache, and Settings without a second window or desktop menu bar.

- [ ] **2.1 Replace multi-window Manager with in-webview UI (`wol-desktop/src/commands.rs`, `wol-desktop/src/bridge.rs`, `wol-desktop/ui/`)**
  - Mobile OSes only allow a single main webview. Keep `open_manager_window` for `#[cfg(desktop)]`, and on mobile have the `"Manage Highlights & Settings"` / `"Settings"` actions open an in-page full-screen modal/drawer (or navigate within `"main"` with a clear "Back to Reader" button).
- [ ] **2.2 Mobile navigation controls (`wol-desktop/src/bridge.rs`)**
  - Add touch-friendly **Back**, **Forward**, **Home**, and **Menu/Manager** controls to the injected floating toolbar (since mobile has no `History` menu or `Cmd+[` / `Cmd+]` shortcuts).
- [ ] **2.3 Safe-area & viewport layout (`wol-desktop/ui/*.css`, `wol-desktop/src/bridge.rs`)**
  - Add `viewport-fit=cover` and `env(safe-area-inset-top)` / `env(safe-area-inset-bottom)` padding so toolbars and modals do not collide with the iOS notch/Dynamic Island or Android status/navigation bars.

---

## Phase 3 — External Links, Cloud Sync OAuth & File Import/Export

**Outcome**: Backup import/export and Nextcloud / Google Drive sync work end-to-end on mobile.

- [ ] **3.1 Replace `std::process::Command` browser launch (`wol-core/src/sync.rs`)**
  - `open_browser()` currently spawns `open` / `xdg-open` / `cmd`. Replace with `tauri-plugin-opener` so external URLs open in SFSafariViewController / Chrome Custom Tabs on mobile and the default browser on desktop.
- [ ] **3.2 Mobile OAuth callback (`wol-core/src/sync.rs`)**
  - A `127.0.0.1` loopback HTTP server can be suspended when iOS backgrounds the app during browser login. Register a custom URL scheme / deep link (e.g. `offlinewol://oauth/callback`) via `tauri-plugin-deep-link` for mobile OAuth redirects.
- [ ] **3.3 `.jwlibrary` import & export (`wol-desktop/src/commands.rs`)**
  - Verify `tauri-plugin-dialog` file picker behavior on iOS Files and Android Storage Access Framework for `.jwlibrary` / `.json` import and export.
  - Ensure export writes to a user-accessible location or triggers the native share sheet on iOS/Android.

---

## Phase 4 — Mobile Touch & Highlight UX

**Outcome**: Creating, editing, and deleting highlights and notes feels natural on touchscreens.

- [ ] **4.1 Text selection & popover positioning (`wol-highlights/content.js`, `wol-desktop/src/bridge.rs`)**
  - Test `mouseup` / `pointerup` / `selectionchange` behavior in `WKWebView` and `Android WebView`.
  - Position the highlight color picker and note editor so it does not get obscured by the native OS copy/paste callout or the on-screen keyboard.
- [ ] **4.2 Offline background crawling & battery/storage limits (`wol-core/src/crawler.rs`, `wol-core/src/state.rs`)**
  - Keep default cache cap conservative on mobile (or warn before large Bible/library downloads on cellular connections).

---

## Phase 5 — Testing, Signing & Release

- [ ] **5.1 Rust & desktop regression check**
  - Run `npm run desktop:test` (`cargo test --workspace`) to ensure `wol-core`, the desktop app and the CLI all stay green.
- [ ] **5.2 Mobile smoke test matrix**
  - Verify on iOS Simulator/device and Android Emulator/device:
    1. First-run setup (`setup.html`) → language selection → `wol://` home page loads.
    2. Offline mode (Airplane Mode): previously visited articles and cached subresources render from `DiskCache`.
    3. Import `.jwlibrary` backup → highlights & notes render on matching `data-pid` paragraphs and Bible verses.
    4. Create/edit/delete highlight via touch selection → persists across app restarts.
    5. Cloud sync round-trip (Nextcloud / Google Drive).
- [ ] **5.3 Distribution setup**
  - Configure Apple Developer signing/provisioning (`tauri.conf.json` / Xcode) and Android keystore signing (`gen/android`), then add optional mobile build jobs to `.github/workflows/`.
