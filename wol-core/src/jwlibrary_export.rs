use std::collections::HashMap;
use std::fs::File;
use std::io::{Cursor, Seek, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

const SCHEMA_VERSION: i64 = 16;

const SCHEMA_SQL: &str = r#"
PRAGMA journal_mode = DELETE;
PRAGMA user_version = 16;

CREATE TABLE IF NOT EXISTS LastModified (
  LastModified TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS Location (
  LocationId INTEGER NOT NULL PRIMARY KEY,
  BookNumber INTEGER,
  ChapterNumber INTEGER,
  DocumentId INTEGER,
  Track INTEGER,
  IssueTagNumber INTEGER NOT NULL DEFAULT 0,
  KeySymbol TEXT,
  MepsLanguage INTEGER,
  Type INTEGER NOT NULL,
  Title TEXT,
  Specialty TEXT,
  Edition TEXT,
  UNIQUE(BookNumber, ChapterNumber, KeySymbol, MepsLanguage, Type),
  CHECK ((
    Type = 0 AND (
      (DocumentId IS NOT NULL AND DocumentId != 0)
      OR (
        Track IS NOT NULL AND (
          (KeySymbol IS NOT NULL AND (length(KeySymbol) > 0))
          OR (DocumentId IS NOT NULL AND DocumentId != 0)))
      OR (
        BookNumber IS NOT NULL AND BookNumber != 0
        AND KeySymbol IS NOT NULL AND (length(KeySymbol) > 0)
        AND (ChapterNumber IS NULL OR ChapterNumber = 0))
      OR (
        ChapterNumber IS NOT NULL AND ChapterNumber != 0
        AND BookNumber IS NOT NULL AND BookNumber != 0
        AND KeySymbol IS NOT NULL AND (length(KeySymbol) > 0))))
    OR Type != 0),
  CHECK ((
    Type = 1
      AND (BookNumber IS NULL OR BookNumber = 0)
      AND (ChapterNumber IS NULL OR ChapterNumber = 0)
      AND (DocumentId IS NULL OR DocumentId = 0)
      AND KeySymbol IS NOT NULL AND (length(KeySymbol) > 0)
      AND Track IS NULL)
    OR Type != 1),
  CHECK ((
    Type IN (2, 3)
      AND (BookNumber IS NULL OR BookNumber = 0)
      AND (ChapterNumber IS NULL OR ChapterNumber = 0))
    OR Type NOT IN (2, 3))
);

CREATE INDEX IF NOT EXISTS IX_Location_KeySymbol_MepsLanguage_BookNumber_ChapterNumber
  ON Location(KeySymbol, MepsLanguage, BookNumber, ChapterNumber);

CREATE UNIQUE INDEX IF NOT EXISTS IX_Location_Media ON Location(
  KeySymbol,
  IssueTagNumber,
  MepsLanguage,
  DocumentId,
  Track,
  Type,
  COALESCE(Specialty, ''),
  COALESCE(Edition, '')
);

CREATE INDEX IF NOT EXISTS IX_Location_MepsLanguage_DocumentId
  ON Location(MepsLanguage, DocumentId);

CREATE TABLE IF NOT EXISTS UserMark (
  UserMarkId INTEGER PRIMARY KEY,
  ColorIndex INTEGER NOT NULL,
  LocationId INTEGER NOT NULL,
  StyleIndex INTEGER NOT NULL,
  UserMarkGuid TEXT NOT NULL UNIQUE,
  Version INTEGER NOT NULL,
  FOREIGN KEY (LocationId) REFERENCES Location(LocationId)
);

CREATE TABLE IF NOT EXISTS BlockRange (
  BlockRangeId INTEGER PRIMARY KEY,
  BlockType INTEGER NOT NULL,
  Identifier INTEGER NOT NULL,
  StartToken INTEGER,
  EndToken INTEGER,
  UserMarkId INTEGER NOT NULL,
  FOREIGN KEY (UserMarkId) REFERENCES UserMark(UserMarkId)
);

CREATE TABLE IF NOT EXISTS Note (
  NoteId INTEGER PRIMARY KEY,
  Guid TEXT NOT NULL UNIQUE,
  UserMarkId INTEGER,
  LocationId INTEGER,
  Title TEXT,
  Content TEXT,
  LastModified TEXT NOT NULL,
  Created TEXT NOT NULL,
  BlockType INTEGER NOT NULL DEFAULT 0,
  BlockIdentifier INTEGER,
  FOREIGN KEY (UserMarkId) REFERENCES UserMark(UserMarkId),
  FOREIGN KEY (LocationId) REFERENCES Location(LocationId)
);

CREATE TABLE IF NOT EXISTS InputField (
  LocationId INTEGER NOT NULL,
  TextTag TEXT NOT NULL,
  Value TEXT NOT NULL,
  PRIMARY KEY (LocationId, TextTag),
  FOREIGN KEY (LocationId) REFERENCES Location(LocationId)
);

CREATE TABLE IF NOT EXISTS Bookmark (
  BookmarkId INTEGER PRIMARY KEY,
  LocationId INTEGER NOT NULL,
  PublicationLocationId INTEGER NOT NULL,
  Slot INTEGER NOT NULL,
  Title TEXT NOT NULL,
  Snippet TEXT,
  BlockType INTEGER NOT NULL DEFAULT 0,
  BlockIdentifier INTEGER
);

CREATE TABLE IF NOT EXISTS Tag (
  TagId INTEGER PRIMARY KEY,
  Type INTEGER NOT NULL,
  Name TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS TagMap (
  TagMapId INTEGER PRIMARY KEY,
  PlaylistItemId INTEGER,
  LocationId INTEGER,
  NoteId INTEGER,
  TagId INTEGER NOT NULL,
  Position INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS IndependentMedia (
  IndependentMediaId INTEGER PRIMARY KEY,
  OriginalFilename TEXT NOT NULL,
  FilePath TEXT NOT NULL UNIQUE,
  MimeType TEXT NOT NULL,
  Hash TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS PlaylistItem (
  PlaylistItemId INTEGER PRIMARY KEY,
  Label TEXT NOT NULL,
  StartTrimOffsetTicks INTEGER,
  EndTrimOffsetTicks INTEGER,
  Accuracy INTEGER,
  EndAction INTEGER NOT NULL,
  ThumbnailFilePath TEXT
);

CREATE TABLE IF NOT EXISTS PlaylistItemAccuracy (
  PlaylistItemAccuracyId INTEGER PRIMARY KEY,
  Description TEXT NOT NULL UNIQUE
);

CREATE TABLE IF NOT EXISTS PlaylistItemIndependentMediaMap (
  PlaylistItemId INTEGER NOT NULL,
  IndependentMediaId INTEGER NOT NULL,
  DurationTicks INTEGER NOT NULL,
  PRIMARY KEY (PlaylistItemId, IndependentMediaId)
);

CREATE TABLE IF NOT EXISTS PlaylistItemLocationMap (
  PlaylistItemId INTEGER NOT NULL,
  LocationId INTEGER NOT NULL,
  MajorMultimediaType INTEGER NOT NULL,
  BaseDurationTicks INTEGER,
  PRIMARY KEY (PlaylistItemId, LocationId)
);

CREATE TABLE IF NOT EXISTS PlaylistItemMarker (
  PlaylistItemMarkerId INTEGER PRIMARY KEY,
  PlaylistItemId INTEGER NOT NULL,
  Label TEXT NOT NULL,
  StartTimeTicks INTEGER NOT NULL,
  DurationTicks INTEGER NOT NULL,
  EndTransitionDurationTicks INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS PlaylistItemMarkerBibleVerseMap (
  PlaylistItemMarkerId INTEGER NOT NULL,
  VerseId INTEGER NOT NULL,
  PRIMARY KEY (PlaylistItemMarkerId, VerseId)
);

CREATE TABLE IF NOT EXISTS PlaylistItemMarkerParagraphMap (
  PlaylistItemMarkerId INTEGER NOT NULL,
  MepsDocumentId INTEGER NOT NULL,
  ParagraphIndex INTEGER NOT NULL,
  MarkerIndexWithinParagraph INTEGER NOT NULL,
  PRIMARY KEY (PlaylistItemMarkerId, MepsDocumentId, ParagraphIndex)
);

CREATE TABLE IF NOT EXISTS grdb_migrations (
  identifier TEXT NOT NULL PRIMARY KEY
);

INSERT OR IGNORE INTO grdb_migrations (identifier) VALUES ('v15'), ('v16');
"#;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum LocationKey {
    Doc(i64),
    DocInput(i64),
    Bible {
        meps_lang: i64,
        key_symbol: String,
        book: i64,
        chapter: i64,
    },
}

fn utc_now_timestamps() -> (String, String, String) {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    // Civil date from Unix days (Howard Hinnant's algorithm)
    let days = secs.div_euclid(86_400);
    let time_of_day = secs.rem_euclid(86_400);
    let hour = time_of_day / 3600;
    let minute = (time_of_day % 3600) / 60;
    let second = time_of_day % 60;

    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = y + if m <= 2 { 1 } else { 0 };

    let date = format!("{year:04}-{m:02}-{d:02}");
    let iso_z = format!("{date}T{hour:02}:{minute:02}:{second:02}Z");
    let iso_offset = format!("{date}T{hour:02}:{minute:02}:{second:02}+0000");
    (iso_z, iso_offset, date)
}

fn parse_bible_key(key: &str) -> Option<(i64, String, i64, i64)> {
    let mut parts = key.split('|');
    let meps_lang = parts.next()?.parse::<i64>().ok()?;
    let key_symbol = parts.next()?.trim().to_string();
    let book = parts.next()?.parse::<i64>().ok()?;
    let chapter = parts.next()?.parse::<i64>().ok()?;
    if key_symbol.is_empty() || book <= 0 || chapter <= 0 {
        return None;
    }
    Some((meps_lang, key_symbol, book, chapter))
}

struct DocMeta {
    key_symbol: Option<String>,
    issue_tag_number: i64,
    meps_language: Option<i64>,
    title: Option<String>,
}

fn extract_doc_meta(marks: &Value) -> HashMap<i64, DocMeta> {
    let mut map = HashMap::new();
    let Some(obj) = marks.get("docMeta").and_then(|v| v.as_object()) else {
        return map;
    };
    for (doc_id_str, meta_val) in obj {
        let Ok(doc_id) = doc_id_str.parse::<i64>() else {
            continue;
        };
        let Some(meta) = meta_val.as_object() else {
            continue;
        };
        let key_symbol = meta
            .get("keySymbol")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from);
        let issue_tag_number = meta
            .get("issueTagNumber")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let meps_language = meta.get("mepsLanguage").and_then(|v| v.as_i64());
        let title = meta
            .get("title")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from);
        map.insert(
            doc_id,
            DocMeta {
                key_symbol,
                issue_tag_number,
                meps_language,
                title,
            },
        );
    }
    map
}

fn get_or_insert_location(
    conn: &Connection,
    cache: &mut HashMap<LocationKey, i64>,
    doc_meta: &HashMap<i64, DocMeta>,
    key: LocationKey,
    default_meps_lang: i64,
) -> Result<i64, String> {
    if let Some(&loc_id) = cache.get(&key) {
        return Ok(loc_id);
    }

    match &key {
        LocationKey::Doc(doc_id) => {
            let meta = doc_meta.get(doc_id);
            let key_symbol = meta.and_then(|m| m.key_symbol.as_deref());
            let issue_tag = meta.map(|m| m.issue_tag_number).unwrap_or(0);
            let meps_lang = meta
                .and_then(|m| m.meps_language)
                .unwrap_or(default_meps_lang);
            let title = meta.and_then(|m| m.title.as_deref());

            conn.execute(
                "INSERT INTO Location (DocumentId, IssueTagNumber, KeySymbol, MepsLanguage, Type, Title) VALUES (?1, ?2, ?3, ?4, 0, ?5)",
                params![doc_id, issue_tag, key_symbol, meps_lang, title],
            )
            .map_err(|e| format!("Failed to insert doc Location: {e}"))?;
        }
        LocationKey::DocInput(doc_id) => {
            let meta = doc_meta.get(doc_id);
            let key_symbol = meta.and_then(|m| m.key_symbol.as_deref());
            let issue_tag = meta.map(|m| m.issue_tag_number).unwrap_or(0);

            conn.execute(
                "INSERT INTO Location (DocumentId, IssueTagNumber, KeySymbol, MepsLanguage, Type) VALUES (?1, ?2, ?3, NULL, 0)",
                params![doc_id, issue_tag, key_symbol],
            )
            .map_err(|e| format!("Failed to insert doc InputField Location: {e}"))?;
        }
        LocationKey::Bible {
            meps_lang,
            key_symbol,
            book,
            chapter,
        } => {
            conn.execute(
                "INSERT INTO Location (BookNumber, ChapterNumber, IssueTagNumber, KeySymbol, MepsLanguage, Type) VALUES (?1, ?2, 0, ?3, ?4, 0)",
                params![book, chapter, key_symbol, meps_lang],
            )
            .map_err(|e| format!("Failed to insert bible Location: {e}"))?;
        }
    }

    let loc_id = conn.last_insert_rowid();
    cache.insert(key, loc_id);
    Ok(loc_id)
}

fn insert_highlights(
    conn: &Connection,
    loc_id: i64,
    block_type: i64,
    identifier: i64,
    spans: &[Value],
) -> Result<(), String> {
    for span in spans {
        let Some(arr) = span.as_array() else {
            continue;
        };
        if arr.len() < 3 {
            continue;
        }
        let Some(start) = arr[0].as_i64() else {
            continue;
        };
        let Some(end) = arr[1].as_i64() else {
            continue;
        };
        let color = arr[2].as_i64().unwrap_or(1).clamp(1, 6);
        if end < start {
            continue;
        }

        let guid = Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO UserMark (ColorIndex, LocationId, StyleIndex, UserMarkGuid, Version) VALUES (?1, ?2, 0, ?3, 1)",
            params![color, loc_id, guid],
        )
        .map_err(|e| format!("Failed to insert UserMark: {e}"))?;
        let user_mark_id = conn.last_insert_rowid();

        conn.execute(
            "INSERT INTO BlockRange (BlockType, Identifier, StartToken, EndToken, UserMarkId) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![block_type, identifier, start, end, user_mark_id],
        )
        .map_err(|e| format!("Failed to insert BlockRange: {e}"))?;
    }
    Ok(())
}

fn insert_notes(
    conn: &Connection,
    loc_id: i64,
    default_block_type: i64,
    default_identifier: i64,
    items: &[Value],
    now_iso: &str,
) -> Result<(), String> {
    for item in items {
        let Some(obj) = item.as_object() else {
            continue;
        };
        let guid = obj
            .get("guid")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let title = obj.get("title").and_then(|v| v.as_str());
        let content = obj.get("content").and_then(|v| v.as_str()).unwrap_or("");
        let last_modified = obj
            .get("lastModified")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or(now_iso);
        let created = obj
            .get("created")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or(now_iso);
        let block_type = obj
            .get("blockType")
            .and_then(|v| v.as_i64())
            .unwrap_or(default_block_type);
        let block_identifier = obj
            .get("blockIdentifier")
            .and_then(|v| v.as_i64())
            .unwrap_or(default_identifier);

        conn.execute(
            "INSERT OR IGNORE INTO Note (Guid, UserMarkId, LocationId, Title, Content, LastModified, Created, BlockType, BlockIdentifier) VALUES (?1, NULL, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                guid,
                loc_id,
                title,
                content,
                last_modified,
                created,
                block_type,
                block_identifier
            ],
        )
        .map_err(|e| format!("Failed to insert Note: {e}"))?;
    }
    Ok(())
}

fn build_user_data_db_bytes(marks: &Value, default_meps_lang: i64, now_iso: &str) -> Result<Vec<u8>, String> {
    let temp_db_path = std::env::temp_dir().join(format!("offlinewol_export_{}.db", Uuid::new_v4()));
    if temp_db_path.exists() {
        let _ = std::fs::remove_file(&temp_db_path);
    }

    let build_res = (|| -> Result<Vec<u8>, String> {
        let mut conn = Connection::open(&temp_db_path)
            .map_err(|e| format!("Failed to create temporary SQLite database: {e}"))?;
        conn.execute_batch(SCHEMA_SQL)
            .map_err(|e| format!("Failed to initialize JW Library schema: {e}"))?;

        let tx = conn
            .transaction()
            .map_err(|e| format!("Failed to start SQLite transaction: {e}"))?;

        tx.execute(
            "INSERT INTO LastModified (LastModified) VALUES (?1)",
            params![now_iso],
        )
        .map_err(|e| format!("Failed to insert LastModified: {e}"))?;

        let mut loc_cache: HashMap<LocationKey, i64> = HashMap::new();
        let doc_meta = extract_doc_meta(marks);

        // 1. Document highlights
        if let Some(docs) = marks.get("docs").and_then(|v| v.as_object()) {
            for (doc_id_str, pids_val) in docs {
                let Ok(doc_id) = doc_id_str.parse::<i64>() else {
                    continue;
                };
                if doc_id <= 0 {
                    continue;
                }
                let Some(pids) = pids_val.as_object() else {
                    continue;
                };
                let loc_id = get_or_insert_location(
                    &tx,
                    &mut loc_cache,
                    &doc_meta,
                    LocationKey::Doc(doc_id),
                    default_meps_lang,
                )?;
                for (pid_str, spans_val) in pids {
                    let Ok(pid) = pid_str.parse::<i64>() else {
                        continue;
                    };
                    if let Some(spans) = spans_val.as_array() {
                        insert_highlights(&tx, loc_id, 1, pid, spans)?;
                    }
                }
            }
        }

        // 2. Bible highlights
        if let Some(bible) = marks.get("bible").and_then(|v| v.as_object()) {
            for (key_str, verses_val) in bible {
                let Some((meps_lang, key_symbol, book, chapter)) = parse_bible_key(key_str) else {
                    continue;
                };
                let Some(verses) = verses_val.as_object() else {
                    continue;
                };
                let loc_id = get_or_insert_location(
                    &tx,
                    &mut loc_cache,
                    &doc_meta,
                    LocationKey::Bible {
                        meps_lang,
                        key_symbol,
                        book,
                        chapter,
                    },
                    default_meps_lang,
                )?;
                for (verse_str, spans_val) in verses {
                    let Ok(verse) = verse_str.parse::<i64>() else {
                        continue;
                    };
                    if let Some(spans) = spans_val.as_array() {
                        insert_highlights(&tx, loc_id, 2, verse, spans)?;
                    }
                }
            }
        }

        // 3. Notes
        if let Some(notes_obj) = marks.get("notes").and_then(|v| v.as_object()) {
            if let Some(note_docs) = notes_obj.get("docs").and_then(|v| v.as_object()) {
                for (doc_id_str, pids_val) in note_docs {
                    let Ok(doc_id) = doc_id_str.parse::<i64>() else {
                        continue;
                    };
                    if doc_id <= 0 {
                        continue;
                    }
                    let Some(pids) = pids_val.as_object() else {
                        continue;
                    };
                    let loc_id = get_or_insert_location(
                        &tx,
                        &mut loc_cache,
                        &doc_meta,
                        LocationKey::Doc(doc_id),
                        default_meps_lang,
                    )?;
                    for (pid_str, items_val) in pids {
                        let Ok(pid) = pid_str.parse::<i64>() else {
                            continue;
                        };
                        if let Some(items) = items_val.as_array() {
                            insert_notes(&tx, loc_id, 1, pid, items, now_iso)?;
                        }
                    }
                }
            }

            if let Some(note_bible) = notes_obj.get("bible").and_then(|v| v.as_object()) {
                for (key_str, verses_val) in note_bible {
                    let Some((meps_lang, key_symbol, book, chapter)) = parse_bible_key(key_str) else {
                        continue;
                    };
                    let Some(verses) = verses_val.as_object() else {
                        continue;
                    };
                    let loc_id = get_or_insert_location(
                        &tx,
                        &mut loc_cache,
                        &doc_meta,
                        LocationKey::Bible {
                            meps_lang,
                            key_symbol,
                            book,
                            chapter,
                        },
                        default_meps_lang,
                    )?;
                    for (verse_str, items_val) in verses {
                        let Ok(verse) = verse_str.parse::<i64>() else {
                            continue;
                        };
                        if let Some(items) = items_val.as_array() {
                            insert_notes(&tx, loc_id, 2, verse, items, now_iso)?;
                        }
                    }
                }
            }
        }

        // 4. InputFields
        if let Some(input_obj) = marks.get("inputFields").and_then(|v| v.as_object()) {
            if let Some(input_docs) = input_obj.get("docs").and_then(|v| v.as_object()) {
                for (doc_id_str, tags_val) in input_docs {
                    let Ok(doc_id) = doc_id_str.parse::<i64>() else {
                        continue;
                    };
                    if doc_id <= 0 {
                        continue;
                    }
                    let Some(tags) = tags_val.as_object() else {
                        continue;
                    };
                    let loc_id = get_or_insert_location(
                        &tx,
                        &mut loc_cache,
                        &doc_meta,
                        LocationKey::DocInput(doc_id),
                        default_meps_lang,
                    )?;
                    for (tag, val) in tags {
                        if let Some(text) = val.as_str() {
                            tx.execute(
                                "INSERT OR REPLACE INTO InputField (LocationId, TextTag, Value) VALUES (?1, ?2, ?3)",
                                params![loc_id, tag, text],
                            )
                            .map_err(|e| format!("Failed to insert InputField: {e}"))?;
                        }
                    }
                }
            }

            if let Some(input_bible) = input_obj.get("bible").and_then(|v| v.as_object()) {
                for (key_str, tags_val) in input_bible {
                    let Some((meps_lang, key_symbol, book, chapter)) = parse_bible_key(key_str) else {
                        continue;
                    };
                    let Some(tags) = tags_val.as_object() else {
                        continue;
                    };
                    let loc_id = get_or_insert_location(
                        &tx,
                        &mut loc_cache,
                        &doc_meta,
                        LocationKey::Bible {
                            meps_lang,
                            key_symbol,
                            book,
                            chapter,
                        },
                        default_meps_lang,
                    )?;
                    for (tag, val) in tags {
                        if let Some(text) = val.as_str() {
                            tx.execute(
                                "INSERT OR REPLACE INTO InputField (LocationId, TextTag, Value) VALUES (?1, ?2, ?3)",
                                params![loc_id, tag, text],
                            )
                            .map_err(|e| format!("Failed to insert InputField: {e}"))?;
                        }
                    }
                }
            }
        }

        tx.commit()
            .map_err(|e| format!("Failed to commit SQLite transaction: {e}"))?;
        drop(conn);

        std::fs::read(&temp_db_path).map_err(|e| format!("Failed to read temporary SQLite database: {e}"))
    })();

    let _ = std::fs::remove_file(&temp_db_path);
    build_res
}

fn write_jwlibrary_zip<W: Write + Seek>(
    marks: &Value,
    backup_name: Option<&str>,
    default_meps_lang: i64,
    writer: W,
) -> Result<(), String> {
    let (now_iso_z, now_iso_offset, date_str) = utc_now_timestamps();
    let db_bytes = build_user_data_db_bytes(marks, default_meps_lang, &now_iso_z)?;

    let mut hasher = Sha256::new();
    hasher.update(&db_bytes);
    let db_hash = format!("{:x}", hasher.finalize());

    let resolved_name = backup_name
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .unwrap_or_else(|| format!("UserdataBackup_{date_str}_OfflineWOL.jwlibrary"));

    let manifest = serde_json::json!({
        "name": resolved_name,
        "creationDate": now_iso_offset,
        "version": 1,
        "type": 0,
        "userDataBackup": {
            "lastModifiedDate": now_iso_offset,
            "deviceName": "OfflineWOL",
            "databaseName": "userData.db",
            "hash": db_hash,
            "schemaVersion": SCHEMA_VERSION
        }
    });
    let manifest_bytes = serde_json::to_vec(&manifest)
        .map_err(|e| format!("Failed to serialize manifest.json: {e}"))?;

    let mut zip = ZipWriter::new(writer);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    zip.start_file("userData.db", options)
        .map_err(|e| format!("Failed to add userData.db to archive: {e}"))?;
    zip.write_all(&db_bytes)
        .map_err(|e| format!("Failed to write userData.db: {e}"))?;

    zip.start_file("manifest.json", options)
        .map_err(|e| format!("Failed to add manifest.json to archive: {e}"))?;
    zip.write_all(&manifest_bytes)
        .map_err(|e| format!("Failed to write manifest.json: {e}"))?;

    zip.finish()
        .map_err(|e| format!("Failed to finalize .jwlibrary archive: {e}"))?;

    Ok(())
}

pub fn export_marks_to_jwlibrary(
    marks: &Value,
    dest_path: &Path,
    default_meps_lang: i64,
) -> Result<(), String> {
    let backup_name = dest_path.file_name().and_then(|s| s.to_str());
    let file = File::create(dest_path)
        .map_err(|e| format!("Failed to create destination file: {e}"))?;
    write_jwlibrary_zip(marks, backup_name, default_meps_lang, file)
}

/// Build a `.jwlibrary` archive in memory (used by mobile share-sheet export).
pub fn export_marks_to_jwlibrary_bytes(
    marks: &Value,
    backup_name: Option<&str>,
    default_meps_lang: i64,
) -> Result<Vec<u8>, String> {
    let mut cursor = Cursor::new(Vec::new());
    write_jwlibrary_zip(marks, backup_name, default_meps_lang, &mut cursor)?;
    Ok(cursor.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use zip::ZipArchive;

    #[test]
    fn exports_valid_jwlibrary_archive_with_highlights_notes_and_inputs() {
        let marks = serde_json::json!({
            "version": 1,
            "highlightCount": 2,
            "noteCount": 2,
            "docs": {
                "2025441": {
                    "7": [[53, 62, 1]]
                }
            },
            "docMeta": {
                "2025441": {
                    "keySymbol": "w",
                    "issueTagNumber": 20250600,
                    "mepsLanguage": 1,
                    "title": "Sample Watchtower Article"
                }
            },
            "bible": {
                "1|nwtsty|2|5": {
                    "9": [[0, 15, 2]]
                }
            },
            "notes": {
                "docs": {
                    "2025441": {
                        "7": [{
                            "guid": "note-guid-1",
                            "title": "Study note",
                            "content": "My personal comment",
                            "blockType": 1,
                            "blockIdentifier": 7
                        }]
                    }
                },
                "bible": {}
            },
            "inputFields": {
                "docs": {
                    "2025441": {
                        "field-1": "Answer text"
                    }
                },
                "bible": {}
            }
        });

        let out_path = std::env::temp_dir().join(format!("test_export_{}.jwlibrary", Uuid::new_v4()));
        export_marks_to_jwlibrary(&marks, &out_path, 1).expect("export jwlibrary");

        let file = File::open(&out_path).expect("open exported zip");
        let mut archive = ZipArchive::new(file).expect("read zip archive");

        let mut manifest_str = String::new();
        archive
            .by_name("manifest.json")
            .expect("manifest.json exists")
            .read_to_string(&mut manifest_str)
            .expect("read manifest.json");
        let manifest: Value = serde_json::from_str(&manifest_str).expect("parse manifest");
        assert!(manifest["name"].as_str().unwrap().ends_with(".jwlibrary"));
        assert!(manifest["creationDate"].as_str().unwrap().contains('T'));
        assert_eq!(manifest["userDataBackup"]["databaseName"], "userData.db");
        assert_eq!(manifest["userDataBackup"]["schemaVersion"], 16);
        assert_eq!(manifest["userDataBackup"]["deviceName"], "OfflineWOL");

        let mut db_bytes = Vec::new();
        archive
            .by_name("userData.db")
            .expect("userData.db exists")
            .read_to_end(&mut db_bytes)
            .expect("read userData.db");

        let mut hasher = Sha256::new();
        hasher.update(&db_bytes);
        let actual_hash = format!("{:x}", hasher.finalize());
        assert_eq!(manifest["userDataBackup"]["hash"], actual_hash);

        let verify_db_path = std::env::temp_dir().join(format!("verify_{}.db", Uuid::new_v4()));
        std::fs::write(&verify_db_path, &db_bytes).expect("write temp db");
        let conn = Connection::open(&verify_db_path).expect("open temp db");

        let mark_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM UserMark", [], |r| r.get(0))
            .unwrap();
        let range_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM BlockRange", [], |r| r.get(0))
            .unwrap();
        let note_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM Note", [], |r| r.get(0))
            .unwrap();
        let input_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM InputField", [], |r| r.get(0))
            .unwrap();

        assert_eq!(mark_count, 2);
        assert_eq!(range_count, 2);
        assert_eq!(note_count, 1);
        assert_eq!(input_count, 1);

        let (key_symbol, issue_tag, meps_lang, specialty, edition): (Option<String>, i64, i64, Option<String>, Option<String>) = conn
            .query_row(
                "SELECT KeySymbol, IssueTagNumber, MepsLanguage, Specialty, Edition FROM Location WHERE DocumentId = 2025441 AND MepsLanguage IS NOT NULL",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .unwrap();
        assert_eq!(key_symbol.as_deref(), Some("w"));
        assert_eq!(issue_tag, 20250600);
        assert_eq!(meps_lang, 1);
        assert_eq!(specialty, None);
        assert_eq!(edition, None);

        let input_loc_lang: Option<i64> = conn
            .query_row(
                "SELECT l.MepsLanguage FROM InputField i JOIN Location l ON l.LocationId = i.LocationId WHERE i.TextTag = 'field-1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(input_loc_lang, None);

        let user_version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(user_version, 16);

        let migration_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM grdb_migrations WHERE identifier IN ('v15', 'v16')", [], |r| r.get(0))
            .unwrap();
        assert_eq!(migration_count, 2);

        let _ = std::fs::remove_file(&verify_db_path);
        let _ = std::fs::remove_file(&out_path);
    }

    #[test]
    fn exports_v16_manifest_and_zip_order_matching_jw_library() {
        let marks = serde_json::json!({
            "version": 1,
            "docs": {
                "202026402": {
                    "8": [[0, 2, 5]]
                }
            },
            "docMeta": {
                "202026402": {
                    "keySymbol": "mwb",
                    "issueTagNumber": 20261100,
                    "mepsLanguage": 1,
                    "title": "9-15 de noviembre"
                }
            }
        });

        let out_path = std::env::temp_dir().join(format!("test_v16_{}.jwlibrary", Uuid::new_v4()));
        export_marks_to_jwlibrary(&marks, &out_path, 1).expect("export jwlibrary");

        let file = File::open(&out_path).expect("open zip");
        let mut archive = ZipArchive::new(file).expect("read zip");
        assert_eq!(archive.len(), 2);
        assert_eq!(archive.by_index(0).unwrap().name(), "userData.db");
        assert_eq!(archive.by_index(1).unwrap().name(), "manifest.json");

        let mut manifest_str = String::new();
        archive
            .by_name("manifest.json")
            .unwrap()
            .read_to_string(&mut manifest_str)
            .unwrap();
        let manifest: Value = serde_json::from_str(&manifest_str).unwrap();
        let creation = manifest["creationDate"].as_str().unwrap();
        let modified = manifest["userDataBackup"]["lastModifiedDate"].as_str().unwrap();
        assert!(creation.ends_with("+0000"), "creationDate={creation}");
        assert!(modified.ends_with("+0000"), "lastModifiedDate={modified}");
        assert_eq!(manifest["userDataBackup"]["schemaVersion"], 16);

        let _ = std::fs::remove_file(&out_path);
    }

    #[test]
    fn separates_input_field_location_with_null_meps_language() {
        let marks = serde_json::json!({
            "version": 1,
            "docs": {
                "202026402": {
                    "8": [[0, 2, 5]]
                }
            },
            "docMeta": {
                "202026402": {
                    "keySymbol": "mwb",
                    "issueTagNumber": 20261100,
                    "mepsLanguage": 1
                }
            },
            "inputFields": {
                "docs": {
                    "202026402": {
                        "tt21": "Test3",
                        "tt26": "Test4"
                    }
                },
                "bible": {}
            }
        });

        let out_path = std::env::temp_dir().join(format!("test_inputs_{}.jwlibrary", Uuid::new_v4()));
        export_marks_to_jwlibrary(&marks, &out_path, 1).expect("export jwlibrary");

        let file = File::open(&out_path).expect("open zip");
        let mut archive = ZipArchive::new(file).expect("read zip");
        let mut db_bytes = Vec::new();
        archive.by_name("userData.db").unwrap().read_to_end(&mut db_bytes).unwrap();

        let verify_db_path = std::env::temp_dir().join(format!("verify_inputs_{}.db", Uuid::new_v4()));
        std::fs::write(&verify_db_path, &db_bytes).unwrap();
        let conn = Connection::open(&verify_db_path).unwrap();

        let note_count: i64 = conn.query_row("SELECT COUNT(*) FROM Note", [], |r| r.get(0)).unwrap();
        assert_eq!(note_count, 0, "InputField must not create Note rows");

        let input_count: i64 = conn.query_row("SELECT COUNT(*) FROM InputField", [], |r| r.get(0)).unwrap();
        assert_eq!(input_count, 2);

        let loc_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM Location WHERE DocumentId = 202026402", [], |r| r.get(0))
            .unwrap();
        assert_eq!(loc_count, 2, "UserMark uses MepsLanguage=1, InputField uses MepsLanguage=NULL");

        let _ = std::fs::remove_file(&verify_db_path);
        let _ = std::fs::remove_file(&out_path);
    }

    #[test]
    fn skips_invalid_zero_doc_and_bible_keys_without_violating_check_constraints() {
        let marks = serde_json::json!({
            "version": 1,
            "docs": {
                "0": { "1": [[0, 1, 1]] },
                "-5": { "1": [[0, 1, 1]] }
            },
            "bible": {
                "1|nwtsty|0|5": { "1": [[0, 1, 1]] },
                "1|nwtsty|1|0": { "1": [[0, 1, 1]] }
            }
        });

        let out_path = std::env::temp_dir().join(format!("test_invalid_keys_{}.jwlibrary", Uuid::new_v4()));
        export_marks_to_jwlibrary(&marks, &out_path, 1).expect("invalid keys should be skipped cleanly");
        let _ = std::fs::remove_file(&out_path);
    }

    #[test]
    fn exports_valid_jwlibrary_archive_in_memory_bytes() {
        let marks = serde_json::json!({
            "version": 1,
            "docs": { "2025441": { "7": [[53, 62, 1]] } }
        });

        let bytes = export_marks_to_jwlibrary_bytes(&marks, Some("MobileBackup.jwlibrary"), 0)
            .expect("in-memory export");
        let mut archive = ZipArchive::new(Cursor::new(bytes)).expect("read zip bytes");
        assert!(archive.by_name("userData.db").is_ok());
        let mut manifest = String::new();
        archive.by_name("manifest.json").unwrap().read_to_string(&mut manifest).unwrap();
        assert!(manifest.contains("MobileBackup.jwlibrary"));
    }
}
