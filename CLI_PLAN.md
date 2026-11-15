# `wol-cli` — Pure-Terminal OfflineWOL & Highlights Plan

> Goal: Build a **non-GUI, pure-terminal version** (`wol-cli/`) alongside `wol-desktop/` ("OfflineWOL") and `wol-highlights/`, sharing a core Rust library (`wol-core/`) so desktop, CLI/TUI, and future mobile targets share caching, sync, `.jwlibrary` import/export, and tokenization without duplicating logic.

---

## 1. Architecture: Cargo Workspace Split

Currently `wol-desktop/` mixes GUI-agnostic logic with Tauri (`lib.rs`, `bridge.rs`, `commands.rs`). Convert the Rust side into a Cargo workspace at the repository root (or under a shared `Cargo.toml`):

```text
.
├── Cargo.toml                  # Workspace: wol-core, wol-desktop, wol-cli
├── wol-core/                   # Pure Rust, zero GUI/Tauri dependencies
│   ├── src/
│   │   ├── cache.rs            # Moved from wol-desktop/src/cache.rs (DiskCache, LRU)
│   │   ├── handler.rs          # Moved from wol-desktop/src/handler.rs (fetch, SWR, cache)
│   │   ├── url_map.rs          # Moved from wol-desktop/src/url_map.rs
│   │   ├── crawler.rs          # Moved from wol-desktop/src/crawler.rs
│   │   ├── state.rs            # Moved from wol-desktop/src/state.rs (wolMarks, settings, cookies)
│   │   ├── sync.rs             # Moved from wol-desktop/src/sync.rs
│   │   ├── publications.rs     # Moved from wol-desktop/src/publications.rs
│   │   ├── jwlibrary_export.rs # Moved from wol-desktop/src/jwlibrary_export.rs
│   │   ├── languages.rs        # Moved from wol-desktop/src/languages.rs
│   │   ├── tokenize.rs         # NEW: Rust port of wol-highlights/lib/tokenize.js
│   │   ├── backup.rs           # NEW: Rust port of wol-highlights/lib/backup.js (.jwlibrary -> wolMarks)
│   │   └── dom.rs              # NEW: HTML #article parser & token-aligned DOM extractor
├── wol-desktop/                # Tauri 2 GUI app (depends on wol-core)
│   └── src/{lib,bridge,commands,rewrite}.rs
└── wol-cli/                    # Pure-terminal CLI + interactive TUI (depends on wol-core)
    ├── Cargo.toml
    └── src/
        ├── main.rs             # clap subcommands + TTY detection
        ├── render/
        │   ├── mod.rs          # Theme, TrueColor/256-color/ASCII capability detection
        │   ├── article.rs      # HTML DOM -> styled terminal blocks & wrapped spans
        │   ├── table.rs        # UTF-8 box-drawing table layout
        │   └── highlight.rs    # Token-range -> ANSI color span mapper
        └── tui/
            ├── mod.rs          # ratatui + crossterm event loop
            ├── reader.rs       # Scrollable article/Bible view, visual highlight selection
            ├── sidebar.rs      # TOC / cached docs / search / footnotes drawer
            └── notes.rs        # Inline note viewer & editor (.gen-field / wolMarks)
```

### Why this layout?
- **Zero duplicated business logic**: `wol-cli` and `wol-desktop` share the exact same `DiskCache` directory (`com.offlinewol.desktop`), `wolMarks` JSON format, `.jwlibrary` exporter, and sync engine.
- **Mobile-ready**: `wol-core` has no Tauri or terminal dependencies, making it straightforward to wrap via UniFFI (Swift/Kotlin) or Tauri Mobile later.

---

## 2. Phase 1 — Extract `wol-core` & Port JS Tokenization/Import

1. **Workspace & Crate Extraction**
   - Move GUI-independent modules from `wol-desktop/src/` into `wol-core/src/`.
   - Keep `wol-desktop/src/lib.rs`, `bridge.rs`, `commands.rs`, and `rewrite.rs` in `wol-desktop`.
   - Verify `npm run desktop:test` passes with zero regressions.
2. **Port `tokenize.js` -> `wol-core/src/tokenize.rs`**
   - Match `wol-highlights/lib/tokenize.js` **byte-for-byte in behavior**:
     - `WORD`: Unicode `\p{L}|\p{N}|\p{Pc}` runs form a single token.
     - `SPACE`: Unicode whitespace (`\s`, `\u00A0`, `\u1680`, `\u2000-\u200A`, `\u2028`, `\u2029`, `\u202F`, `\u205F`, `\u3000`, `\uFEFF`) is skipped.
     - Any other character (punctuation, `\u00AD` soft hyphen, zero-width spaces) is an individual 1-char token.
   - Add unit tests against the same reference cases documented in `tokenize.js` (document `2025441` paragraph 7, `nwtsty` Exodus `5:9` and `6:6`).
3. **Port `backup.js` -> `wol-core/src/backup.rs`**
   - Unzip `.jwlibrary` / open `userData.db` using `zip` + `rusqlite` (already in `Cargo.toml`).
   - Build the exact `StoredIndex` (`wolMarks`) structure: `docs`, `bible`, `notes`, `inputFields`, `colorCounts`, `highlightCount`.
   - This removes the need for `sql.js` / WebAssembly in `wol-cli`.
4. **DOM Extractor -> `wol-core/src/dom.rs`**
   - Parse raw WOL HTML using `scraper` (`html5ever`).
   - Extract `#article` metadata (title, publication breadcrumb, prev/next links) and structured blocks (`h1`–`h4`, `p[data-pid]`, `span.v[id^="v..."]`, blockquotes, tables, `.gen-field` textareas, footnotes `.fn`).
   - Walk text nodes using the same ignore rules as `wol-highlights/lib/highlight.js` (`.parNum`, `.pageNum`, `.fn`, `a.vl`, `a.cl`, `rt`, `rp`, `a.b` with `"+"`) so token indices align 1:1 with `wolMarks`.

---

## 3. Phase 2 — Rich UTF-8 / ANSI HTML Renderer (`wol-cli/src/render/`)

Instead of generic `html2text`, render the structured DOM from `wol-core::dom` into styled terminal lines using UTF-8 box-drawing, Unicode typography, and ANSI colors (with graceful fallback when piped or on limited terminals).

### Visual Mapping Specification

| WOL HTML Element | Terminal Representation (UTF-8 + ANSI) |
|---|---|
| **Document Header** | Rounded frame (`╭─╮│╰─╯`) with title, publication/chapter breadcrumb, and cache status (`● cached` / `○ live`). |
| **Headings (`h1`–`h3`)** | Bold + accent color + horizontal rule (`━━━━━━` for `h1`, `──────` for `h2`). |
| **Paragraph / Verse Numbers** (`.parNum`, `a.vl`) | Dimmed Unicode superscripts (`¹`, `²`, `³`) or left-gutter column (` 12 │ ...`) to keep prose clean. |
| **Highlights (`colors 1–6`)** | **24-bit TrueColor** background (`\x1b[48;2;R;G;Bm`) + contrasting foreground matching JW Library's 6 colors; 256-color or colored-bracket (`⎸…⎹`) fallback when `COLORTERM` is absent. |
| **Scripture Quotes / Callouts** | Thick left bar (`┃ `) in muted accent color with `textwrap` indentation. |
| **Footnotes & Cross-refs** (`.fn`, `a.cl`, `+`) | Compact inline markers (`[†1]`, `[*]`) that expand in a bottom drawer/footer rather than cluttering paragraphs. |
| **Study Notes / `.gen-field`** | Rounded note box (`╭─ ✎ Note ──╮ ... ╰───────────╯`) rendered directly under the matching `data-pid` paragraph. |
| **Tables** | Unicode grid (`┌─┬─┐`, `├─┼─┤`, `└─┴─┘`) with terminal-width-aware column wrapping. |
| **Images / Media** | Styled caption box (`[ 🖼 Figure: ... ]`), with optional inline image rendering (`ratatui-image` for Kitty/Sixel/iTerm2) behind a feature flag. |
| **Plain / `--ascii` mode** | Clean ASCII-only borders (`+-+`, `|`, `---`) and `--no-color` support respecting `NO_COLOR`. |

---

## 4. Phase 3 — CLI Commands & Interactive TUI (`wol-cli`)

### A. One-Shot CLI Subcommands (`clap`)
- `wol-cli read <url-or-ref>` — Fetch (or load from `DiskCache`), apply `wolMarks` highlights/notes, and print formatted output (auto-paging through `less -R` if stdout is a TTY, or raw stdout when piped).
- `wol-cli import <file.jwlibrary|userData.db|marks.json>` — Import highlights/notes into `StateManager`.
- `wol-cli export <out.jwlibrary|marks.json>` — Export via `jwlibrary_export` or `wolMarks` JSON.
- `wol-cli cache status | crawl <url> | clear | prune` — Manage offline cache from the terminal.
- `wol-cli sync status | pull | push` — Run `SyncManager` headlessly.
- `wol-cli tui [url]` — Launch the full-screen interactive reader.

### B. Interactive TUI (`ratatui` + `crossterm`)
- **Navigation**: Vim-style scrolling (`j`/`k`, `Ctrl-d`/`Ctrl-u`, `g`/`G`), link following (`Enter` / `Tab`), history (`H`/`L`), and `/` in-page search.
- **Interactive Highlighting**: Visual selection mode (`v` to mark start/end tokens, `1`–`6` to apply color, `0`/`x` to remove) — writes directly to `wolMarks` using exact token coordinates.
- **Note Editing**: Press `n` on the active paragraph (`data-pid`) to edit/save a note in a popup modal (or `$EDITOR`).
- **Panels**: Toggleable sidebar (`b`) for cached documents / TOC / publications, and a bottom drawer (`f`) for footnotes & cross-references.

---

## 5. Phase 4 — Verification & Packaging

- **Unit & Snapshot Tests**:
  - Tokenization parity tests comparing `wol-core::tokenize` against `wol-highlights/lib/tokenize.js` outputs.
  - Golden-file ANSI/UTF-8 render tests for sample WOL articles and Bible chapters at 80-col and 120-col widths.
- **Cross-Compatibility**:
  - Verify `.jwlibrary` imported in `wol-cli` renders identically in `wol-desktop` and the browser extension (`wol-highlights`), and vice versa.
- **Distribution**:
  - Add `wol-cli` build targets to `.github/workflows/release.yml` (macOS, Linux, Windows static/portable binaries) and `flake.nix`.
