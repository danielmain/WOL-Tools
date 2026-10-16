# WOL Highlights & JW Library UserMark Tools

A privacy-focused browser extension and reverse-engineering research toolkit that brings your **JW Library** personal study highlights directly to **Watchtower Online Library ([wol.jw.org](https://wol.jw.org))**.

---

> [!WARNING]
> **Important when installing the extension in Firefox:**  
> When navigating to `about:debugging` &rarr; **This Firefox** &rarr; **Load Temporary Add-on...**, **DO NOT** select the `manifest.json` in the root folder. That root file is the **JW Library backup manifest** from your exported device backup.  
> **Select:** `wol-highlights/manifest.json`.  
> Selecting the root manifest will fail with: `Unexpected params.manifestVersion value: undefined`.

---

## Overview

JW Library backups (`.jwlibrary`) do not store publication text; they store color identities and token-based pointers pointing at publications, paragraphs, and Bible verses. This repository contains:

1. **`wol-highlights/`**: A Manifest V3 Firefox extension that parses JW Library backups (`.jwlibrary` or `userData.db`) 100% locally in-browser using WebAssembly SQLite (`sql.js`), maps highlight word ranges to live DOM nodes on `wol.jw.org`, and paints them in native JW Library colors.
2. **`usermark.md`**: Comprehensive architectural reverse-engineering documentation detailing the JW Library SQLite schema (`UserMark`, `Location`, `BlockRange`), tokenization rules, and web DOM mapping heuristics.
3. **Reference Backup Files**:
   - `userData.db`: SQLite database extracted from backup.
   - `userdata.zip` / `manifest.json`: Device backup archive and backup metadata.

---

## Quick Start & Installation

### Option 1: One-Click Development Launch (Linux CLI)

If you are developing or testing locally on Linux, run:
```bash
./run-firefox.sh
```
This script automatically packages the extension, launches an isolated Firefox session with `wol-highlights` pre-installed, and opens `https://wol.jw.org`.

---

### Option 2: Standard Firefox (`about:debugging`)

This method works immediately in all versions of Firefox (including standard release) without requiring developer flags or signing:

1. Open Firefox and type `about:debugging` in the address bar (or navigate to `about:debugging#/runtime/this-firefox`).
2. Click **This Firefox** in the left sidebar.
3. Click **Load Temporary Add-on...**.
4. Select either:
   - The packaged file: **`wol-highlights.xpi`** (in the workspace root), OR
   - The manifest: **`wol-highlights/manifest.json`** (inside the `wol-highlights` folder).
5. The extension icon will appear in your Firefox toolbar.

> [!NOTE]
> Firefox unloads temporary extensions when the browser is completely quit and restarted. To reload, repeat step 3.

---

### Option 3: Permanent Installation (Survives Browser Restarts)

By default, Firefox Release blocks permanent installation of unsigned add-ons with:
`This add-on could not be installed because it has not been verified.`

To install permanently so it persists across restarts:

- **A. Self-Distribution Signing on AMO (Recommended for Release Firefox)**:
  1. Go to [addons.mozilla.org/developers/addon/submit/distribution](https://addons.mozilla.org/developers/addon/submit/distribution).
  2. Choose **"On your own"** (self-distribution / unlisted).
  3. Upload `wol-highlights.xpi`.
  4. Mozilla's automated system signs it in ~2 minutes and gives you a signed `.xpi`.
  5. Drag-and-drop the signed `.xpi` into Firefox or use `about:addons` &rarr; ⚙️ &rarr; **Install Add-on From File...**. It installs permanently!

- **B. Using Firefox Developer Edition / Nightly / ESR**:
  1. Open `about:config`.
  2. Search for `xpinstall.signatures.required` and toggle it to `false`.
  3. Open `about:addons`, click the gear icon ⚙️, and select **Install Add-on From File...**.
  4. Select `wol-highlights.xpi`. It installs permanently.

---

### Packaging the Extension (`build.sh`)

Whenever you make changes to extension files in `wol-highlights/`, run:
```bash
./build.sh
```
This regenerates `wol-highlights.xpi`.

---

### Using the Extension

1. Click the **WOL Highlights** icon in the browser toolbar.
2. Click **Load backup** and select either:
   - Your raw `.jwlibrary` backup file exported from JW Library (*Settings &rarr; Backup and Restore*), or
   - An extracted `userData.db` file.
3. The popup will display the loaded file name, total highlight count, affected articles and Bible chapters, and a color breakdown.
4. Open any corresponding study article or Bible chapter on [wol.jw.org](https://wol.jw.org) (e.g. Spanish or English editions).
5. Highlights appear automatically matching your JW Library colors.
6. Use the **Show highlights** toggle in the popup to show/hide highlights at any time.

---

## Repository Structure

```text
.
├── AGENT.md                 # Technical guide and operational rules for AI agents & contributors
├── README.md                # Project documentation (this file)
├── build.sh                 # Packaging script creating wol-highlights.xpi
├── run-firefox.sh           # Dev launcher running Firefox with extension pre-installed
├── wol-highlights.xpi       # Packaged Firefox extension (ready to load or sign)
├── usermark.md              # Deep dive into UserMark SQLite schema, tokens, and WOL mapping
├── manifest.json            # Backup manifest metadata from JW Library (NOT the extension manifest)
├── userData.db              # Reference SQLite database from JW Library backup
├── userdata.zip             # Reference zip package containing backup data
├── default_thumbnail.png    # Backup thumbnail
└── wol-highlights/          # Firefox WebExtension source code
    ├── manifest.json        # WebExtension Manifest V3 (Select THIS in about:debugging)
    ├── popup.html           # Toolbar popup interface
    ├── popup.css            # Styles for toolbar popup
    ├── popup.js             # Backup loader, storage manager, and popup controller
    ├── content.css          # Highlight color definitions and mark element styling
    ├── content.js           # Content script coordinating page detection and highlight lifecycle
    ├── icons/
    │   └── icon-48.png      # Extension icon
    ├── lib/
    │   ├── backup.js        # Zip unpacker (DecompressionStream) and SQLite indexer
    │   ├── highlight.js     # DOM TreeWalker, token slice mapper, and splitText node wrapper
    │   ├── languages.js     # MEPS language ID to symbol mapper (English: 0, Spanish: 1)
    │   ├── tokenize.js      # Unicode tokenization matching JW Library's word/punctuation rules
    │   ├── sql-wasm.js      # sql.js WebAssembly SQLite engine loader
    │   └── sql-wasm.wasm    # sql.js WebAssembly binary
    └── test/
        └── run.html         # In-browser test runner verifying parsing and highlight placement
```

---

## How It Works

### 1. Database Schema & Indexing

A JW Library backup records highlights without publication text. A highlight consists of:

```text
UserMark (Color & Guid)
   │
   ├── LocationId ──► Location (DocumentId, KeySymbol, BookNumber, ChapterNumber, Language)
   │
   └── UserMarkId ──► BlockRange (Paragraph/Verse Identifier, StartToken, EndToken)
```

- **Articles**: Identified by `Location.DocumentId`. The paragraph number corresponds to `BlockRange.Identifier` (`BlockType = 1`), matching `p[data-pid]` in the WOL DOM.
- **Bible Chapters**: Identified by `Location.KeySymbol` (e.g. `nwtsty`), `BookNumber`, and `ChapterNumber`. Verses correspond to `BlockRange.Identifier` (`BlockType = 2`), matching `span.v[id^="v{book}-{chapter}-{verse}"]`.

### 2. Tokenization Engine (`lib/tokenize.js`)

JW Library addresses text using **0-indexed token offsets** (inclusive range `[StartToken, EndToken]`):
- A **word token** is a continuous sequence of Unicode letters, numbers, or connector punctuation (`[\p{L}\p{N}\p{Pc}]`).
- A **punctuation token** is any individual non-whitespace character (periods, commas, quotation marks, hyphens, brackets).
- **Whitespace is skipped** (regular spaces, non-breaking spaces `\u00A0`, narrow non-breaking spaces `\u202F`, etc.).

### 3. DOM Range Mapping (`lib/highlight.js`)

When visiting an article or chapter:
1. `TreeWalker` walks text nodes within the target paragraph or verse, automatically skipping non-counted elements:
   - Paragraph numbers (`.parNum`), page numbers (`.pageNum`), footnote letters (`.fn`).
   - Chapter/verse number anchors (`a.vl`, `a.cl`), ruby text (`rt`, `rp`).
   - Cross-reference markers whose only text is `+` (`a.b`).
2. The extracted text is tokenized and aligned with `StartToken` and `EndToken`.
3. DOM text nodes are sliced using `Text.splitText()` and wrapped in `<mark class="wol-hl wol-c{1-6}">`.
4. If a highlight crosses an embedded hyperlink (`<a>`), multiple discrete `<mark>` tags are wrapped seamlessly across nodes without breaking HTML structure.

### 4. Color Palette

Highlights match the official JW Library color palette:

| Index | Color | CSS Hex |
|---|---|---|
| `1` | Yellow | `#ffe36b` |
| `2` | Green | `#b6e89a` |
| `3` | Blue | `#a9d4fb` |
| `4` | Red | `#f7b4c4` |
| `5` | Orange | `#ffc48a` |
| `6` | Purple | `#d8c0f8` |

---

## Privacy & Security

- **100% Offline**: All processing occurs strictly inside your browser tab and extension storage (`chrome.storage.local`).
- **No Network Requests**: Your backups and highlights are never uploaded to any server or external API.
- **Minimal Permissions**: Requests only `storage`, `tabs`, and `host_permissions` for `https://wol.jw.org/*`.

---

## Testing

The extension includes a standalone test harness:
- Open `wol-highlights/test/run.html` via a local HTTP server (or file URL if WebAssembly permissions allow).
- The test harness loads `userData.db` and `userdata.zip`, verifies identical indexing, parses sample fixtures, and asserts correct token highlighting on Watchtower article 2025441 and Exodus chapters 5 & 6.

---

## License

Extension code is available under the MIT License. Embedded `sql-wasm.js` and `sql-wasm.wasm` are distributed under the MIT license by the [sql.js](https://github.com/sql-js/sql.js) project.
