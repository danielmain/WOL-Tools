# How a UserMark points at an article

This backup is `UserdataBackup_2026-10-15_iPhone.jwlibrary` (schema version 16). The publication text is not in the backup. `userData.db` stores only the highlight and a pointer to where it sits in a publication.

A highlight is two rows:

```
UserMark  --color and identity-->  Location     (which article or Bible chapter)
   |
   +-- UserMarkId -->  BlockRange               (which paragraph or verse, and which words)
```

`UserMark` itself has no article title, no paragraph number, and no highlighted text.

## Counts in this backup

| Table | Rows | Role |
|---|---:|---|
| `UserMark` | 2762 | One highlight |
| `BlockRange` | 2765 | The word span of a highlight |
| `Location` | 276 | A place in a publication |
| `Note` | 27 | A note; only sometimes attached to a highlight |
| `Bookmark` | 12 | A bookmark |
| `Tag` / `TagMap` | 14 / 34 | Tags on notes, locations, or playlist items |

All 2762 highlights sit on a `Location` of type 0. 2760 highlights have one `BlockRange`. Two highlights cross a boundary and have more than one range (UserMark 1078 has three, UserMark 1790 has two).

## UserMark

```sql
UserMarkId    INTEGER PRIMARY KEY
ColorIndex    INTEGER NOT NULL
LocationId    INTEGER NOT NULL  -- FK to Location
StyleIndex    INTEGER NOT NULL
UserMarkGuid  TEXT NOT NULL UNIQUE
Version       INTEGER NOT NULL
```

| Column | In this backup | Meaning |
|---|---|---|
| `LocationId` | | The article or Bible chapter. This is the link. |
| `ColorIndex` | 1–6 | Highlight color. 1 yellow (826), 2 green (610), 3 blue (328), 4 red (301), 5 orange (444), 6 purple (253). Grey (`0`) is used for notes that have no highlight. |
| `StyleIndex` | always 0 | Normal background highlight. |
| `Version` | always 1 | Token-layout version. The app keeps this so word offsets still apply if the publication text is revised. |
| `UserMarkGuid` | | Stable id used for sync. It does not identify the article. |

## Location: which article

```sql
LocationId, BookNumber, ChapterNumber, DocumentId, Track,
IssueTagNumber, KeySymbol, MepsLanguage, Type, Title, Specialty, Edition
```

`Type` says what the row is:

| Type | Meaning | Rows here |
|---:|---|---:|
| 0 | A document or a Bible chapter. Every highlight uses this. | 269 |
| 1 | The publication itself, used by bookmarks (`PublicationLocationId`). | 5 |
| 2 | Audio. | 0 |
| 3 | Video. | 2 |

### Magazine or book article

The article is `DocumentId`. The other columns say which publication it belongs to.

| Column | Meaning |
|---|---|
| `KeySymbol` | Publication symbol used by JW Library. |
| `MepsLanguage` | MEPS language id. `0` is English. `1` is Spanish, which is almost every row here (262 locations). One location is language `2`. |
| `IssueTagNumber` | Periodical issue as `YYYYMMDD`. `0` means the publication is not a magazine. |
| `DocumentId` | The article id. The same id is used by wol.jw.org and by `https://www.jw.org/finder?wtlocale=S&docid=<DocumentId>`. |

`IssueTagNumber` examples from this file:

- `20250600` is the June 2025 study issue (`day` `00` is the current monthly study edition).
- `20150415` is the 15 April 2015 study edition. Older study issues use day `15`. Day `01` is the old public edition.

`Title` is optional and usually empty. Only 44 of the 241 locations that have highlights have a title, so the title is not the link to the article.

Publications that have highlights in this backup:

| `KeySymbol` | Language | Locations | Highlights |
|---|---:|---:|---:|
| `w` Watchtower (La Atalaya) | 1 | 89 | 1856 |
| `wcg` | 1 | 10 | 156 |
| `lfb` | 1 | 17 | 144 |
| `nwtsty` Study Bible | 1 | 57 | 129 |
| `sfg` | 1 | 6 | 113 |
| `it` Insight (Perspicacia) | 1 | 12 | 84 |
| `od` | 1 | 3 | 79 |
| `be` | 1 | 2 | 52 |
| `th` | 1 | 5 | 25 |
| `mwb` Meeting Workbook | 1 | 8 | 23 |
| `S-315i` | 1 | 1 | 21 |
| `g` Awake (¡Despertad!) | 1 | 2 | 18 |
| `lmd` | 1 | 8 | 12 |
| `cl` | 1 | 2 | 8 |
| `wp` | 1 | 4 | 7 |
| `km` | 1 | 1 | 6 |
| `ia` | 1 | 1 | 5 |
| `jr`, `lvs` | 1 | 1–2 | 4 each |
| `S-38`, `ijwia` | 1 | 1 | 3 each |
| `Rbi8`, `nwt`, `rr` | 1 | 1–2 | 2 each |
| `S-34`, `es26`, `ws` | 1 | 1 | 1 each |
| `od` | 2 | 1 | 1 |

Symbols `S-…` are individual documents rather than a book or a magazine. Names are given only where the symbol is the standard JW Library symbol or a title in this database confirms it.

### Bible chapter

Bible highlights leave `DocumentId` empty and use:

| Column | Meaning |
|---|---|
| `KeySymbol` | Bible edition. Here: `nwtsty` (study edition), `nwt`, `Rbi8`. |
| `BookNumber` | Book, in standard order. 1 Genesis, 2 Exodus, 19 Psalms, 23 Isaiah, 66 Revelation. |
| `ChapterNumber` | Chapter. |
| `IssueTagNumber` | `0`. |

The verse is not on `Location`. It is `BlockRange.Identifier` with `BlockType` 2.

## BlockRange: which words

```sql
BlockRangeId  INTEGER PRIMARY KEY
BlockType     INTEGER NOT NULL   -- 1 or 2
Identifier    INTEGER NOT NULL
StartToken    INTEGER
EndToken      INTEGER
UserMarkId    INTEGER NOT NULL   -- FK to UserMark
```

| Column | Meaning |
|---|---|
| `BlockType` | `1` paragraph (2658 ranges). `2` Bible verse (107 ranges). |
| `Identifier` | Paragraph number, or verse number. On a study article this is the printed paragraph number. In this file, article highlights start at paragraph 2 because paragraph 1 is the title. |
| `StartToken`, `EndToken` | Token index inside that paragraph or verse. The index starts at 0 and `EndToken` is included. A single token has `StartToken = EndToken`. |

No range in this file has a null token or an end before the start. 50 ranges start at word 0.

A highlight that runs past the end of a paragraph or verse is still one `UserMark`, with one `BlockRange` per block. The two cases here:

- UserMark 1078, Watchtower December 2025, document `2025682`: paragraph 19 words 88–138, paragraph 20 words 0–161, and paragraph 57 words 0–13.
- UserMark 1790, study Bible, Numbers (book 4) chapter 27: verse 16 words 13–20 and verse 17 words 0–27.

## Worked examples

First highlight in the file:

```
UserMark 1
  ColorIndex     1          yellow
  StyleIndex     0
  Version        1
  LocationId     1
    KeySymbol       w
    MepsLanguage    1        Spanish
    IssueTagNumber  20250600 June 2025 study issue
    DocumentId      2025441  the article
  BlockRange
    BlockType    1          paragraph
    Identifier   7
    StartToken   53
    EndToken     62         tokens 53 through 62, inclusive
```

That article is “Qué nos enseñan las últimas palabras de Jacob (parte 2)”, at `https://wol.jw.org/es/wol/d/r4/lp-s/2025441` and `https://www.jw.org/finder?wtlocale=S&docid=2025441`.

A Bible highlight:

```
UserMark 2708
  ColorIndex  1
  Location
    KeySymbol      nwtsty
    BookNumber     2       Exodus
    ChapterNumber  5
    DocumentId     NULL
  BlockRange
    BlockType   2          verse
    Identifier  9          Exodus 5:9
    StartToken  0
    EndToken    15         tokens 0 through 15, the whole verse
```

## Notes are a separate link

`Note` may point at a highlight through `UserMarkId`, but it does not have to. A note always has its own place:

- `LocationId` is the same article or chapter key as above.
- `BlockType` and `BlockIdentifier` are the paragraph or verse, using the same values as `BlockRange`.
- `UserMarkId` is set only when the note was attached to a highlight.

11 of the 27 notes in this backup are attached to a highlight. The others are attached only to a paragraph or verse. Tags hang off `Note`, `Location`, or `PlaylistItem` through `TagMap`. They do not hang off `UserMark`.

## What this file cannot answer

The words of the highlight are not stored. To read them, open the publication (or the finder URL for that `DocumentId`) and take tokens `StartToken` through `EndToken` of paragraph or verse `Identifier`.

`PlaylistItemMarker` is unrelated. Those rows are timestamps inside a playlist item, not highlights in an article.

## What a token is

A token is a word or one punctuation mark. It is not only a stretch of letters.

- A run of letters, digits, or connector punctuation such as `_` is one token. Accented letters (`á`, `ñ`) stay inside the word.
- Every other character that is not whitespace is its own token. A quotation mark, a period, and a comma each count.
- Whitespace is not a token. That includes a normal space, a non-breaking space (U+00A0), and a narrow no-break space (U+202F).
- A soft hyphen (U+00AD) and a zero-width space are each a token. A combining acute accent (U+0301) is not part of the letter it sits on.

On wol.jw.org the highlighter skips text that the app does not count: paragraph numbers (`.parNum`), page numbers (`.pageNum`), footnote markers (`.fn`), verse numbers (`a.vl`, `a.cl`), ruby text (`rt`, `rp`), and a cross-reference whose only text is `+`. The words inside a scripture link are tokens.

`EndToken` past the last token of that paragraph or verse is skipped. The span is not shortened to fit.

These spans were checked against the page text. The same phrases were still the highlighted text on the live site in Firefox 158 on 15 October 2026:

| Place | Tokens | Text |
|---|---|---|
| Document `2025441`, paragraph 7 | 53–62 | Qué les diría ahora al resto de sus hijos varones |
| Same paragraph | 74–88 | Includes Zabulón and Benjamín |
| Paragraph 10 | | siempre hay razones para sentirnos felices |
| `nwtsty` Exodus 5:9 | 0–15 | Pónganlos a trabajar más duro y manténganlos tan ocupados que no puedan escuchar mentiras”. |
| `nwtsty` Exodus 6:3 | 0–29 | Begins “Yo me aparecía” and ends at “ellos”, before the final period |
| `nwtsty` Exodus 6:6 | 37–45 | Los rescataré con brazo poderoso y con grandes castigos |

## The same place on wol.jw.org

The article page is `https://wol.jw.org/<language>/wol/d/r4/lp-<language>/<DocumentId>`. Spanish uses `lp-s`, and the article element has `data-lang="S"`. The number at the end of the path is `Location.DocumentId`. In this backup a document id belongs to one location, so an article highlight does not need `MepsLanguage` to be found.

Paragraphs are `p[data-pid]` inside `article#article`. `data-pid` is `BlockRange.Identifier`. On a study article, pid 1 is the title, which is why the highlights in this file start at paragraph 2.

A Bible chapter is `https://wol.jw.org/es/wol/b/r4/lp-s/nwtsty/2/5` for the study Bible, Exodus 5. The same chapter can also appear under `/wol/binav/`. The edition is `KeySymbol`, then the book and the chapter. `BookNumber` follows the usual order: 1 Genesis, 2 Exodus. Each verse is one or more `span.v` elements whose id is `v<book>-<chapter>-<verse>-<part>`, such as `v2-5-9-1`. The verse number in that id is `BlockRange.Identifier` when `BlockType` is 2.

The page writes the language as a symbol. The database writes it as `MepsLanguage`. The confirmed pairs are `E` = 0 and `S` = 1. Windows locale ids are a different numbering. This file has one location in language 2 (`od`, one highlight); the symbol for that id is not confirmed. A Bible chapter in any other language still matches when the backup has that symbol, book, and chapter in only one language.

## Implementations

The rules above exist twice, and both must agree, because a backup written by one is read by
the other:

- `wol-highlights/lib/tokenize.js` — the browser extension's tokenizer (the reference).
- `wol-core/src/tokenize.rs` — the Rust port used by OfflineWOL and `wol-cli`. It classifies
  characters by Unicode general category (`\p{L}` = Lu/Ll/Lt/Lm/Lo, `\p{N}` = Nd/Nl/No), so
  combining marks stay separate tokens, and it emits one token per UTF-16 code unit, so astral
  characters are two tokens.
- Parity is enforced, not assumed: `npm run test:tokenize` regenerates
  `wol-core/tests/fixtures/tokenize_cases.json` from the JS, and
  `cargo test -p wol-core --test tokenize_parity` replays it against Rust. Add a case there
  whenever a tokenization edge case is discovered.

## Add-on

`wol-highlights/` loads `userData.db` or the `.jwlibrary` zip in Firefox and paints the ranges on wol.jw.org. Install steps are in `wol-highlights/README.md`. The backup is not shipped inside the add-on, and the highlights are not sent anywhere.

The same ranges are painted by **OfflineWOL** (`wol-desktop/`, Tauri) and read/written by
**`wol`** (`wol-cli/`, terminal), which share `wol-core/` and the same data directory — a
highlight created in any of the three shows up in the others.

From this file the index is 2,765 ranges, 184 articles, and 56 Bible chapters. `userData.db` and `userdata.zip` produce the same index.

Painted colors are yellow `#ffe36b`, green `#b6e89a`, blue `#a9d4fb`, red `#f7b4c4`, orange `#ffc48a`, and purple `#d8c0f8`. The text keeps the color of the page. A range that crosses a link becomes more than one `<mark>`. On the live Jacob article, 69 ranges painted 76 marks. The first of them, paragraph 7 tokens 53–62, was yellow. Exodus 5:9 was one mark covering the verse and not the verse number.
