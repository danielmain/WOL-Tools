//! Rust port of `wol-highlights/lib/backup.js`.
//!
//! Reads a `.jwlibrary` archive (a ZIP containing `userData.db`) or a bare
//! `userData.db` / `marks.json` file and builds the exact `wolMarks`
//! ([`StoredIndex`]) shape the browser extension and the desktop app consume.
//!
//! Only `rusqlite` + `zip` are needed — no `sql.js`/WebAssembly, which is what
//! makes the terminal client possible at all.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufReader, Read, Seek};
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// `[startToken, endToken, colorIndex]`, `endToken` inclusive — identical to the
/// extension's tuple layout.
pub type Span = (i64, i64, i64);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocMeta {
    pub key_symbol: Option<String>,
    #[serde(default)]
    pub issue_tag_number: i64,
    pub meps_language: Option<i64>,
    pub title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteItem {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guid: Option<String>,
    #[serde(default)]
    pub user_mark_id: Option<i64>,
    #[serde(default)]
    pub location_id: Option<i64>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub last_modified: Option<String>,
    #[serde(default)]
    pub created: Option<String>,
    #[serde(default)]
    pub block_type: Option<i64>,
    #[serde(default)]
    pub block_identifier: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotesIndex {
    #[serde(default)]
    pub docs: BTreeMap<String, BTreeMap<String, Vec<NoteItem>>>,
    #[serde(default)]
    pub bible: BTreeMap<String, BTreeMap<String, Vec<NoteItem>>>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InputFieldIndex {
    #[serde(default)]
    pub docs: BTreeMap<String, BTreeMap<String, String>>,
    #[serde(default)]
    pub bible: BTreeMap<String, BTreeMap<String, String>>,
}

/// The `wolMarks` object. Field names and nesting match the extension exactly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredIndex {
    pub version: u32,
    pub highlight_count: u64,
    #[serde(default)]
    pub note_count: u64,
    pub ignored: u64,
    pub document_count: u64,
    pub bible_chapter_count: u64,
    pub color_counts: BTreeMap<i64, u64>,
    #[serde(default)]
    pub file_name: Option<String>,
    #[serde(default)]
    pub loaded_at: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub docs: BTreeMap<String, BTreeMap<String, Vec<Span>>>,
    #[serde(default)]
    pub bible: BTreeMap<String, BTreeMap<String, Vec<Span>>>,
    #[serde(default)]
    pub doc_meta: BTreeMap<String, DocMeta>,
    #[serde(default)]
    pub notes: NotesIndex,
    #[serde(default)]
    pub input_fields: InputFieldIndex,
}

fn default_true() -> bool {
    true
}

impl Default for StoredIndex {
    fn default() -> Self {
        Self {
            version: 1,
            highlight_count: 0,
            note_count: 0,
            ignored: 0,
            document_count: 0,
            bible_chapter_count: 0,
            color_counts: BTreeMap::new(),
            file_name: None,
            loaded_at: None,
            enabled: true,
            docs: BTreeMap::new(),
            bible: BTreeMap::new(),
            doc_meta: BTreeMap::new(),
            notes: NotesIndex::default(),
            input_fields: InputFieldIndex::default(),
        }
    }
}

impl StoredIndex {
    /// Ranges stored for an article paragraph.
    pub fn doc_ranges(&self, document_id: &str, pid: &str) -> &[Span] {
        self.docs
            .get(document_id)
            .and_then(|paragraphs| paragraphs.get(pid))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Notes stored for an article paragraph.
    pub fn doc_notes(&self, document_id: &str, pid: &str) -> &[NoteItem] {
        self.notes
            .docs
            .get(document_id)
            .and_then(|paragraphs| paragraphs.get(pid))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Input-field text (`.gen-field`) stored for an article paragraph.
    pub fn doc_input_field(&self, document_id: &str, text_tag: &str) -> Option<&str> {
        self.input_fields
            .docs
            .get(document_id)
            .and_then(|fields| fields.get(text_tag))
            .map(String::as_str)
    }

    /// Bible chapter key as used by the extension: `mepsLang|keysymbol|book|chapter`.
    pub fn bible_key(
        meps_language: Option<i64>,
        key_symbol: &str,
        book_number: i64,
        chapter_number: i64,
    ) -> String {
        format!(
            "{}|{}|{}|{}",
            meps_language.map(|v| v.to_string()).unwrap_or_else(|| "null".to_string()),
            key_symbol.to_lowercase(),
            book_number,
            chapter_number
        )
    }

    pub fn bible_ranges(&self, key: &str, verse: &str) -> &[Span] {
        self.bible
            .get(key)
            .and_then(|verses| verses.get(verse))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Sum of stored ranges across documents and Bible chapters.
    pub fn total_ranges(&self) -> usize {
        let docs = self
            .docs
            .values()
            .flat_map(|paragraphs| paragraphs.values())
            .map(Vec::len)
            .sum::<usize>();
        let bible = self
            .bible
            .values()
            .flat_map(|verses| verses.values())
            .map(Vec::len)
            .sum::<usize>();
        docs + bible
    }
}

#[derive(Debug, Error)]
pub enum BackupError {
    #[error("could not read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("the backup archive is corrupt or truncated: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("could not read this database: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("no userData.db inside this backup")]
    MissingDatabase,
    #[error("that file is not a valid JW Library backup, SQLite database, or marks.json")]
    UnrecognizedFormat,
    #[error("invalid marks.json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("could not extract userData.db to a temporary file: {0}")]
    Temp(std::io::Error),
}

/// What the input file turned out to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportFormat {
    ZipArchive,
    SqliteDatabase,
    MarksJson,
}

#[derive(Debug, Clone)]
pub struct ImportSource {
    path: PathBuf,
    format: ImportFormat,
}

impl ImportSource {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn format(&self) -> ImportFormat {
        self.format
    }

    pub fn file_name(&self) -> Option<String> {
        self.path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
    }
}

/// Sniff the input format without parsing it.
pub fn detect_format(path: &Path) -> Result<ImportFormat, BackupError> {
    let mut header = [0u8; 16];
    let read = read_header(path, &mut header)?;
    let bytes = &header[..read];
    if bytes.starts_with(b"PK\x03\x04") {
        return Ok(ImportFormat::ZipArchive);
    }
    if bytes.starts_with(b"SQLite format 3\0") {
        return Ok(ImportFormat::SqliteDatabase);
    }
    if is_likely_json(path, bytes)? {
        return Ok(ImportFormat::MarksJson);
    }
    Err(BackupError::UnrecognizedFormat)
}

fn read_header(path: &Path, buffer: &mut [u8]) -> Result<usize, BackupError> {
    let mut file = File::open(path).map_err(|source| BackupError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    file.read(buffer).map_err(|source| BackupError::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn is_likely_json(path: &Path, header: &[u8]) -> Result<bool, BackupError> {
    if header.first().is_some_and(|byte| *byte == b'{' || *byte == b'[') {
        return Ok(true);
    }
    let json_extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("json"));
    Ok(json_extension)
}

/// Read `userData.db` out of a `.jwlibrary` zip into a byte buffer.
pub fn extract_user_data_db(path: &Path) -> Result<Vec<u8>, BackupError> {
    let file = File::open(path).map_err(|source| BackupError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut archive = zip::ZipArchive::new(BufReader::new(file))?;
    let index = (0..archive.len())
        .find(|index| {
            archive
                .by_index(*index)
                .map(|entry| is_user_data_entry(entry.name()))
                .unwrap_or(false)
        })
        .ok_or(BackupError::MissingDatabase)?;
    let mut entry = archive.by_index(index)?;
    let mut bytes = Vec::with_capacity(usize::try_from(entry.size()).unwrap_or(0));
    entry.read_to_end(&mut bytes).map_err(BackupError::Temp)?;
    Ok(bytes)
}

fn is_user_data_entry(name: &str) -> bool {
    let lowercase = name.to_ascii_lowercase();
    lowercase == "userdata.db" || lowercase.ends_with("/userdata.db")
}

/// Load a `.jwlibrary`, `userData.db`, or `marks.json` into a [`StoredIndex`].
pub fn import_backup(path: &Path) -> Result<StoredIndex, BackupError> {
    let format = detect_format(path)?;
    let mut index = match format {
        ImportFormat::MarksJson => read_marks_json(path)?,
        ImportFormat::SqliteDatabase => read_database(path)?,
        ImportFormat::ZipArchive => {
            let bytes = extract_user_data_db(path)?;
            read_database_bytes(&bytes)?
        }
    };
    index.file_name = Some(
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "userData.db".to_string()),
    );
    index.loaded_at = Some(crate::state::now_iso_utc());
    index.version = 1;
    Ok(index)
}

fn read_marks_json(path: &Path) -> Result<StoredIndex, BackupError> {
    let file = File::open(path).map_err(|source| BackupError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(serde_json::from_reader(BufReader::new(file))?)
}

fn read_database(path: &Path) -> Result<StoredIndex, BackupError> {
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    index_connection(&connection)
}

fn read_database_bytes(bytes: &[u8]) -> Result<StoredIndex, BackupError> {
    let temp = TempSqlite::open(bytes)?;
    index_connection(temp.connection())
}

/// On-disk scratch copy of an extracted `userData.db` (rusqlite needs a path).
struct TempSqlite {
    dir: PathBuf,
    connection: Option<Connection>,
}

impl TempSqlite {
    /// Open an in-memory scratch database seeded from `bytes`.
    fn open(bytes: &[u8]) -> Result<Self, BackupError> {
        let dir = std::env::temp_dir().join(format!(
            "wol-core-import-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).map_err(BackupError::Temp)?;
        let db_path = dir.join("userData.db");
        std::fs::write(&db_path, bytes).map_err(BackupError::Temp)?;
        let connection = match Connection::open_with_flags(&db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        {
            Ok(connection) => connection,
            Err(err) => {
                let _ = std::fs::remove_dir_all(&dir);
                return Err(BackupError::Sqlite(err));
            }
        };
        Ok(Self {
            dir,
            connection: Some(connection),
        })
    }

    fn connection(&self) -> &Connection {
        self.connection
            .as_ref()
            .expect("connection is present until drop")
    }
}

impl Drop for TempSqlite {
    fn drop(&mut self) {
        self.connection.take();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

const HIGHLIGHT_QUERY: &str = "
SELECT
  l.DocumentId, l.BookNumber, l.ChapterNumber, l.KeySymbol, l.IssueTagNumber,
  l.MepsLanguage, l.Title, b.BlockType, b.Identifier, b.StartToken, b.EndToken,
  u.ColorIndex
FROM UserMark u
JOIN Location l ON l.LocationId = u.LocationId
JOIN BlockRange b ON b.UserMarkId = u.UserMarkId
WHERE l.Type = 0
  AND b.StartToken IS NOT NULL
  AND b.EndToken IS NOT NULL
";

const NOTE_QUERY: &str = "
SELECT
  n.NoteId, n.Guid, n.UserMarkId, n.LocationId, n.Title, n.Content,
  n.LastModified, n.Created, n.BlockType, n.BlockIdentifier,
  l.DocumentId, l.BookNumber, l.ChapterNumber, l.KeySymbol, l.MepsLanguage
FROM Note n
LEFT JOIN Location l ON l.LocationId = n.LocationId
WHERE (n.BlockType = 1 AND l.DocumentId IS NOT NULL)
   OR (n.BlockType = 2 AND l.BookNumber IS NOT NULL AND l.ChapterNumber IS NOT NULL)
   OR (l.DocumentId IS NOT NULL AND n.BlockIdentifier IS NOT NULL)
";

const INPUT_FIELD_QUERY: &str = "
SELECT
  i.LocationId, i.TextTag, i.Value, l.DocumentId, l.BookNumber, l.ChapterNumber,
  l.KeySymbol, l.MepsLanguage
FROM InputField i
LEFT JOIN Location l ON l.LocationId = i.LocationId
WHERE l.DocumentId IS NOT NULL
   OR (l.BookNumber IS NOT NULL AND l.ChapterNumber IS NOT NULL)
";

/// Raw highlight row — the port of one `UserMark`/`Location`/`BlockRange` join.
#[derive(Debug, Clone, PartialEq, Eq)]
struct HighlightRow {
    document_id: Option<String>,
    book_number: Option<i64>,
    chapter_number: Option<i64>,
    key_symbol: Option<String>,
    issue_tag_number: Option<i64>,
    meps_language: Option<i64>,
    title: Option<String>,
    block_type: Option<i64>,
    identifier: Option<String>,
    start_token: Option<i64>,
    end_token: Option<i64>,
    color_index: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NoteRow {
    note_id: Option<i64>,
    guid: Option<String>,
    user_mark_id: Option<i64>,
    location_id: Option<i64>,
    title: Option<String>,
    content: Option<String>,
    last_modified: Option<String>,
    created: Option<String>,
    block_type: Option<i64>,
    block_identifier: Option<i64>,
    document_id: Option<String>,
    book_number: Option<i64>,
    chapter_number: Option<i64>,
    key_symbol: Option<String>,
    meps_language: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct InputFieldRow {
    text_tag: Option<String>,
    value: Option<String>,
    document_id: Option<String>,
    book_number: Option<i64>,
    chapter_number: Option<i64>,
    key_symbol: Option<String>,
    meps_language: Option<i64>,
}

fn table_exists(connection: &Connection, table: &str) -> bool {
    connection
        .query_row(
            "SELECT name FROM sqlite_master WHERE type='table' AND name=?1",
            [table],
            |_| Ok(()),
        )
        .is_ok()
}

fn collect<T>(
    connection: &Connection,
    sql: &str,
    map: impl Fn(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
) -> Result<Vec<T>, BackupError> {
    let mut statement = connection.prepare(sql)?;
    let rows = statement.query_map([], map)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

fn index_connection(connection: &Connection) -> Result<StoredIndex, BackupError> {
    let highlights = collect(connection, HIGHLIGHT_QUERY, |row| {
        Ok(HighlightRow {
            document_id: opt_text(row, 0)?,
            book_number: opt_int(row, 1)?,
            chapter_number: opt_int(row, 2)?,
            key_symbol: opt_text(row, 3)?,
            issue_tag_number: opt_int(row, 4)?,
            meps_language: opt_int(row, 5)?,
            title: opt_text(row, 6)?,
            block_type: opt_int(row, 7)?,
            identifier: opt_text(row, 8)?,
            start_token: opt_int(row, 9)?,
            end_token: opt_int(row, 10)?,
            color_index: opt_int(row, 11)?,
        })
    })?;

    let notes = if table_exists(connection, "Note") {
        collect(connection, NOTE_QUERY, |row| {
            Ok(NoteRow {
                note_id: opt_int(row, 0)?,
                guid: opt_text(row, 1)?,
                user_mark_id: opt_int(row, 2)?,
                location_id: opt_int(row, 3)?,
                title: opt_text(row, 4)?,
                content: opt_text(row, 5)?,
                last_modified: opt_text(row, 6)?,
                created: opt_text(row, 7)?,
                block_type: opt_int(row, 8)?,
                block_identifier: opt_int(row, 9)?,
                document_id: opt_text(row, 10)?,
                book_number: opt_int(row, 11)?,
                chapter_number: opt_int(row, 12)?,
                key_symbol: opt_text(row, 13)?,
                meps_language: opt_int(row, 14)?,
            })
        })?
    } else {
        Vec::new()
    };

    let input_fields = if table_exists(connection, "InputField") {
        collect(connection, INPUT_FIELD_QUERY, |row| {
            Ok(InputFieldRow {
                text_tag: opt_text(row, 1)?,
                value: opt_text(row, 2)?,
                document_id: opt_text(row, 3)?,
                book_number: opt_int(row, 4)?,
                chapter_number: opt_int(row, 5)?,
                key_symbol: opt_text(row, 6)?,
                meps_language: opt_int(row, 7)?,
            })
        })?
    } else {
        Vec::new()
    };

    Ok(build_index(&highlights, &notes, &input_fields))
}

/// JW Library stores numeric ids as INTEGER; accept TEXT spellings too.
fn as_string(value: rusqlite::types::ValueRef<'_>) -> Option<String> {
    use rusqlite::types::ValueRef;
    match value {
        ValueRef::Null => None,
        ValueRef::Integer(number) => Some(number.to_string()),
        ValueRef::Real(number) => Some(number.to_string()),
        ValueRef::Text(bytes) => Some(String::from_utf8_lossy(bytes).into_owned()),
        ValueRef::Blob(bytes) => Some(String::from_utf8_lossy(bytes).into_owned()),
    }
}

fn opt_text(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<Option<String>> {
    Ok(as_string(row.get_ref(index)?).filter(|text| !text.is_empty()))
}

fn opt_int(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<Option<i64>> {
    use rusqlite::types::ValueRef;
    match row.get_ref(index)? {
        ValueRef::Null => Ok(None),
        ValueRef::Integer(number) => Ok(Some(number)),
        ValueRef::Real(number) => Ok(Some(number as i64)),
        ValueRef::Text(bytes) => Ok(String::from_utf8_lossy(bytes).trim().parse().ok()),
        ValueRef::Blob(_) => Ok(None),
    }
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|text| !text.is_empty())
}

/// Pure fold from DB rows to `StoredIndex` — the port of `indexRows()`.
fn build_index(
    highlight_rows: &[HighlightRow],
    note_rows: &[NoteRow],
    input_field_rows: &[InputFieldRow],
) -> StoredIndex {
    let mut index = StoredIndex::default();
    let mut color_counts: BTreeMap<i64, u64> = BTreeMap::new();
    let mut highlight_count = 0u64;
    let mut ignored = 0u64;

    for row in highlight_rows {
        let (Some(start), Some(end)) = (row.start_token, row.end_token) else {
            ignored += 1;
            continue;
        };
        if end < start {
            ignored += 1;
            continue;
        }
        let Some(identifier) = row.identifier.as_ref().filter(|value| !value.is_empty()) else {
            ignored += 1;
            continue;
        };
        let color = row.color_index.unwrap_or(0);
        let span: Span = (start, end, color);

        if row.block_type == Some(1) {
            let Some(document_id) = row.document_id.as_ref().filter(|value| !value.is_empty()) else {
                ignored += 1;
                continue;
            };
            index
                .docs
                .entry(document_id.clone())
                .or_default()
                .entry(identifier.clone())
                .or_default()
                .push(span);
            index.doc_meta.entry(document_id.clone()).or_insert(DocMeta {
                key_symbol: row.key_symbol.clone().filter(|value| !value.is_empty()),
                issue_tag_number: row.issue_tag_number.unwrap_or(0),
                meps_language: row.meps_language,
                title: row.title.clone().filter(|value| !value.is_empty()),
            });
            highlight_count += 1;
            *color_counts.entry(color).or_insert(0) += 1;
        } else if row.block_type == Some(2) {
            let (Some(book), Some(chapter), Some(symbol)) = (
                row.book_number.filter(|value| *value != 0),
                row.chapter_number.filter(|value| *value != 0),
                row.key_symbol.as_ref().filter(|value| !value.is_empty()),
            ) else {
                ignored += 1;
                continue;
            };
            let key = StoredIndex::bible_key(row.meps_language, symbol, book, chapter);
            index
                .bible
                .entry(key)
                .or_default()
                .entry(identifier.clone())
                .or_default()
                .push(span);
            highlight_count += 1;
            *color_counts.entry(color).or_insert(0) += 1;
        } else {
            ignored += 1;
        }
    }

    for group in [&mut index.docs, &mut index.bible] {
        for blocks in group.values_mut() {
            for spans in blocks.values_mut() {
                spans.sort_by_key(|(start, end, _)| (*start, *end));
            }
        }
    }

    let mut note_count = 0u64;
    for row in note_rows {
        let item = NoteItem {
            note_id: row.note_id,
            guid: row.guid.clone().filter(|value| !value.is_empty()),
            user_mark_id: row.user_mark_id,
            location_id: row.location_id,
            title: row.title.clone().filter(|value| !value.is_empty()),
            content: row.content.clone().unwrap_or_default(),
            last_modified: non_empty(row.last_modified.clone()),
            created: non_empty(row.created.clone()),
            block_type: row.block_type,
            block_identifier: row.block_identifier,
        };

        let document_scope = row
            .document_id
            .as_ref()
            .filter(|value| !value.is_empty())
            .filter(|_| row.block_type == Some(1) || row.block_identifier.is_some());
        if let (Some(document_id), Some(identifier)) = (document_scope, row.block_identifier) {
            index
                .notes
                .docs
                .entry(document_id.clone())
                .or_default()
                .entry(identifier.to_string())
                .or_default()
                .push(item);
            note_count += 1;
            continue;
        }

        let bible_scope = row.block_type == Some(2)
            && row.book_number.is_some_and(|value| value != 0)
            && row.chapter_number.is_some_and(|value| value != 0);
        if let (true, Some(symbol), Some(book), Some(chapter), Some(identifier)) = (
            bible_scope,
            row.key_symbol.as_ref().filter(|value| !value.is_empty()),
            row.book_number,
            row.chapter_number,
            row.block_identifier,
        ) {
            let key = StoredIndex::bible_key(row.meps_language, symbol, book, chapter);
            index
                .notes
                .bible
                .entry(key)
                .or_default()
                .entry(identifier.to_string())
                .or_default()
                .push(item);
            note_count += 1;
        }
    }

    let mut input_field_count = 0u64;
    for row in input_field_rows {
        let Some(tag) = row.text_tag.as_ref().filter(|value| !value.is_empty()) else {
            continue;
        };
        let value = row.value.clone().unwrap_or_default();
        let filled = !value.trim().is_empty();

        let document_scope = row.document_id.as_ref().filter(|value| !value.is_empty());
        if let Some(document_id) = document_scope {
            index
                .input_fields
                .docs
                .entry(document_id.clone())
                .or_default()
                .insert(tag.clone(), value);
            if filled {
                input_field_count += 1;
            }
            continue;
        }

        if let (Some(symbol), Some(book), Some(chapter)) = (
            row.key_symbol.as_ref().filter(|value| !value.is_empty()),
            row.book_number,
            row.chapter_number,
        ) {
            let key = StoredIndex::bible_key(row.meps_language, symbol, book, chapter);
            index
                .input_fields
                .bible
                .entry(key)
                .or_default()
                .insert(tag.clone(), value);
            if filled {
                input_field_count += 1;
            }
        }
    }

    index.highlight_count = highlight_count;
    index.note_count = note_count + input_field_count;
    index.ignored = ignored;
    index.document_count = index.docs.len() as u64;
    index.bible_chapter_count = index.bible.len() as u64;
    index.color_counts = color_counts;
    index
}

/// Recompute counters from the stored ranges/notes (used after CLI edits).
pub fn recount(index: &StoredIndex) -> StoredIndex {
    let mut next = index.clone();
    let mut color_counts: BTreeMap<i64, u64> = BTreeMap::new();
    let mut highlight_count = 0u64;
    for spans in index
        .docs
        .values()
        .flat_map(|paragraphs| paragraphs.values())
        .chain(index.bible.values().flat_map(|verses| verses.values()))
    {
        for (_, _, color) in spans {
            highlight_count += 1;
            *color_counts.entry(*color).or_insert(0) += 1;
        }
    }
    let note_rows = index
        .notes
        .docs
        .values()
        .flat_map(|paragraphs| paragraphs.values())
        .chain(index.notes.bible.values().flat_map(|verses| verses.values()))
        .map(Vec::len)
        .sum::<usize>();
    let filled_fields = index
        .input_fields
        .docs
        .values()
        .flat_map(|fields| fields.values())
        .chain(index.input_fields.bible.values().flat_map(|fields| fields.values()))
        .filter(|value| !value.trim().is_empty())
        .count();

    next.highlight_count = highlight_count;
    next.color_counts = color_counts;
    next.note_count = (note_rows + filled_fields) as u64;
    next.document_count = index.docs.len() as u64;
    next.bible_chapter_count = index.bible.len() as u64;
    next.version = 1;
    next
}

/// Read a `.jwlibrary` archive's bytes from any reader (used by tests).
pub fn read_zip_entry<R: Read + Seek>(
    reader: R,
    matches: impl Fn(&str) -> bool,
) -> Result<Vec<u8>, BackupError> {
    let mut archive = zip::ZipArchive::new(reader)?;
    let index = (0..archive.len())
        .find(|index| {
            archive
                .by_index(*index)
                .map(|entry| matches(entry.name()))
                .unwrap_or(false)
        })
        .ok_or(BackupError::MissingDatabase)?;
    let mut entry = archive.by_index(index)?;
    let mut bytes = Vec::new();
    entry.read_to_end(&mut bytes).map_err(BackupError::Temp)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests;
