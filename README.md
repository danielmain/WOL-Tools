# WOL Highlights & OfflineWOL

A privacy-first **cross-browser WebExtension** and **Rust/Tauri desktop app (`OfflineWOL`)** that render your personal **JW Library** study highlights and notes directly on **Watchtower Online Library ([wol.jw.org](https://wol.jw.org))**.

- **Browser Extension (`wol-highlights/`)**: Supported across **Mozilla Firefox**, **Google Chrome**, **Microsoft Edge**, **Brave**, **Opera**, and **Apple Safari (macOS & iOS)**.
- **Desktop App (`wol-desktop/`)**: Native **OfflineWOL** desktop reader built with Rust + Tauri 2 that automatically caches visited pages as you browse for offline reading, with built-in highlight/note sync and dark mode.
- **Terminal CLI (`wol-cli/`)**: `wol` — the same reader in any terminal. Opens with the same home screen as the desktop app (Bible / Publications / Meetings / search, and today's daily text), browses WOL's directory pages as a two-pane category browser, and reads cached articles with highlights, notes, search and highlight editing, backed by the **same cache, `wolMarks` and sync state** as the desktop app (`wol-core/`).

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
| `npm test` | Runs the extension's Node test suites (`node --test wol-highlights/test/*.test.mjs`) |
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
npm run desktop:test  # Run the whole Rust workspace test suite
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
(`~/Library/Application Support/com.offlinewol.desktop`; the current resolver uses that path on
every platform, and the legacy `com.wolhighlights.desktop` directory is migrated on first use):
one cache, one `wolMarks` set, one sync configuration. Point it elsewhere with
`WOL_DATA_DIR=<DIR>` or the `--data-dir <DIR>` flag (handy for tests and scratch profiles).

Running `wol` with no subcommand opens the **home dashboard** — the terminal
equivalent of launching OfflineWOL. On first run it asks for your language (type to
filter ~1000 languages, `Enter` to confirm) and remembers it in the same
`settings.json` the desktop app uses; afterwards it opens straight to WOL's home
screen and you drive everything with the arrow keys.

```text
 OfflineWOL · BIBLIOTECA EN LÍNEA Watchtower                        español
  BIBLIA   PUBLICACIONES   REUNIONES   Introducir búsqueda   configuración
┌ BIBLIOTECA EN LÍNEA ──────────┐┌ Lunes 16 de noviembre ──────────────────────┐
│ Watchtower                    ││  ─ Lunes 16 de noviembre                    │
│ BIBLIOTECA EN LÍNEA           ││  48 │ Deja todo lo que hagas en manos…      │
│ ¡Bienvenido!                  ││  49 │ A medida que se acerca el fin…        │
│ En esta página podrá…        ││  • Examinemos las Escrituras…               │
│ Anuncio: Nuevos idiomas…      ││                                             │
└───────────────────────────────┘└─────────────────────────────────────────────┘
2026-11-16 · 1/5   ←/→ choose · Enter open · ↓/Tab read · / search · q quit
```

The toolbar, its labels, the search endpoint and the welcome copy are parsed from
the page itself, so the dashboard speaks whatever language you picked. Only
*today's* daily text is shown (WOL ships three days per page), and it is a real
reader: select text and press `1`–`6` or `n` to highlight/annotate it.

```bash
wol                                       # home dashboard (toolbar + today's daily text)
wol tui /es/wol/library/r4/lp-s/biblioteca   # ...or start on a specific page
wol info                                  # data dir, cache size, loaded highlights
wol import ~/Downloads/UserdataBackup.jwlibrary
wol read /en/wol/d/r1/lp-e/2025441        # cache-first; ⟦…⟧ marks in --no-color mode
wol read /es/wol/library/r4/lp-s/biblioteca --links   # list a page's link targets
wol read /en/wol/d/r1/lp-e/2025441 --refresh --legend
wol cache status | wol cache list --limit 20 | wol cache crawl /en/wol/d/r1/lp-e/2025441
wol export out.jwlibrary                  # or --format json for the extension's marks.json
wol sync status | wol sync push
```

WOL's directory pages (library, meetings, Bible versions, publication years) open a
**two-pane browser**: sub-categories on the left, their cards on the right, so
"Reuniones" is not one long link dump but *Vida y Ministerio*, *Estudio de La
Atalaya* and *Otras publicaciones* you can switch between with `j`/`k`.

```text
 OfflineWOL · REUNIONES                                    1/8
┌ REUNIONES ────────────────────┐┌ Vida y Ministerio ────────────────────────────┐
│ Vida y Ministerio             ││▸ 16-22 de noviembre                           │
│▸ 16-22 de noviembre           ││  Guía de actividades 2026 | noviembre         │
│ Estudio de La Atalaya         ││  /es/wol/d/r4/lp-s/202026403                  │
│  Predicamos las buenas noti…  ││                                               │
│ Otras publicaciones           ││                                               │
│  Guía de actividades 2026…    ││                                               │
│  La Atalaya (estudio) 2026…   ││                                               │
└───────────────────────────────┘└───────────────────────────────────────────────┘
1/8  j/k move · Enter open · Esc/Backspace back · / search · r refresh · q quit
```

`Enter` opens the selected card (an article, workbook, Bible book, …);
`Esc`/`Backspace` returns to the same sub-category and cursor. Article pages get the
full reader: highlights, notes, visual selection and search. Pages that are a single
list of entries (e.g. `/library/…/biblioteca/biblias`) show the selected card's
detail on the right instead of repeating the menu.

Global flags: `--no-color` (also honours `NO_COLOR`), `--ascii`, `--width <COLUMNS>`,
`--offline` (never touch the network), `--data-dir <DIR>` (or `WOL_DATA_DIR`).
`wol read` pages through `$PAGER` (default `less -R -F -X`) when stdout is a TTY; set
`WOL_NO_PAGER=1` to print straight through.

Accepted targets: full `https://wol.jw.org/...` URLs, `wol://…` addresses, site-relative
paths (`/en/wol/d/r1/lp-e/2025441`), or a bare document id (`2025441`).

#### TUI keys

**Home dashboard**

| Key | Action |
|---|---|
| `←` / `→` (or `h` / `l`) | Move between toolbar buttons |
| `Enter` / `Space` | Open the selected button (Bible, Publications, Meetings…, search, settings) |
| `↓` / `Tab` / `j` | Focus the daily text (normal reader keys apply) |
| `Esc` / `Tab` | Back to the toolbar |
| `[` / `]` | Previous / next day of the daily text |
| `/` | Site search (asks for a query) |
| `r` | Re-fetch the home page |
| `Ctrl-l` | Change language |

The gear button opens a settings sheet with the language picker, a shortcut back to the home
page, and the data directory / cache size.

**Directory browser**

| Key | Action |
|---|---|
| `j` / `k`, `↓` / `↑` | Move through sub-categories and cards |
| `g` / `G` | First / last entry |
| `Enter`, `l`, `→` | Open the selected card |
| `Esc`, `Backspace`, `h`, `←` | Back (selection is restored) |
| `/` | Site search · `r` refresh · `Ctrl-l` language · `q` quit |

**Reader (articles, Bible chapters, daily text)**

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
- **Astral characters** (emoji, rare CJK) are two UTF-16 code units and therefore **two**
  tokens — neither half of a surrogate pair is a letter.
- **Rust parity**: `wol-core::tokenize` is a byte-for-byte port of the same rules
  (it uses Unicode general categories, so combining marks stay separate tokens). It is
  enforced by `cargo test -p wol-core --test tokenize_parity`, with fixtures generated from
  `lib/tokenize.js` by `npm run test:tokenize:update`.

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
│   └── src/                 # main.rs, context.rs, commands.rs, render/,
│                            #   tui/ (home dashboard, directory browser, reader,
│                            #   sidebar, notes, language picker)
├── wol-desktop/             # Rust + Tauri 2 OfflineWOL desktop app
│   ├── Cargo.toml           # Rust crate manifest
│   ├── tauri.conf.json      # Tauri configuration
│   ├── src/                 # lib.rs (app setup, wol:// protocol, menus), bridge.rs (JS bridge),
│   │                        #   commands.rs (IPC), main.rs — shared logic comes from wol-core
│   ├── tests/               # Acceptance & integration tests (cargo test)
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
        ├── run.html              # In-browser parser & highlight-placement test runner
        ├── backup.test.mjs       # .jwlibrary / SQLite indexer tests (node --test)
        ├── content.test.mjs      # DOM highlight & toolbar behaviour tests
        ├── cli-export.test.mjs   # CLI `.jwlibrary` export re-imported by lib/backup.js
        ├── jwpub-crypto.test.mjs # Encrypted JWPUB decoding tests
        ├── sync.test.mjs         # Sync decision/merge tests
        └── tokenize-parity.mjs   # Regenerates the Rust tokenizer parity fixtures
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

**Using the app (end users):** nothing to configure. The Google OAuth client is baked into the
released extension, the OfflineWOL desktop app and the `wol` CLI at build time, so
*Sign in with Google Drive* just works. Your own refresh token stays on your device (extension
local storage / the app data directory); it is never sent anywhere except Google.

**Building from source (maintainers):** put the OAuth *Desktop app* client in a git-ignored `.env`
at the repo root — either copy the values, or point the generator straight at the JSON you download
from Google Cloud Console:

```bash
node scripts/gen-oauth.js --from-json ~/Downloads/client_secret_*.json
```

The script writes `wol-highlights/lib/oauth.generated.js` (git-ignored) from
`WOL_GOOGLE_CLIENT_ID` / `WOL_GOOGLE_CLIENT_SECRET`, falling back to `CHROME_CLIENT_ID` /
`CHROME_CLIENT_SECRET`. `npm run build`, `npm run lint`, `npm start` and the AMO deploy app all run
it automatically, so a plain `npm install` is enough for browser development.

For Rust builds, `wol-core/build.rs` reads the same keys from the environment (or the workspace
`.env`) and bakes them into the binary; `cargo build` therefore "just works" locally. On CI, set
`WOL_GOOGLE_CLIENT_ID` / `WOL_GOOGLE_CLIENT_SECRET` as repository secrets — without them the build
still succeeds, but Google Drive sync reports "not configured in this build".

Runtime environment variables still override the baked-in client, which is handy for testing a
personal client without rebuilding. The browser extension accepts the same override in storage:

```js
browser.storage.local.set({ wolGoogleOAuth: { clientId: "YOUR_CLIENT_ID", clientSecret: "YOUR_CLIENT_SECRET" } });
```

Notes:

- The client secret of an *installed/desktop* OAuth client is **not** a confidential secret — it
  ships inside every copy of the app either way. Never commit it anyway, and never put the
  **refresh token** of the Chrome Web Store publishing client (`CHROME_REFRESH_TOKEN`, used only by
  `npm run publish:*`) in a package: that one really is privileged.
- iOS builds need their own *iOS* OAuth client id (public PKCE client, no secret): set
  `WOL_GOOGLE_CLIENT_ID` when building, because Google only accepts the reversed-client-id
  redirect scheme for registered iOS clients.
- `npm run pull:env` regenerates `.env` from Bitwarden and only understands the store-publishing
  keys; re-add `WOL_GOOGLE_*` (or re-run the generator with `--from-json`) afterwards.
- Nextcloud sync needs no Google credentials at all.
