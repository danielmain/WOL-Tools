# WOL Highlights

A Firefox add-on that reads a JW Library backup and paints those highlights on the same article at [wol.jw.org](https://wol.jw.org).

The backup does not contain the article text. Each highlight points at a publication location and a word span. On an article page the add-on finds the same paragraph and wraps that span. On a Bible chapter it does the same for the verse. How those rows are stored is written in [`usermark.md`](../usermark.md).

## Install

The add-on is not signed, so Firefox loads it for the current session.

1. Open `about:debugging`.
2. Choose **This Firefox**.
3. Choose **Load Temporary Add-on...**.
4. Select `manifest.json` **inside this `wol-highlights` folder** (e.g. `wol-highlights/manifest.json`).
   > **Note**: Do **not** select the `manifest.json` in the parent backup directory—that is the JW Library backup metadata and will cause an `Unexpected params.manifestVersion value: undefined` error.

Temporary add-ons are removed when Firefox exits. Load it again the next time.

## Use

1. Open the toolbar button.
2. Choose **Load backup**.
3. Pick either file:
   - the `.jwlibrary` backup from JW Library (Personal Study, Backup and Restore)
   - `userData.db` from inside that backup
4. Open the same article on `https://wol.jw.org`.

Colors follow the app: yellow, green, blue, red, orange, purple. **Show highlights** hides them without deleting the loaded file. **Remove loaded backup** deletes only the copy stored in Firefox. Nothing is uploaded.

## How a highlight is found

`UserMark` points at a `Location`. `BlockRange` says which words.

| Page | What matches |
|---|---|
| Article `/wol/d/.../<document id>` | `Location.DocumentId`, then `p[data-pid]` equal to `BlockRange.Identifier` |
| Bible chapter `/wol/b/.../<symbol>/<book>/<chapter>` | `KeySymbol`, book, chapter, and the verse span `v<book>-<chapter>-<verse>` |

`Identifier` for an article is the paragraph number (`data-pid`), the same number printed in a study article. For a Bible chapter it is the verse. `StartToken` and `EndToken` are token indexes starting at 0, and the end index is included. A punctuation mark is its own token. Paragraph numbers, page numbers, footnote letters, verse numbers, and the study-Bible `+` markers are not tokens.

Checked against this backup, on the saved page and again on the live site in Firefox 158:

- Watchtower document `2025441` paragraph 7, tokens 53–62: “Qué les diría ahora al resto de sus hijos varones”. Tokens 74–88 include Zabulón and Benjamín.
- Paragraph 10 includes “siempre hay razones para sentirnos felices”.
- Study Bible Exodus 5:9, tokens 0–15, is the whole verse and not the verse number.
- Exodus 6:3, tokens 0–29, ends at “ellos” and leaves the final period out.
- Exodus 6:6, tokens 37–45: “Los rescataré con brazo poderoso y con grandes castigos”.

Loading `userdata.zip` reported 2,765 highlights in 184 articles and 56 Bible chapters. The Jacob article’s 69 ranges painted 76 marks, because a range that crosses a link is more than one mark. The paragraph 7 highlight was yellow, `#ffe36b`.

## Scripts

Firefox 158 ignores `"type": "module"` on a content script, so `lib/tokenize.js`, `lib/languages.js`, `lib/highlight.js`, and `content.js` are ordinary scripts, loaded in that order. A `function` declaration in one of them is visible to the later ones. `import` and `export` in those files do not run. `popup.js` is a module. It calls `parseBackup` from `lib/backup.js`, which `popup.html` loads first.

## Limits

The page text has to be the same text the highlight was made on. If Watchtower Online Library and the app disagree inside a paragraph, that highlight is skipped rather than painted on the wrong words. The popup says when that happens.

Notes, bookmarks, and playlist markers are not shown. Bible chapters in a language other than English or Spanish still match when the backup contains that chapter in only one language.

The database reader is [sql.js](https://github.com/sql-js/sql.js) 1.13.0 (MIT). `lib/sql-wasm.js` and `lib/sql-wasm.wasm` are that distribution.
