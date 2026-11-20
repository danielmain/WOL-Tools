# TODO — WOL Desktop (Tauri) migration

> **Goal**: A Rust + Tauri desktop app that opens `wol.jw.org` in a native webview,
> integrates the existing WOL Highlights extension logic, and caches content **as you
> browse** so previously-visited pages work offline — no download button.

**Status**: Core Tauri migration completed (§1 spike, §2 cache, §3 crawler, §4 bridge & manager window, §5 settings & cache inspector). The Rust side is now a Cargo workspace — `wol-core/` (GUI-agnostic engine, 76 unit tests + tokenizer parity), `wol-desktop/` (Tauri shell, 5 unit + 12 integration tests), `wol-cli/` (`wol` terminal reader with home dashboard and directory browser, see `CLI_PLAN.md`) — so file paths mentioned in the task lists below may now live under `wol-core/src/` instead of `wol-desktop/src/`.
**Origin**: this repo (`wol-highlights`) — a working MV3 browser extension. The Tauri app
is a **new sibling crate in this same repo**, reusing `wol-highlights/lib/*` and
`wol-highlights/content.js` + `content.css` largely unmodified.

---

## 0. What was already researched (do not re-derive)

### WOL is server-rendered
Articles contain ~63 `<p data-pid="…">` elements in the raw HTML response. No client-side
rendering of article text. **Therefore a cached article page is self-contained and renders
offline.** This is the fact the whole design rests on.

### Subresources referenced by a real article
Fetched `/es/wol/d/r4/lp-s/1102026210` (78,951 bytes). Outbound references:

| Count | Target | Notes |
|---|---|---|
| 220 | root-relative `/assets/...` | CSS + JS, same origin as `wol.jw.org` |
| 5 | `https://www.jw.org` | footer/nav links only, not needed to render |
| 2 | `https://b.jw-cdn.org` | media-player CSS + JS |

Only 3 CSS + 3 JS files from `/assets/` on an article page. The only *third-party* origin
needed for rendering is `b.jw-cdn.org` (media player).

### Cache headers from CloudFront
```
cache-control: max-age=21600, s-maxage=21600
etag: W/"…"
last-modified: …
edge-control: max-age=259200, downstream-ttl=21600
strict-transport-security: max-age=31536000
```
So: usable `ETag` + `Last-Modified` for conditional revalidation. No header guessing needed.

### URL shape (matters for cache keys)
```
https://wol.jw.org/<lang>/wol/d/r<region>/lp-<symbol>/<docId>       article
https://wol.jw.org/<lang>/wol/b/r<region>/lp-<symbol>/<book>/<chap>  bible chapter
https://wol.jw.org/<lang>/wol/h/r<region>/lp-<symbol>               home / language root
```
`lp-<symbol>` is the MEPS language symbol, `r<region>` the region. **These are part of the
cache key** — `/es/wol/d/r4/lp-s/X` and `/en/wol/d/r1/lp-e/X` are different documents.

### The real third-party dependency: `b.jw-cdn.org`
Endpoints seen in article HTML:
- `GETPUBMEDIALINKS` — audio/video URLs for a publication
- `GETPUBMEDIA` — publication metadata
- `/code/media-player/v3.3.0/{css,js}` — player assets

Note: direct URL guessing on `app.jw-cdn.org/pub-media/...` returns **403** (signed or
referrer-gated). Do not build a downloader on guessed CDN paths; go through the API
endpoints the page itself calls.

### Known parity gap vs JW Library
WOL offline ≈ **articles** offline (study editions, daily text, workbooks, Bible chapters).
JW Library additionally offers the publication library as structured `.jwpub` packages
(zip → SQLite `contents` DB → HTML/MP3 payloads) with multi-bitrate media.

**Decision: do NOT attempt `.jwpub` parity in M1.** Cache what is visited. Leave a seam
(see `CacheStore` trait in §3) for a future importer. Be honest in UI copy: this is
"WOL, offline", not a JW Library clone.

---

## 1. Go/no-go spike (do this before any UI work)

**Success criterion**: with the network physically disabled, a previously visited article
renders correctly from cache, including CSS layout and your highlights.

| # | Task | Verify |
|---|---|---|
| 1.1 | `cargo init` a `wol-desktop/` crate with Tauri 2 + `serde`, `reqwest`, `tokio` | `cargo build` succeeds |
| 1.2 | Register custom protocol `wol://` via `register_uri_scheme_protocol` | Navigate window to `wol://es/wol/h/r4/lp-s`; page appears |
| 1.3 | In handler: fetch upstream `https://wol.jw.org/<path>`, return bytes + content-type | Same, but content came from Rust |
| 1.4 | Rewrite HTML: root-relative `"/…"` → `"wol://<current-host-corresponding>/…"`, and `https://b.jw-cdn.org/…` → routed too. Simplest correct approach: rewrite only `/assets/` and `/icon/` first, then widen. | CSS loads: page is styled, not raw HTML |
| 1.5 | Subresource handler path: same fetch-and-serve for JS/CSS/images | DevTools network shows all 200s from `wol://` |
| 1.6 | **Kill network** (airplane mode / `sudo pfctl -e; pfctl -f /etc/pf.conf` block, or physically disable Wi-Fi) and reload | **GO/NO-GO**: article renders, highlights paint |
| 1.7 | Only if 1.6 passes: proceed to §2 | — |

If 1.6 fails, diagnose before continuing: likely a subresource still pointing at
`https://` (inspect the network panel offline), or `Content-Type` mismatch causing CSS to
be rejected. Do not proceed to caching until rendering is correct.

> **Note on UA**: WOL served a normal layout to a desktop Safari UA. If a `WebKit`/`WKWebView`
> default UA ever gets a stripped page, set `user_agent` in `tauri.conf.json`. Not observed
> as a problem yet.

---

## 2. Cache layer

Design: **stale-while-revalidate**, so a hit is *always* instant and freshness is a
background concern.

```
request(url)
  ├─ disk hit  → serve bytes immediately; spawn background revalidate (If-None-Match /
  │              If-Modified-Since); on 200 replace entry, on 304 touch `lastChecked`
  └─ disk miss → fetch → write → serve. On network error: 404-style miss page, never hang.
```

### 2.1 Storage layout
```
$APP_DATA/
├── state.json                 # settings + wolMarks (see §4)
└── cache/
    ├── index.json             # or sqlite — see decision below
    └── blobs/<sha256-of-canonical-url>
```

**Recommend SQLite** over a JSON index (add `rusqlite`), because the cache inspector needs
`SUM(size)`, `ORDER BY lastAccessed`, and eviction queries. Schema:

```sql
CREATE TABLE entries (
  url          TEXT PRIMARY KEY,  -- canonical URL, includes lang+region
  path         TEXT NOT NULL,     -- blob filename
  etag         TEXT,
  last_modified TEXT,
  content_type TEXT NOT NULL,
  size         INTEGER NOT NULL,
  fetched_at   INTEGER NOT NULL,
  checked_at   INTEGER NOT NULL,  -- last successful revalidation/touch
  last_used_at INTEGER NOT NULL   -- for LRU eviction
);
```

Blobs on disk, metadata in SQLite. **Never store large blobs in the DB.**

### 2.2 Rules that prevent subtle bugs
- **Canonicalize the cache key**: normalize to `https://wol.jw.org<path>?<sorted query>`;
  do not cache by the `wol://` URL. Strip nothing else — `lp-`/`r` are significant.
- **Never cache non-2xx.** Never cache `Set-Cookie` responses.
- **Serve `Content-Type` from the stored metadata**, not guessed by extension.
- **Total size cap** (default 2 GB, configurable) with LRU eviction on write when exceeded.
- **Bounded background revalidation**: one in-flight revalidation per URL, and a small
  concurrency semaphore (e.g. 4). A page with 200 subresources must not open 200 sockets.
- **Atomic writes**: write to `*.tmp` then `rename`. A crashed write must not corrupt entries.

### 2.3 Acceptance tests for this layer
| Test | Expected |
|---|---|
| Load article online, disable network, reload | Renders fully |
| Load article, re-enable network, reload | Served from disk *immediately*, revalidated in background |
| Modify `etag` in DB to force a 200 | Blob replaced, page still correct |
| Request an uncached URL offline | Clean "not available offline" page, no hang, app usable |
| Fill cache past cap | Oldest `last_used_at` evicted; size under cap |

---

## 3. Crawler (optional, but cheap — it reuses §2)

The crawler is **not a second system**. It is a URL list fed into the same fetch-and-store
path used by navigation.

| # | Task |
|---|---|
| 3.1 | Define `CacheStore` trait so navigation and crawler share one write path (also the seam for a future `.jwpub` importer) |
| 3.2 | "Save this page + its subresources" — enqueue current URL, parse HTML for `/assets/`, `b.jw-cdn.org`, images; fetch with the same bounded concurrency |
| 3.3 | "Save this issue" — parse the issue's table-of-contents links, enqueue each article via 3.2 |
| 3.4 | Progress UI + cancel. Crawl must be resumable/interruptible, and must not hold a lock that blocks navigation |
| 3.5 | Politeness: ≤4 concurrent, small delay between requests, respect `403` by stopping that subtree |

Ship 3.2 first; it is 80% of user value for the "I'm boarding a plane" case.

---

## 4. Translation-extension integration

Mapping from the browser extension's APIs to Tauri. **Reuse the JS files; replace only the
`storage` / `tabs` / `runtime` calls.**

| Extension | Where | Tauri replacement |
|---|---|---|
| `ext.storage.local.get/set("wolMarks")` | `content.js` ×6, `popup.js` ×5 | `invoke("get_marks")` / `invoke("set_marks")` |
| `ext.runtime.onMessage("wol-reapply")` | `content.js:364` | `window.__emit` from Rust, or `eval` re-injection |
| `ext.tabs.query` + `sendMessage` | `popup.js:125,144` | Tauri `WebviewWindow` list; push a global event instead |
| `action.onClicked` → open manager | `background.js` | Native menu item / tray → new `WebviewWindow` |
| `<input type=file>` for `.jwlibrary` | `popup.html:19` | `tauri-plugin-dialog` (no WASM CSP round-trip) |
| `content_scripts` injection | `manifest.json` | `initialization_script()` + `on_page_load` |

### Files that move unchanged
- `wol-highlights/lib/tokenize.js` (32 lines) — pure
- `wol-highlights/lib/highlight.js` (278) — pure DOM
- `wol-highlights/lib/backup.js` (327) — sql.js, pure
- `wol-highlights/lib/sql-wasm.js` + `sql-wasm.wasm` (659 KB) — keep as files next to binary, load via `Wasm::File`
- `wol-highlights/content.css` — unchanged (50 `!important` rules are fine in a webview)

### Files that need a thin shim
- `content.js` — add `const ext = window.__wolBridge` at top (12 storage call sites, 1 message listener)
- `popup.js` — same shim, 5 storage sites + tab querying
- `background.js` — **delete**, replace with Rust menu/tray handlers

### `state.json` (replaces `chrome.storage.local`)
```json
{
  "version": 1,
  "settings": {
    "language": "es",
    "region": 4,
    "home": "https://wol.jw.org/es/wol/h/r4/lp-s",
    "cacheCapBytes": 2147483648
  },
  "wolMarks": { "...": "verbatim shape from popup.js, version: 1" }
}
```
**Keep the `wolMarks` schema byte-identical** to the extension's so existing exported JSON
imports work unchanged.

### Important: language setting is mostly a table
`lib/languages.js` currently maps only `{E: 0, S: 1}` (MEPS ids). The language setting needs
a `symbol → {mepsId, langCode, region}` table covering all languages — which is reachable by
scraping the language selector on `/wol/h/...` (all `/<lang>/wol/h/r<region>/lp-<sym>` links
were present in the home page response). **This table is real work, and it is required before
Bible-chapter highlight matching works for non-ES/EN.**

---

## 5. Settings / UX surface

| # | Task |
|---|---|
| 5.1 | Settings window: language picker (drives `home` URL), cache cap |
| 5.2 | **Cache inspector**: total size, per-entry list, "cached offline?" indicator per page, clear-all, clear-older-than |
| 5.3 | Offline indicator in chrome when a request fell through to a miss |
| 5.4 | Manager window = ported `popup.html`: load `.jwlibrary`, counts, per-color toggle, export JSON, remove |

5.2 is not optional. Invisible caching that users cannot inspect produces "why did this app
eat 4 GB" bug reports. Make the cache visible to make it trustworthy.

---

## 6. Out of scope for M1 (documented, not promised)
- `.jwpub` import / full publication library
- Multi-bitrate media prefetch
- Sync of highlights across devices
- Windows/Linux packaging (verify macOS first)
- Code signing / notarization (needed for distribution, not for the spike)

---

## 7. Verification commands

```bash
cd wol-desktop
cargo build                        # compiles
cargo tauri dev                    # runs the app
# offline test (macOS): toggle Wi-Fi off, or:
sudo pfctl -e && sudo pfctl -f /tmp/block-pf.conf   # block outbound 443
# cache inspection during development:
sqlite3 "$HOME/Library/Application Support/com.example.woldesktop/cache/index.db" \
  "SELECT url, size, datetime(last_used_at,'unixepoch') FROM entries ORDER BY last_used_at DESC LIMIT 20;"
du -sh "$HOME/Library/Application Support/com.example.woldesktop/cache/blobs"
```

---

## 8. Ordered task list (short form)

- [x] 1.1–1.7 spike; **STOP if 1.6 fails** (PASSED: spike verified, offline article and highlights render cleanly)
- [x] 2.1 SQLite schema + `CacheStore` trait
- [x] 2.2 SWR fetch path + eviction + atomic writes
- [x] 2.3 five acceptance tests (PASSED: all 5 tests passing in `tests/cache_acceptance.rs`)
- [x] 4.1 `state.json` + `get_marks`/`set_marks` commands
- [x] 4.2 `__wolBridge` shim; port `content.js`, run highlights offline
- [x] 4.3 port `popup.js` as manager window; `.jwlibrary` & JSON via file picker & drag-and-drop
- [x] 4.4 language table from selector scrape (992 languages); wire settings → home URL
- [x] 5.2 cache inspector (stats, filterable entries list, clean 30+ days, clear all)
- [x] 3.2 "save page + subresources" crawler with bounded concurrency
- [x] 3.3 "save issue" TOC crawler
- [x] 5.x settings window & polish, native application menu, offline status indicator

### Handoff line for a new session
> Read `AGENTS.md`, then `TODO.md`. Start at item 1.1.
> Do not skip the 1.6 offline go/no-go.
