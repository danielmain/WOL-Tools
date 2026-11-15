use std::io::Write;

use rusqlite::Connection;
use zip::write::SimpleFileOptions;

use super::*;

const SCHEMA: &str = "
CREATE TABLE Location (
  LocationId INTEGER PRIMARY KEY,
  BookNumber INTEGER,
  ChapterNumber INTEGER,
  DocumentId INTEGER,
  IssueTagNumber INTEGER DEFAULT 0,
  KeySymbol TEXT,
  MepsLanguage INTEGER,
  Type INTEGER,
  Title TEXT
);
CREATE TABLE UserMark (
  UserMarkId INTEGER PRIMARY KEY,
  ColorIndex INTEGER,
  LocationId INTEGER,
  StyleIndex INTEGER DEFAULT 0
);
CREATE TABLE BlockRange (
  BlockRangeId INTEGER PRIMARY KEY,
  BlockType INTEGER,
  Identifier TEXT,
  StartToken INTEGER,
  EndToken INTEGER,
  UserMarkId INTEGER
);
CREATE TABLE Note (
  NoteId INTEGER PRIMARY KEY,
  Guid TEXT,
  UserMarkId INTEGER,
  LocationId INTEGER,
  Title TEXT,
  Content TEXT,
  LastModified TEXT,
  Created TEXT,
  BlockType INTEGER,
  BlockIdentifier INTEGER
);
CREATE TABLE InputField (
  LocationId INTEGER,
  TextTag TEXT,
  Value TEXT
);
";

fn make_database(path: &std::path::Path) {
    let connection = Connection::open(path).expect("create db");
    connection.execute_batch(SCHEMA).expect("schema");
    connection
        .execute_batch(
            "
            -- Article 1102026210: two highlights on paragraph '7', one on 'title'
            INSERT INTO Location VALUES (1, NULL, NULL, 1102026210, 2025441, 'w', 0, 0, 'Watchtower Study');
            INSERT INTO Location VALUES (2, NULL, NULL, 1102026210, 2025441, 'w', 0, 0, 'Watchtower Study');
            INSERT INTO UserMark VALUES (11, 1, 1, 0);
            INSERT INTO UserMark VALUES (12, 4, 2, 0);
            INSERT INTO BlockRange VALUES (101, 1, '7', 3, 9, 11);
            INSERT INTO BlockRange VALUES (102, 1, '1', 0, 2, 12);

            -- Exodus 5:9 (book 2, chapter 5) in nwtsty/Spanish (MEPS 1)
            INSERT INTO Location VALUES (3, 2, 5, NULL, 0, 'nwtsty', 1, 0, 'Exodus');
            INSERT INTO UserMark VALUES (13, 2, 3, 0);
            INSERT INTO BlockRange VALUES (103, 2, '9', 4, 4, 13);

            -- Ignored: invalid token range and an unsupported block type
            INSERT INTO UserMark VALUES (14, 3, 1, 0);
            INSERT INTO BlockRange VALUES (104, 1, '8', 9, 3, 14);
            INSERT INTO BlockRange VALUES (105, 3, '2', 0, 1, 14);

            -- Notes: paragraph note + Bible verse note + orphan row
            INSERT INTO Note VALUES (201, 'guid-a', 11, 1, 'Title', 'Body text', '2024-01-01T00:00:00Z', '2024-01-01T00:00:00Z', 1, 7);
            INSERT INTO Note VALUES (202, 'guid-b', NULL, 3, NULL, 'Verse note', NULL, NULL, 2, 9);
            INSERT INTO Note VALUES (203, 'guid-c', NULL, NULL, NULL, 'Orphan', NULL, NULL, NULL, NULL);

            -- Input fields: paragraph text tag (filled + empty) and a chapter tag
            INSERT INTO InputField VALUES (1, '7', 'my answer');
            INSERT INTO InputField VALUES (1, '8', '');
            INSERT INTO InputField VALUES (3, 'study', 'chapter notes');
            ",
        )
        .expect("seed rows");
}

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "wol-core-backup-{tag}-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn sample_db(dir: &std::path::Path) -> std::path::PathBuf {
    let path = dir.join("userData.db");
    make_database(&path);
    path
}

#[test]
fn indexes_highlights_notes_and_input_fields() {
    let dir = temp_dir("index");
    let db = sample_db(&dir);
    let index = import_backup(&db).expect("import");

    assert_eq!(index.version, 1);
    assert_eq!(index.highlight_count, 3);
    assert!(index.ignored >= 1);
    assert_eq!(index.document_count, 1);
    assert_eq!(index.bible_chapter_count, 1);
    assert_eq!(index.color_counts.get(&1), Some(&1));
    assert_eq!(index.color_counts.get(&4), Some(&1));
    assert_eq!(index.color_counts.get(&2), Some(&1));

    assert_eq!(index.doc_ranges("1102026210", "7"), &[(3, 9, 1)]);
    assert_eq!(index.doc_ranges("1102026210", "1"), &[(0, 2, 4)]);

    let key = StoredIndex::bible_key(Some(1), "nwtsty", 2, 5);
    assert_eq!(key, "1|nwtsty|2|5");
    assert_eq!(index.bible_ranges(&key, "9"), &[(4, 4, 2)]);

    let notes = index.doc_notes("1102026210", "7");
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].guid.as_deref(), Some("guid-a"));
    assert_eq!(notes[0].content, "Body text");

    let verse_notes = index.notes.bible.get(&key).and_then(|verses| verses.get("9"));
    assert!(verse_notes.is_some_and(|list| list.len() == 1));

    assert_eq!(index.doc_input_field("1102026210", "7"), Some("my answer"));
    assert_eq!(index.doc_input_field("1102026210", "8"), Some(""));
    // noteCount mirrors backup.js: every note row + every *filled* input field
    // (2 notes + 2 filled fields). The empty field is stored but not counted.
    assert_eq!(index.note_count, 4);
    assert_eq!(index.file_name.as_deref(), Some("userData.db"));
    assert!(index.loaded_at.is_some());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn imports_jwlibrary_zip_archive() {
    let dir = temp_dir("zip");
    let db = sample_db(&dir);
    let archive_path = dir.join("backup.jwlibrary");
    {
        let file = std::fs::File::create(&archive_path).expect("create zip");
        let mut writer = zip::ZipWriter::new(file);
        let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        writer.start_file("userData.db", options).expect("start entry");
        let bytes = std::fs::read(&db).expect("read db");
        writer.write_all(&bytes).expect("write entry");
        writer.finish().expect("finish zip");
    }

    assert_eq!(detect_format(&archive_path).unwrap(), ImportFormat::ZipArchive);
    let index = import_backup(&archive_path).expect("import zip");
    assert_eq!(index.highlight_count, 3);
    assert_eq!(index.file_name.as_deref(), Some("backup.jwlibrary"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn imports_marks_json_roundtrip() {
    let dir = temp_dir("json");
    let db = sample_db(&dir);
    let original = import_backup(&db).expect("import db");
    let json_path = dir.join("marks.json");
    std::fs::write(&json_path, serde_json::to_vec(&original).unwrap()).expect("write json");

    assert_eq!(detect_format(&json_path).unwrap(), ImportFormat::MarksJson);
    let mut reloaded = import_backup(&json_path).expect("import json");
    reloaded.file_name = original.file_name.clone();
    reloaded.loaded_at = original.loaded_at.clone();
    assert_eq!(reloaded, original);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn rejects_unknown_formats() {
    let dir = temp_dir("bad");
    let path = dir.join("notes.txt");
    std::fs::write(&path, b"hello, world").unwrap();
    assert!(matches!(
        detect_format(&path),
        Err(BackupError::UnrecognizedFormat)
    ));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn zip_without_user_data_is_reported() {
    let dir = temp_dir("empty-zip");
    let path = dir.join("empty.jwlibrary");
    {
        let file = std::fs::File::create(&path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        writer
            .start_file("readme.txt", SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"nothing here").unwrap();
        writer.finish().unwrap();
    }
    assert!(matches!(
        import_backup(&path),
        Err(BackupError::MissingDatabase)
    ));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn spans_are_sorted_and_ranges_default_to_empty() {
    let index = build_index(
        &[
            HighlightRow {
                document_id: Some("42".into()),
                book_number: None,
                chapter_number: None,
                key_symbol: Some("w".into()),
                issue_tag_number: Some(2025441),
                meps_language: Some(0),
                title: Some("Doc".into()),
                block_type: Some(1),
                identifier: Some("7".into()),
                start_token: Some(9),
                end_token: Some(12),
                color_index: Some(3),
            },
            HighlightRow {
                document_id: Some("42".into()),
                book_number: None,
                chapter_number: None,
                key_symbol: Some("w".into()),
                issue_tag_number: Some(2025441),
                meps_language: Some(0),
                title: Some("Doc".into()),
                block_type: Some(1),
                identifier: Some("7".into()),
                start_token: Some(1),
                end_token: Some(4),
                color_index: Some(2),
            },
        ],
        &[],
        &[],
    );
    assert_eq!(index.doc_ranges("42", "7"), &[(1, 4, 2), (9, 12, 3)]);
    assert!(index.doc_ranges("42", "missing").is_empty());
    assert!(index.doc_notes("42", "7").is_empty());
    assert_eq!(index.total_ranges(), 2);
    assert_eq!(index.note_count, 0);
}
