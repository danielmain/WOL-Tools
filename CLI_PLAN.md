# `wol-cli` — Pure-Terminal OfflineWOL & Highlights Plan

> Goal: Build a **non-GUI, pure-terminal version** (`wol-cli/`) alongside `wol-desktop/` ("OfflineWOL") and `wol-highlights/`, sharing a core Rust library (`wol-core/`) so desktop, CLI/TUI, and future mobile targets share caching, sync, `.jwlibrary` import/export, and tokenization without duplicating logic.

> **Status: implemented** (all four phases). `wol-core/` holds the Tauri-free engine,
> `wol-cli/` ships the `wol` binary with `read`/`import`/`export`/`cache`/`sync`/`tui`/`info`,
> and CI publishes portable binaries for six targets. Read the "Implementation notes" below
> for the places where reality deliberately differs from this plan — the plan is kept as the
> original design record, not as a live task list.

## Implementation notes (what actually shipped, and why)

1. **`wol` with no subcommand launches the TUI** at `settings.home` → the configured
   language's home → a first-run language picker (saves `baseHost`/`language`/`region`/`home`
   exactly like the desktop settings UI). This is not in the plan above, but it is what makes
   the CLI feel like the desktop app.
2. **Home pages open a dashboard, not a wall of text.** WOL's own toolbar, welcome panel and
   daily-text widget are parsed out of the page HTML (`wol_core::dom::parse_chrome` /
   `parse_welcome` / `parse_daily_text`), so the CLI shows the same `BIBLIA · PUBLICACIONES ·
   REUNIONES · search · gear` buttons, in the page's language, as the desktop app. The toolbar
   is driven by the arrow keys + `Enter` (`↓`/`Tab` reads the daily text, `Esc`/`Tab` returns,
   `[`/`]` walk previous/next day) and the daily text is the normal reader, so highlights and
   notes work there too. Dated home pages (`/h/…/2026/11/15`) show *that* day; only the day
   matching the reader's local date is shown out of the three sections WOL always renders.
   Below 90 columns the welcome panel is dropped so an 80-column terminal still reads well.
3. **Directory pages got a two-pane browser.** WOL's library/meetings/Bible-version pages are
   `#materialNav` grids of cards; `dom::parse_nav` extracts the heading-delimited sections and
   their cards, and `tui/browser.rs` renders sub-categories on the left with the selected
   section's cards on the right (single-section pages show the selected card's detail instead
   of mirroring the menu). `j/k` move, `Enter` opens, `Esc`/`Backspace` back, and the cursor is
   remembered per URL. The window header names the site section (`BIBLIA · Biblias en
   español`) by matching the URL against WOL's own toolbar, so no per-language strings live in
   the CLI.
4. **Link browsing is a first-class mode** (`Tab`): `j`/`k` (or arrows) walk the page's links
   with wrap-around; the selection is auto-scrolled into view and marked `▸`. `Enter` opens it,
   `Backspace`/`H` go back, `gh` = home, `Ctrl-l` = language picker, `r` = re-fetch.
5. **Navigation pages needed parser work**, not just a renderer: WOL library/year pages are
   pretty-printed `<a>` cards, so `wol_core::dom` now (a) seeds a block's style from the block
   element itself so anchor-rooted blocks keep their `href`, (b) collapses whitespace inside
   text nodes and trims block tails, and (c) `render::article::is_nav_link` renders
   identifier-less all-link blocks as a bulleted link list. Whitespace handling cannot shift
   token indices (whitespace never forms a token).
6. **The tokenizer port needed three fixes to match `lib/tokenize.js` byte-for-byte**: general
   categories (`\p{L}`/`\p{N}`, so combining marks like Hebrew niqqud stay separate tokens),
   one token per UTF-16 code unit for astral characters (surrogate halves never match `WORD`),
   and U+0085 is *not* whitespace in JS. All three are covered by
   `wol-core/tests/tokenize_parity.rs` + fixtures generated from the JS.
7. **`n` edits a note; search hits cycle with `N` / `Ctrl-n`**, and `p` opens the colour legend
   popup (the plan had `n` doing double duty).
8. **Sidebar tabs are Cached · Contents · Highlights** (no separate "publications" tab).
9. **Images render as styled captions.** The optional `ratatui-image` (Kitty/Sixel/iTerm2)
   backend described above is not implemented.
10. **Cross-compatibility is a JS test**, not just a manual check:
   `wol-highlights/test/cli-export.test.mjs` exports from the CLI and re-imports through
   `wol-highlights/lib/backup.js`.

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
- **Home dashboard**: WOL's own toolbar (`BIBLIA · PUBLICACIONES · REUNIONES · search · gear`), the localised welcome/announcement panel and today's daily text in two panes. Arrows select a button, `Enter` opens it, `↓`/`Tab` focuses the daily text, `[`/`]` change day, `Ctrl-l` switches language.
- **Directory browser**: pages with `#materialNav` (library, meetings, Bible versions, publication years) render sub-categories on the left and the selected section's cards on the right; `j/k` move, `Enter` opens, `Esc`/`Backspace` back, and the cursor is remembered per URL.
- **Navigation**: nvim-style cursor row (`↑`/`↓` or `j`/`k` move it, viewport scrolls only when
  the cursor leaves it; `Ctrl-d`/`Ctrl-u`, `PageUp`/`PageDown`, `g`/`G`/`Home`/`End`), `←`/`→`
  (or `h`/`l`) move the token cursor, link following (`Enter` / `Tab`), history (`H`/`L`), and
  `/` in-page search. The status line shows the cursor row, token index and token text.
- **Interactive Highlighting**: Visual selection mode (`v` to mark start/end tokens, `1`–`6` to
  apply color, `0`/`x` to remove) — writes directly to `wolMarks` using exact token coordinates.
- **Note Editing**: Press `n` on the active paragraph (`data-pid`) to edit/save a note in a popup
  modal. `.gen-field` answer boxes are rendered read-only; they are edited in the extension or
  desktop app.
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
