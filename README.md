# WOL Highlights & JW Library UserMark Tools

A privacy-first, cross-browser WebExtension and reverse-engineering toolkit that renders your personal **JW Library** study highlights directly on **Watchtower Online Library ([wol.jw.org](https://wol.jw.org))**.

Supported across **Mozilla Firefox**, **Google Chrome**, **Microsoft Edge**, **Brave**, **Opera**, and **Apple Safari (macOS & iOS)**.

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
2. **Deterministic Unicode Tokenization**: Precise tokenization engine matching JW Library's word/punctuation boundaries and skipping non-counting annotations (paragraph numbers, footnote letters, cross-references).
3. **Reactive Content Script**: Dynamically detects client-side page transitions on `wol.jw.org` using `MutationObserver` and paints matching text using native JW Library colors.
4. **Zero Network Telemetry**: 100% offline; your personal study backups are never uploaded anywhere.

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

---

## Usage Guide

1. Export a backup from your JW Library app (*Settings &rarr; Backup and Restore*).
2. Click the **WOL Highlights** icon in your browser toolbar.
3. Click **Load backup** and choose your `.jwlibrary` file (or `userData.db`).
   - *Tip:* Click **Open in permanent tab** in the popup header if you prefer managing backups in a full tab.
4. The popup confirms the loaded file name, total highlight count, affected articles, Bible chapters, and color breakdown.
5. Open any matching study article or Bible chapter on [wol.jw.org](https://wol.jw.org).
6. Highlights appear automatically matching your JW Library colors.
7. Use the **Show highlights** toggle in the popup to show or hide highlights at any time.

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
├── .gitignore               # Ignored build outputs, local test databases, and secrets
├── AGENT.md                 # Technical guide and operational rules for AI agents
├── README.md                # Project documentation (this file)
├── flake.nix                # Nix Flake devShell & AMO deployment app
├── package.json             # Build, lint, dev runner, bump, and multi-store publish scripts
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
└── wol-highlights/          # Cross-browser WebExtension source (Manifest V3)
    ├── manifest.json        # Extension manifest
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
