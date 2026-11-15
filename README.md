# WOL Highlights & OfflineWOL

A privacy-first **cross-browser WebExtension** and **Rust/Tauri desktop app (`OfflineWOL`)** that render your personal **JW Library** study highlights and notes directly on **Watchtower Online Library ([wol.jw.org](https://wol.jw.org))**.

- **Browser Extension (`wol-highlights/`)**: Supported across **Mozilla Firefox**, **Google Chrome**, **Microsoft Edge**, **Brave**, **Opera**, and **Apple Safari (macOS & iOS)**.
- **Desktop App (`wol-desktop/`)**: Native **OfflineWOL** desktop reader built with Rust + Tauri 2 that automatically caches visited pages as you browse for offline reading, with built-in highlight/note sync and dark mode.
- **Terminal CLI (`wol-cli/`)**: `wol` — the same reader in any terminal. Cached articles with highlights, notes, search and highlight editing, backed by the **same cache, `wolMarks` and sync state** as the desktop app (`wol-core/`).

---

> [!TIP]
> **Quick Start for Local Development:**
> - **Firefox**: `about:debugging` &rarr; **This Firefox** &rarr; **Load Temporary Add-on...** &rarr; select `wol-highlights/manifest.json`.
> - **Chrome / Edge / Brave**: `chrome://extensions` &rarr; toggle **Developer mode** &rarr; click **Load unpacked** &rarr; select the `wol-highlights/` directory.
> - **Safari**: Build and run the native wrapper via `npm run build:safari` or open `safari/WOL Highlights/WOL Highlights.xcodeproj`.

---

## Browser Support Matrix

| Browser / Platform | Support Level | Distribution / Loading |
|---|---|---|
| **Mozilla Firefox** (Desktop & Android) | Native MV3 | AMO Store, self-hosted signed `.xpi`, or temporary add-on |
| **Google Chrome / Chromium / Brave** | Native MV3 | Chrome Web Store or unpacked developer folder |
| **Microsoft Edge** | Native MV3 | Edge Add-ons store or unpacked developer folder |
| **Apple Safari** (macOS 12+ & iOS 15+) | Native Safari Web Extension | Packaged native App & Extension in `safari/` |

---

## Key Features & Architecture

JW Library backup files (`.jwlibrary`) do not store publication text. Instead, they store lightweight relational entries consisting of color identifiers and token-based word offsets pointing at publications, paragraphs, or Bible verses.

1. **100% Client-Side SQLite Parsing**: Uses WebAssembly SQLite (`sql.js`) to parse `.jwlibrary` (Zip) or `userData.db` directly in your browser.
2. **Interactive In-Browser Marker Creation**: Select any text on `wol.jw.org` (across existing or brand-new articles and Bible chapters) to create, change, or remove highlights using JW Library's 6 colors with a floating selection toolbar.
3. **Deterministic Unicode Tokenization**: Precise tokenization engine matching JW Library's word/punctuation boundaries and skipping non-counting annotations (paragraph numbers, footnote letters, cross-references).
4. **Reactive Content Script**: Dynamically detects client-side page transitions on `wol.jw.org` using `MutationObserver` and paints matching text using native JW Library colors.
5. **Zero Network Telemetry**: 100% offline; your personal study backups and highlights are stored locally in the browser and never uploaded anywhere.

---

## Development & Automation Workflow

This repository includes modern cross-browser build, lint, versioning, and store-deployment tooling modeled after production extension workflows.

### 1. Nix Flake Environment

If you use [Nix](https://nixos.org/), enter the reproducible dev shell with all required toolchains (`nodejs`, `web-ext`, `jq`, `zip`, `resvg`, `rbw`, `bitwarden-cli`):

```bash
nix develop
```

Deploy directly to Mozilla Add-ons (AMO) using the Flake app:
```bash
nix run .#deploy
```

---

### 2. NPM Scripts

| Command | Action |
|---|---|
| `npm run desktop` | Runs the OfflineWOL Tauri desktop app (`cargo run -p wol-desktop`) |
| `npm run desktop:dev` | Runs OfflineWOL in Tauri dev mode (`cargo tauri dev`) |
| `npm run desktop:test` | Runs the whole Rust workspace test suite (`cargo test --workspace`) |
| `npm run cli` | Runs the terminal reader (`cargo run -p wol-cli -- …`) |
| `npm run cli:test` | Runs the `wol-cli` unit tests |
| `npm run test:tokenize` | Verifies the Rust tokenizer against the shipped `lib/tokenize.js` fixtures |
| `npm run test:tokenize:update` | Regenerates `wol-core/tests/fixtures/tokenize_cases.json` from `lib/tokenize.js` |
| `npm start` | Launches Firefox with extension pre-loaded, live-reloading, and opened to `wol.jw.org` |
| `npm run start:chrome` | Launches Chrome/Chromium with extension pre-loaded and live-reloading |
| `npm run build` | Builds packages for Chrome/Chromium, Firefox (`dist/*.xpi`), and Safari macOS |
| `npm run build:chrome` | Bundles clean Chromium zip into `dist/` |
| `npm run build:firefox` | Generates both `.zip` and `.xpi` add-on archives in `dist/` |
| `npm run build:safari` | Builds macOS Safari App & Extension using `xcodebuild` |
| `npm run lint` | Runs `web-ext lint` with zero errors/warnings |
| `npm run generate:icons` | Generates all standard multi-resolution icons (16, 32, 48, 128 px) |
| `npm run bump` | Bumps patch version in `manifest.json`, `package.json`, and Safari Xcode project |
| `npm run bump:minor` | Bumps minor version across all project manifests |
| `npm run bump:major` | Bumps major version across all project manifests |
| `npm run pull:env` | Automatically pulls store API secrets from Bitwarden CLI (`bw`) into `.env` |
| `npm run auth:chrome` | Interactive OAuth2 helper to generate Chrome Web Store refresh token |
| `npm run publish:firefox` | Automated build and submission to Mozilla Add-ons (AMO) |
| `npm run publish:chrome` | Automated upload and publish to Google Chrome Web Store |
| `npm run publish:edge` | Automated upload and publish to Microsoft Edge Add-ons |
| `npm run publish` | One-command publication pipeline across Firefox, Chrome, and Edge |

---

## Installation Guide

### Option A: Mozilla Firefox

1. Go to `about:debugging#/runtime/this-firefox`.
2. Click **Load Temporary Add-on...**.
3. Select `wol-highlights/manifest.json` (or `dist/wol_highlights-*.xpi` built via `npm run build:firefox`).

### Option B: Google Chrome / Edge / Brave

1. Open `chrome://extensions` (or `edge://extensions`).
2. Turn on the **Developer mode** toggle in the top-right corner.
3. Click **Load unpacked** and select the **`wol-highlights`** folder.

### Option C: Apple Safari (macOS)

1. Run `npm run build:safari` (or open `safari/WOL Highlights/WOL Highlights.xcodeproj` in Xcode).
2. In Safari, open **Settings &rarr; Advanced** and check **Show features for web developers**.
3. In Safari's **Developer** menu, enable **Allow Unsigned Extensions**.
4. Open **Settings &rarr; Extensions** and enable **WOL Highlights**.

### Option D: OfflineWOL Desktop App (Tauri 2)

Prebuilt installers and binaries for **OfflineWOL** are published on GitHub Releases (triggered by `v*` tags or manual dispatch via `.github/workflows/release.yml`) for:
- **macOS**: Apple Silicon (`aarch64-apple-darwin`) and Intel (`x86_64-apple-darwin`)
- **Linux**: `x86_64` (`x86_64-unknown-linux-gnu`) and ARM64 (`aarch64-unknown-linux-gnu`)
- **Windows**: `x86_64` (`x86_64-pc-windows-msvc`) and ARM64 (`aarch64-pc-windows-msvc`)

To build or run from source, Rust (`cargo`) and Tauri 2 prerequisites are required:

```bash
npm run desktop:dev   # Launch in development mode
npm run desktop       # Run directly via Cargo
npm run desktop:test  # Run Rust unit & integration tests
```

OfflineWOL intercepts `wol.jw.org` navigation via a custom `wol://` protocol, caches visited pages and subresources on disk (stale-while-revalidate with LRU eviction and a Cache Inspector UI), and shares the same `wolMarks` JSON format as the browser extension.

To add a local publication, open **Highlights & Settings → Settings → Load JWPUB**, select a `.jwpub` file, then open the Publications library and choose **Imported publications**. Imported articles are stored separately from the offline cache. Select text to create a highlight or use the note button to add a paragraph note. Load a JW Library backup to see existing study data, and export a backup from the Highlights & Backups tab.

### Option E: Terminal CLI (`wol`)

The `wol` binary is the terminal version of OfflineWOL. Prebuilt portable archives
(`wol-<version>-<target>.tar.gz` / `.zip`) are attached to the same GitHub Releases as
OfflineWOL; or build it from source:

```bash
cargo build --release -p wol-cli     # -> target/release/wol
npm run cli -- --help                # or: cargo run -p wol-cli -- --help
nix build .#wol && ./result/bin/wol  # Nix users
```

It shares `wol-core` with the desktop app, so it reads and writes the **same data directory**
(`~/Library/Application Support/com.offlinewol.desktop` on macOS,
`~/.local/share/com.offlinewol.desktop` on Linux, `%APPDATA%\com.offlinewol.desktop` on
Windows): one cache, one `wolMarks` set, one sync configuration. Use `--data-dir <DIR>` to
point at a scratch copy (handy for testing).

Running `wol` with no subcommand opens the interactive reader — the terminal
equivalent of launching OfflineWOL. On first run it asks for your language
(type to filter ~1000 languages, `Enter` to confirm) and remembers it in the same
`settings.json` the desktop app uses; afterwards it opens straight to your home
page and you can browse WOL with the arrow keys.

```bash
wol                                       # interactive reader at your home page
wol tui /es/wol/library/r4/lp-s/biblioteca   # ...or start somewhere specific
wol info                                  # data dir, cache size, loaded highlights
wol import ~/Downloads/UserdataBackup.jwlibrary
wol read /en/wol/d/r1/lp-e/2025441        # cache-first; ⟦…⟧ marks in --no-color mode
wol read /es/wol/library/r4/lp-s/biblioteca --links   # list a page's link targets
wol read /en/wol/d/r1/lp-e/2025441 --refresh --legend
wol cache status | wol cache list --limit 20 | wol cache crawl /en/wol/d/r1/lp-e/2025441
wol export out.jwlibrary                  # or --format json for the extension's marks.json
wol sync status | wol sync push
```

Browsing works exactly like the website: the library index, publication and year
lists are server-rendered link pages, so `Tab` turns the cursor into a link cursor
and `Enter` opens the entry (`Backspace` goes back). Article pages get the full
reader: highlights, notes, visual selection and search.

Global flags: `--no-color` (also honours `NO_COLOR`), `--ascii`, `--width <COLUMNS>`,
`--offline` (never touch the network), `--data-dir <DIR>`.

Accepted targets: full `https://wol.jw.org/...` URLs, `wol://…` addresses, site-relative
paths (`/en/wol/d/r1/lp-e/2025441`), or a bare document id (`2025441`).

#### TUI keys

| Key | Action |
|---|---|
| `j` / `k`, `↓` / `↑` | Scroll one line (move the link cursor while browsing links) |
| `Ctrl-d` / `Ctrl-u` | Scroll half a page |
| `Space` / `PgDn`, `PgUp` | Scroll a full page |
| `gg` / `G` | Jump to top / bottom |
| `J` / `K` | Next / previous paragraph |
| `h` / `l`, `←` / `→` | Move the token cursor |
| `Tab` | Toggle link browsing: `j`/`k` walk the page's links, `Enter` opens, `Esc` leaves |
| `v` | Start/cancel a visual selection |
| `1`–`6` | Apply that highlight colour to the selection |
| `p` | Open the colour picker (legend from `--legend`) |
| `0` / `x` | Remove highlights from the selection |
| `n` | Edit the note for the current paragraph (`Enter` saves, empty removes) |
| `/` then `Enter` | Search the rendered text |
| `N` / `Ctrl-n` | Previous / next search hit |
| `f` | References & notes drawer for the current paragraph |
| `Enter` | Follow the link under the cursor |
| `Backspace`, `H` / `L` | History back / forward |
| `gh` | Home page for the configured language |
| `Ctrl-l` | Change language (opens the picker, saved to shared settings) |
| `r` | Re-fetch the page from the network |
| `b` | Toggle the sidebar (cached pages · contents · highlights) |
| `Tab` (in sidebar) | Cycle sidebar tabs · `j`/`k` move · `Enter` opens/jumps |
| `q` / `Ctrl-c` | Quit |

Highlight and note edits are written straight back to `wolMarks`, so the desktop app and the
browser extension pick them up on their next read.

---

## Usage Guide

1. **Importing Highlights**:
   - Export a backup from your JW Library app (*Settings &rarr; Backup and Restore*).
   - Click the **WOL Highlights** icon in your browser toolbar.
   - Click **Load backup** and choose your `.jwlibrary` file, `userData.db`, or a previously exported `.json` highlights file.
   - *Tip:* Click **Open in permanent tab** in the popup header if you prefer managing backups in a full tab.
   - The popup displays the loaded file name, total highlight count, affected articles, Bible chapters, and color breakdown.
2. **Reading & Viewing**:
   - Open any matching study article or Bible chapter on [wol.jw.org](https://wol.jw.org).
   - Highlights appear automatically matching your JW Library colors.
   - Use the **Show highlights** toggle in the popup to show or hide highlights at any time.
3. **Creating & Removing Highlights**:
   - Select text on any article or Bible chapter (both existing from backup or brand-new) to open the floating toolbar.
   - Click any of the 6 color buttons to highlight the selection.
   - Click on any existing highlight (from backup or newly created) or select it to open the toolbar, then click the **Remove** button (✕) to delete it.
4. **Exporting Highlights**:
   - In the popup, click **Export highlights (JSON)** to save your current highlights and markers as a portable JSON file.


---

## Technical Architecture & How It Works

### Database Schema & Relationship

A JW Library backup stores highlights without publication text using three primary tables:

```text
UserMark (ColorIndex, UserMarkGuid)
   │
   ├── LocationId ──► Location (DocumentId, KeySymbol, BookNumber, ChapterNumber, MepsLanguage)
   │
   └── UserMarkId ──► BlockRange (Identifier, BlockType, StartToken, EndToken)
```

| Page Type | Matching Criteria |
|---|---|
| **Article** (`/wol/d/.../<documentId>`) | `Location.DocumentId`, matched with paragraph DOM nodes `p[data-pid="<Identifier>"]` |
| **Bible Chapter** (`/wol/b/.../<symbol>/<book>/<chapter>`) | `KeySymbol`, `BookNumber`, `ChapterNumber`, matched with verse spans `span.v[id^="v<book>-<chapter>-<verse>"]` |

### Tokenization Engine (`lib/tokenize.js`)

JW Library addresses text using **0-indexed token offsets** (inclusive range `[StartToken, EndToken]`):
- **Word tokens**: Consecutive sequences of Unicode letters, digits, and connector punctuation (`[\p{L}\p{N}\p{Pc}]`).
- **Punctuation tokens**: Any non-whitespace character that is not a word token (e.g. periods, quotation marks, brackets, hyphens).
- **Whitespace**: Skipped and does not advance the token counter.
- **Special tokens**: Soft hyphens (`\u00AD`) are counted as individual tokens.

### DOM Range Alignment (`lib/highlight.js`)

When visiting an article or chapter:
1. A DOM `TreeWalker` traverses text nodes within the target paragraph or verse, skipping non-counted elements:
   - Paragraph numbers (`.parNum`), page numbers (`.pageNum`), footnote letters (`.fn`).
   - Chapter/verse number anchors (`a.vl`, `a.cl`), ruby text (`rt`, `rp`).
   - Study Bible cross-reference markers containing only `+` (`a.b`).
2. Slices are mapped and text nodes are split via `Text.splitText()` in descending document order and wrapped in `<mark class="wol-hl wol-c{1-6}">`.
3. Highlights crossing embedded hyperlinks (`<a>`) wrap each text fragment cleanly without breaking DOM structure.

### Color Mapping

Highlights replicate native JW Library colors:

| Index | Color | CSS Variable | Hex |
|---|---|---|---|
| `1` | Yellow | `--hl-yellow` | `#ffe36b` |
| `2` | Green | `--hl-green` | `#b6e89a` |
| `3` | Blue | `--hl-blue` | `#a9d4fb` |
| `4` | Red | `--hl-red` | `#f7b4c4` |
| `5` | Orange | `--hl-orange` | `#ffc48a` |
| `6` | Purple | `--hl-purple` | `#d8c0f8` |

---

## Repository Structure

```text
.
├── .github/workflows/       # CI/CD workflows (release.yml: OfflineWOL + `wol` CLI builds)
├── .gitignore               # Ignored build outputs, local test databases, and secrets
├── AGENTS.md                # Unified technical guide and operational rules for AI agents
├── README.md                # Project documentation (this file)
├── TODO.md                  # Tauri desktop migration plan and WOL research notes
├── CLI_PLAN.md              # Terminal CLI / wol-core plan (phases, renderer, TUI spec)
├── Cargo.toml               # Cargo workspace: wol-core, wol-desktop, wol-cli
├── flake.nix                # Nix Flake devShell, `wol` package/app, AMO deployment app
├── package.json             # Build, lint, dev runner, desktop, CLI, bump, publish scripts
├── usermark.md              # Deep dive into UserMark schema, tokens, and WOL DOM mapping
├── scripts/                 # Cross-browser build & automated store publishing tools
│   ├── bump-version.js      # Version bumper across manifest, package.json, and Safari
│   ├── publish-firefox.js   # Mozilla Add-ons (AMO) sign & publish automation
│   ├── publish-chrome.js    # Chrome Web Store upload & publish automation
│   ├── publish-edge.js      # Microsoft Edge Add-ons upload & publish automation
│   ├── publish-all.js       # Unified multi-store publishing script
│   ├── pull-bw-env.js       # Bitwarden CLI secret synchronizer
│   └── auth-chrome.js       # Interactive OAuth2 generator for Chrome Web Store
├── safari/                  # Native Safari Web Extension Xcode project (macOS & iOS)
│   └── WOL Highlights/      # Xcode project referencing wol-highlights/ assets
├── wol-core/                # GUI-agnostic engine shared by desktop & CLI
│   ├── src/                 # cache, handler, crawler, url_map, state, sync, backup (wolMarks),
│   │                        #   tokenize, dom, highlight, jwlibrary export, languages
│   └── tests/               # tokenizer parity fixtures replayed against lib/tokenize.js
├── wol-cli/                 # `wol` terminal reader (clap CLI + ratatui TUI)
│   └── src/                 # main.rs, context.rs, commands.rs, render/, tui/
├── wol-desktop/             # Rust + Tauri 2 OfflineWOL desktop app
│   ├── Cargo.toml           # Rust crate manifest
│   ├── tauri.conf.json      # Tauri configuration
│   ├── src/                 # Protocol handler, disk cache, URL rewriter, crawler, bridge
│   ├── tests/               # Acceptance & integration tests
│   └── ui/                  # Manager & setup UI + reused wol-highlights/lib/* assets
└── wol-highlights/          # Cross-browser WebExtension source (Manifest V3)
    ├── manifest.json        # Extension manifest
    ├── background.js        # MV3 background script / service worker
    ├── popup.html           # Toolbar popup interface
    ├── popup.css            # Toolbar popup styling
    ├── popup.js             # Backup loader, storage manager, and popup controller
    ├── content.css          # Injected highlight styles
    ├── content.js           # Content script coordinating page detection and highlight lifecycle
    ├── icons/               # Multi-resolution icons (16, 32, 48, 128 px)
    ├── lib/
    │   ├── backup.js        # Stream-based zip unpacker (DecompressionStream) & SQLite indexer
    │   ├── highlight.js     # DOM TreeWalker and text node split/wrap engine
    │   ├── languages.js     # MEPS language ID to symbol mapper
    │   ├── tokenize.js      # Unicode tokenization engine
    │   ├── sql-wasm.js      # sql.js WebAssembly SQLite loader
    │   └── sql-wasm.wasm    # sql.js WebAssembly SQLite binary
    └── test/
        └── run.html         # Test runner verifying parsing and highlight placement
```

---

## Privacy & Security

- **100% Client-Side**: All parsing and highlight lookups occur strictly in your browser using local storage (`ext.storage.local`).
- **Zero Telemetry**: No network requests are made. Your personal backup files and study marks are never transmitted to external servers.
- **Minimal Permissions**: Requests only `storage`, `tabs`, and access to `https://wol.jw.org/*`.

---

## License

This project is licensed under the [MIT License](LICENSE). Embedded `sql.js` files are distributed under the MIT License by the sql.js authors.

### Google Drive sync credentials

Google Drive sync requires your own Google OAuth desktop client credentials. Create a new client and keep its credentials private; do not commit them or include them in extension packages. Desktop OAuth clients are public clients: a secret stored on a user's computer cannot be kept confidential from that user.

For OfflineWOL, set `WOL_GOOGLE_CLIENT_ID` and `WOL_GOOGLE_CLIENT_SECRET` in the environment before launching the app. An ignored `.env` file can hold these values for local development, but must be loaded into the environment by your shell before starting the app.

For the browser extension, set `wolGoogleOAuth` in the extension's local storage using its background/service-worker developer console (use `browser` on Firefox and `chrome` on Chromium):

```js
browser.storage.local.set({ wolGoogleOAuth: { clientId: "YOUR_CLIENT_ID", clientSecret: "YOUR_CLIENT_SECRET" } });
```

These credentials remain in local extension storage. Reconfigure them on each browser installation. Google Drive sync reports an error when credentials are missing; Nextcloud sync does not require them.
