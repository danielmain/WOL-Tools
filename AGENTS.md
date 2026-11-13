# AGENTS.md

> Unified technical and operational reference for AI agents and developers working in this repository.
> For the UserMark SQLite schema, backup format v16, and tokenization specification, see `usermark.md`.
> For the Tauri migration checklist and architecture notes, see `TODO.md`.

---

## 1. What This Repository Is

This repository bridges **JW Library** personal study data with **Watchtower Online Library ([wol.jw.org](https://wol.jw.org))**, and now ships two components that share the same core highlight/backup JS engine:

1. **Browser Extension (`wol-highlights/`)** — A shipped MV3 WebExtension (Firefox / Chrome / Edge / Brave / Safari) that parses `.jwlibrary` / `userData.db` / JSON backups client-side and paints/creates/edits highlights and study notes on `wol.jw.org`.
2. **Desktop App (`wol-desktop/`, "OfflineWOL")** — A Rust + Tauri 2 desktop app that opens `wol.jw.org` in a native webview via a custom `wol://` protocol, caches visited pages automatically for offline reading, and integrates the extension's highlight, note, and dark-mode logic.

**Key concept**: `.jwlibrary` backups do **not** contain publication text. They contain `userData.db` (SQLite) rows of *(location, word-token range, color 1–6)*. Both the extension and desktop app tokenize the live WOL DOM with identical rules and paint the matching ranges.

---

## 2. Current Status

| Area | State |
|---|---|
| Browser extension (`wol-highlights/`) | ✅ Shipped MV3 extension (~2.5k lines of JS) |
| Desktop app (`wol-desktop/`) | ✅ Core Tauri app implemented (§1–§5 in `TODO.md`, 4 integration test suites) |
| Packaging & store scripts (`scripts/`, `safari/`) | ✅ Working (`npm run build`, `npm run lint`, `npm run publish`) |

---

## 3. Repository Layout

```text
.
├── .github/workflows/       # GitHub Actions (release.yml: OfflineWOL builds for macOS/Linux/Windows x86_64 & aarch64)
├── AGENTS.md                # Unified AI agent & developer reference (this file)
├── README.md                # User-facing documentation
├── TODO.md                  # Tauri desktop migration plan & verified WOL research
├── usermark.md              # Authoritative spec: UserMark schema, tokens, WOL DOM mapping
├── flake.nix                # Nix devShell & AMO deployment app
├── package.json             # Extension & desktop scripts (build, lint, start, desktop:*)
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
└── wol-desktop/             # Rust + Tauri 2 Desktop App ("OfflineWOL")
    ├── Cargo.toml           # Rust dependencies (tauri 2, tokio, reqwest, rusqlite, etc.)
    ├── tauri.conf.json      # Tauri app config (productName: OfflineWOL)
    ├── src/
    │   ├── lib.rs           # App setup, custom wol:// protocol, menu, windows
    │   ├── handler.rs       # Custom protocol handler (fetch, cache, revalidate)
    │   ├── rewrite.rs       # HTML URL rewriting (wol.jw.org & b.jw-cdn.org -> wol://)
    │   ├── url_map.rs       # Canonical URL <-> wol:// mapping & cache key normalization
    │   ├── cache.rs         # SQLite metadata + atomic disk blob cache (LRU eviction)
    │   ├── crawler.rs       # Background prefetch / crawl helpers
    │   ├── bridge.rs        # window.__wolBridge injection & theme/cookie/marks sync
    │   ├── commands.rs      # Tauri IPC commands for manager UI
    │   ├── state.rs         # Persistent app settings, marks, cookies
    │   └── languages.rs     # Language table (languages.json)
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
- **`wol-highlights/lib/languages.js` only maps `{E: 0, S: 1}`** (MEPS IDs). Non-EN/ES Bible-chapter matching requires the full language table (`wol-desktop/src/languages.json`).
- **Bound background work.** A page references ~220 subresources; keep concurrency bounded and deduplicate in-flight URLs.
- **Never cache non-2xx or `Set-Cookie` responses.** Serve stored `Content-Type`, don't guess from file extensions.
- The extension's `!important` CSS rules are fine inside a Tauri `WebviewWindow`.

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
- **After editing `wol-desktop/`**:
  - Run `npm run desktop:test` (`cargo test --manifest-path wol-desktop/Cargo.toml`).
- **Git commits & pushes**:
  - When asked to commit and push, always stage and commit **all** modified and untracked files in the working tree (`git add -A`) and push to **both** remotes (`git push github master:main` and `git push origin master`).
