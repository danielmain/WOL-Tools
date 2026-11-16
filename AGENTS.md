# AGENTS.md

> Unified technical and operational reference for AI agents and developers working in this repository.
> For the UserMark SQLite schema, backup format v16, and tokenization specification, see `usermark.md`.
> For the Tauri migration checklist and architecture notes, see `TODO.md`.

---

## 1. What This Repository Is

This repository bridges **JW Library** personal study data with **Watchtower Online Library ([wol.jw.org](https://wol.jw.org))**, and ships three components over two shared engines:

1. **Browser Extension (`wol-highlights/`)** — A shipped MV3 WebExtension (Firefox / Chrome / Edge / Brave / Safari) that parses `.jwlibrary` / `userData.db` / JSON backups client-side and paints/creates/edits highlights and study notes on `wol.jw.org`.
2. **Desktop App (`wol-desktop/`, "OfflineWOL")** — A Rust + Tauri 2 desktop app that opens `wol.jw.org` in a native webview via a custom `wol://` protocol, caches visited pages automatically for offline reading, and integrates the extension's highlight, note, and dark-mode logic.
3. **Terminal CLI (`wol-cli/`, binary `wol`)** — A pure-terminal reader (clap + ratatui) sharing `wol-core` with the desktop app: same cache directory, same `wolMarks`, same `.jwlibrary` import/export and sync engine, no webview.

**Key concept**: `.jwlibrary` backups do **not** contain publication text. They contain `userData.db` (SQLite) rows of *(location, word-token range, color 1–6)*. The extension (JS) and `wol-core` (Rust) tokenize the text with **identical rules** and paint the matching ranges; `wol-core/tests/tokenize_parity.rs` enforces that parity against `lib/tokenize.js`.

---

## 2. Current Status

| Area | State |
|---|---|
| Browser extension (`wol-highlights/`) | ✅ Shipped MV3 extension (~2.5k lines of JS) |
| Desktop app (`wol-desktop/`) | ✅ Core Tauri app implemented (§1–§5 in `TODO.md`, 4 integration test suites) |
| Shared engine (`wol-core/`) | ✅ Tauri-free; 68 unit tests + tokenizer parity suite |
| Terminal CLI (`wol-cli/`) | ✅ 57 unit tests; `read`/`import`/`export`/`cache`/`sync`/`tui`/`info`, `wol` alone opens the TUI at the language home and browses links (see `CLI_PLAN.md`) |
| Packaging & store scripts (`scripts/`, `safari/`) | ✅ Working (`npm run build`, `npm run lint`, `npm run publish`) |

---

## 3. Repository Layout

```text
.
├── .github/workflows/       # GitHub Actions (release.yml: OfflineWOL + `wol` CLI builds, all targets)
├── AGENTS.md                # Unified AI agent & developer reference (this file)
├── README.md                # User-facing documentation
├── TODO.md                  # Tauri desktop migration plan & verified WOL research
├── CLI_PLAN.md              # Terminal CLI / `wol-core` plan (renderer & TUI spec)
├── Cargo.toml               # Cargo workspace root (members: wol-core, wol-desktop, wol-cli)
├── usermark.md              # Authoritative spec: UserMark schema, tokens, WOL DOM mapping
├── flake.nix                # Nix devShell, `wol` package/app, AMO deployment app
├── package.json             # Extension, desktop & CLI scripts (build, lint, cli, desktop:*)
├── scripts/                 # Version bump & multi-store publishing (Firefox, Chrome, Edge)
├── safari/                  # Native Safari Web Extension Xcode project (macOS & iOS)
├── wol-highlights/          # MV3 Browser Extension source
│   ├── manifest.json        # Extension manifest (load this in browser dev tools)
│   ├── background.js        # MV3 service worker / background script
│   ├── popup.html/.css/.js  # Popup & full-tab backup manager
│   ├── content.css/.js      # Highlight styles, selection toolbar, textarea sync
│   ├── lib/
│   │   ├── backup.js        # Stream-based zip unpacker (DecompressionStream) & SQLite indexer
│   │   ├── highlight.js     # DOM TreeWalker & text node split/wrap engine
│   │   ├── languages.js     # MEPS language ID to symbol mapper
│   │   ├── tokenize.js      # Unicode word & punctuation tokenization engine
│   │   ├── sql-wasm.js      # sql.js Emscripten loader
│   │   └── sql-wasm.wasm    # sql.js WebAssembly binary
│   └── test/run.html        # In-browser parser & DOM highlight test runner
├── wol-core/                # GUI-agnostic engine (no Tauri, no terminal deps)
│   └── src/
│       ├── cache.rs         # SQLite metadata + atomic disk blob cache (LRU eviction)
│       ├── handler.rs       # Fetch, cache, stale-while-revalidate (http_types abstraction)
│       ├── crawler.rs       # Background prefetch / crawl helpers
│       ├── url_map.rs       # Canonical URL <-> wol:// mapping (uses the `url` crate)
│       ├── rewrite.rs       # HTML URL rewriting (wol.jw.org & b.jw-cdn.org -> wol://)
│       ├── state.rs         # Persistent settings, wolMarks, cookies
│       ├── sync.rs          # Sync engine (SyncListener callback, no AppHandle)
│       ├── publications.rs  # JWPUB publication store
│       ├── jwlibrary_export.rs # wolMarks -> .jwlibrary (schema v16) writer
│       ├── backup.rs        # .jwlibrary/userData.db/JSON -> wolMarks importer
│       ├── tokenize.rs      # Rust port of lib/tokenize.js (UTF-16 token parity)
│       ├── dom.rs           # #article parser -> blocks/tokens (ignore rules of highlight.js)
│       ├── highlight.rs     # Token-range add/remove helpers shared by CLI & TUI
│       ├── http_types.rs    # Response/Method/StatusCode/Headers abstraction
│       ├── dirs.rs          # Shared app data directory resolution
│       └── languages.rs     # Language table (languages.json)
├── wol-cli/                 # Terminal CLI + TUI (binary: `wol`)
│   └── src/
│       ├── main.rs          # clap commands & global flags
│       ├── context.rs       # Cache-first loading, canonicalization, --offline
│       ├── commands.rs      # read/import/export/cache/sync/info
│       ├── render/          # Theme (TrueColor/256/none), article, tables, legend
│       └── tui/             # ratatui event loop, reader, sidebar, notes, language picker
└── wol-desktop/             # Rust + Tauri 2 Desktop App ("OfflineWOL")
    ├── Cargo.toml           # Depends on wol-core + tauri 2
    ├── tauri.conf.json      # Tauri app config (productName: OfflineWOL)
    ├── src/
    │   ├── lib.rs           # App setup, custom wol:// protocol, menu, windows
    │   ├── bridge.rs        # window.__wolBridge injection & theme/cookie/marks sync
    │   ├── commands.rs      # Tauri IPC commands for manager UI
    │   └── main.rs
    ├── tests/               # Acceptance & integration tests (cargo test)
    └── ui/                  # Manager & setup HTML/CSS/JS + reused wol-highlights/lib/*
```

---

## 4. Desktop App Architecture & Decisions (`wol-desktop/`)

> **Verified fact**: WOL article pages are **server-rendered** (~63 `<p data-pid="…">` elements in raw HTML, 3 CSS + 3 JS from `/assets/`, and only `b.jw-cdn.org` for media player). A cached article page is self-contained and renders offline.

Key architectural decisions (do not reopen without cause):
1. **Interception via custom protocol `wol://`**, not a MITM proxy. `register_uri_scheme_protocol` only intercepts your own scheme; the Rust handler fetches upstream, stores, serves, and rewrites URLs in HTML so subresources also traverse `wol://`.
2. **Cache policy = stale-while-revalidate.** Disk hits serve immediately; revalidation runs in the background using `ETag` / `Last-Modified` (CloudFront provides both).
3. **SQLite for cache metadata, blobs on disk.** Never store page bodies in SQLite. Use atomic writes (`tmp` → `rename`).
4. **LRU eviction with a size cap** (default 2 GB) and a built-in Cache Inspector UI.
5. **`wolMarks` JSON schema stays byte-identical** to the extension's so exported highlights import unchanged across both.
6. **No `.jwpub` parity in M1.** Cache what is visited; UI copy says "OfflineWOL" / offline reader, not a JW Library clone.
7. **Reuse the JS, shim the APIs.** `tokenize.js`, `highlight.js`, `backup.js`, `sql-wasm.*`, and `content.css` are reused; browser extension `storage`/`tabs`/`runtime` calls are shimmed via `window.__wolBridge`.

### Desktop Gotchas
- **Cache keys must include language and region.** `/es/wol/d/r4/lp-s/X` and `/en/wol/d/r1/lp-e/X` are different documents. Canonicalize as `https://wol.jw.org<path>?<sorted-query>` — never key on `wol://`.
- **Do not guess JW CDN paths.** `app.jw-cdn.org/pub-media/...` returns **403** to direct requests. Use the endpoints the page calls (`GETPUBMEDIALINKS`, `GETPUBMEDIA` on `b.jw-cdn.org/apis/...`).
- **`wol-highlights/lib/languages.js` only maps `{E: 0, S: 1}`** (MEPS IDs). Non-EN/ES Bible-chapter matching requires the full language table (`wol-core/src/languages.json`).
- **Bound background work.** A page references ~220 subresources; keep concurrency bounded and deduplicate in-flight URLs.
- **Never cache non-2xx or `Set-Cookie` responses.** Serve stored `Content-Type`, don't guess from file extensions.
- The extension's `!important` CSS rules are fine inside a Tauri `WebviewWindow`.

### Core / CLI split (`wol-core/`, `wol-cli/`)

- **`wol-core` must stay GUI- and terminal-agnostic.** No Tauri, no clap, no ratatui. Tauri
  coupling was removed deliberately: `url_map` uses `url::Url`, `handler` speaks
  `http_types::{Response, Method, StatusCode, Headers}`, `sync` reports progress through a
  `SyncListener` callback and is driven by `SyncManager::new_shared()`. Adding a Tauri type
  back into `wol-core` re-breaks mobile/UniFFI plans.
- **One data directory, three front-ends.** `wol-core::dirs::app_data_dir()` resolves the
  shared app data dir (`~/Library/Application Support/com.offlinewol.desktop`, identical on
  every OS today, legacy id migrated), `WOL_DATA_DIR` overrides it, and the CLI adds
  `--data-dir` for scratch copies. Never write state anywhere else, or the desktop app and CLI
  stop seeing each other's marks.
- **`wol-cli` rendering is pre-wrapped ANSI text.** `render::article` emits styled, wrapped
  lines; the TUI converts those SGR sequences into ratatui styles (`ansi_to_tui_line`), which
  is what makes highlights visible in the TUI. Don't reintroduce a strip-ANSI pass.
- **TUI edits are real edits.** `1`-`6`, `0`/`x` and the note editor write straight through
  `State::set_stored_index`, so the desktop app and extension see them immediately.
- **`wol` with no subcommand opens the TUI, desktop-style.** Startup target is
  `settings.home` → the configured language's home (`wol_core::languages::home_url`) → the
  language picker on a fresh install. The picker saves `baseHost`/`language`/`region`/`home`
  exactly like `wol-desktop/ui/manager.js`, so both front-ends agree.
- **WOL navigation pages are cards/anchors, not study paragraphs.** `dom` seeds a block's
  style from the block element itself (so `<a>` roots keep their `href`), collapses
  whitespace in text nodes, and trims block tails; `render::article::is_nav_link` turns
  identifier-less all-link blocks into a bulleted link list. `Tab` in the TUI browses those
  links (`Reader::step_link`/`nearest_link`). Do not "simplify" the whitespace handling —
  pretty-printed card markup otherwise renders blank lines and loses hrefs.
- **`--no-color` / `NO_COLOR` / piped output** fall back to `⟦…⟧` highlight markers; keep that
  path working (it is what the tests assert on).

---

## 5. Browser Extension Rules & DOM Details (`wol-highlights/`)

### MV3 & Firefox Constraints
- In Firefox (MV3), content scripts **cannot** use ES module syntax (`import`/`export`).
- `manifest.json` loads content scripts sequentially as standard scripts:
  1. `lib/tokenize.js`
  2. `lib/languages.js`
  3. `lib/highlight.js`
  4. `content.js`
  **Do not add `import` or `export` statements to these files.**
- `manifest.json` requires `"content_security_policy": { "extension_pages": "script-src 'self' 'wasm-unsafe-eval'; object-src 'self'" }` for `sql.js` WebAssembly execution.

### Tokenization & WOL DOM (`lib/tokenize.js`, `lib/highlight.js`, `content.js`)
- **Tokens**: `WORD = /[\p{L}\p{N}\p{Pc}]/u` sequences form one token; any other non-whitespace char is an individual token; whitespace (`SPACE`) is skipped; soft hyphens (`\u00AD`) and zero-width spaces count as individual tokens.
- **DOM targets** (inside `#article`):
  - Articles: `p[data-pid="<Identifier>"]` (`data-pid="1"` is title; study paragraphs start at `2`).
  - Bible verses: `span.v[id^="v<book>-<chapter>-<verse>-"]` (may span multiple fragments).
- **Ignored elements** in `TreeWalker`: `.parNum`, `.pageNum`, `.fn`, `a.vl`, `a.cl`, `rt`, `rp`, `mark.wol-hl`, and `a.b` whose text is strictly `"+"`.
- **Wrapping**: Slices are sorted in **descending document order**, split via `Text.splitText()`, and wrapped in `<mark class="wol-hl wol-c<1-6>" data-start-token data-end-token data-color>`. Clearing unwraps marks and calls `parent.normalize()`.
- **WOL Audio Player Isolation**: `content.js` intercepts `mousedown`, `pointerdown`, and `click` in the **capture phase** (`capture: true`) so WOL's `#playSelectedButton` / `.jwac-textHighlight` doesn't cancel text selection or marker clicks.
- **Textarea Note Sync**: `content.js` unlocks `disabled` `<textarea>` elements in `.gen-field`, maps them to `data-pid` (including `data-rel-pid`), populates from backup `Note` / `InputField` data, and saves edits back to `wolMarks`.
- **Reactive Page Monitoring**: `MutationObserver` on `document.documentElement` debounces `schedule()` (200ms) and sets `document.documentElement.dataset.wolHighlights = "empty" | "on" | "off" | "error:..."`.

### `StoredIndex` Shape (`wolMarks`)
```typescript
interface StoredIndex {
  version: 1;
  highlightCount: number;
  noteCount?: number;
  ignored: number;
  documentCount: number;
  bibleChapterCount: number;
  colorCounts: Record<number, number>;
  fileName?: string;
  loadedAt?: string;
  enabled?: boolean;
  docs: Record<string, Record<string, [startToken: number, endToken: number, colorIndex: number][]>>;
  bible: Record<string, Record<string, [startToken: number, endToken: number, colorIndex: number][]>>;
  notes?: {
    docs: Record<string, Record<string, NoteItem[]>>;
    bible: Record<string, Record<string, NoteItem[]>>;
  };
  inputFields?: {
    docs: Record<string, Record<string, string>>;
    bible: Record<string, Record<string, string>>;
  };
}
```

---

## 6. Conventions & Verification

- **Functional-first, strongly typed Rust**: exhaustive enums, `Result`/`Option`, no `unwrap`/`expect` in library code, newtypes for domain values, no lossy `as`. Keep I/O at the edges so parsing/canonicalization is unit-testable.
- **Verify, don't assume**: check live WOL responses before assuming markup/headers, and document verified facts here or in `TODO.md`.
- **After editing shared JS in `wol-highlights/`**:
  - Run `npm run lint` and `npm run build` to verify the browser extension still lints and packages cleanly.
  - Check `wol-highlights/test/run.html` if tokenization or highlight placement changed.
- **After editing `wol-core/`, `wol-desktop/` or `wol-cli/`**:
  - Run `npm run desktop:test` (`cargo test --workspace`) — covers core, desktop and CLI.
  - Run `npm run cli:test` for a fast `wol-cli`-only loop.
- **After touching tokenization (`wol-core/src/tokenize.rs` or `wol-highlights/lib/tokenize.js`)**:
  - Run `npm run test:tokenize` to prove the Rust port still matches the shipped JS
    tokenizer on the fixture corpus; add cases to
    `wol-core/tests/fixtures/tokenize_cases.json` and refresh with
    `npm run test:tokenize:update`, then `cargo test -p wol-core --test tokenize_parity`.
- **After touching `.jwlibrary` export/import**:
  - Run `node --test wol-highlights/test/cli-export.test.mjs` (needs `cargo build -p wol-cli`
    first, or set `WOL_BIN`), which round-trips a CLI-exported backup through
    `wol-highlights/lib/backup.js`.
- **Git commits & pushes**:
  - When asked to commit and push, always stage and commit **all** modified and untracked files in the working tree (`git add -A`) and push to **both** remotes (`git push github master:main` and `git push origin master`).
