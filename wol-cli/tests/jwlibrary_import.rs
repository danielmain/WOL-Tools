//! End-to-end coverage for the CLI front-end of `.jwlibrary` import/export.
//!
//! The shared parsers are covered elsewhere (`wol-core/src/backup/tests.rs`,
//! `wol-highlights/test/backup.test.mjs`), but `wol-cli/src/commands.rs` had no
//! tests at all, and `wol-highlights/test/cli-export.test.mjs` *skips itself*
//! when no `wol` binary has been built. These tests always run: they drive the
//! real binary against a real zip, a real SQLite database and a scratch
//! `--data-dir`, which is exactly the path a user hits with `wol import`.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use rusqlite::Connection;
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

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

const SEED: &str = "
INSERT INTO Location VALUES (1, NULL, NULL, 1102026210, 2025441, 'w', 0, 0, 'Watchtower Study');
INSERT INTO Location VALUES (2, NULL, NULL, 1102026210, 2025441, 'w', 0, 0, 'Watchtower Study');
INSERT INTO UserMark VALUES (11, 1, 1, 0);
INSERT INTO UserMark VALUES (12, 4, 2, 0);
INSERT INTO BlockRange VALUES (101, 1, '7', 3, 9, 11);
INSERT INTO BlockRange VALUES (102, 1, '1', 0, 2, 12);
INSERT INTO Note VALUES (201, 'guid-a', 11, 1, 'Title', 'Body text', '2024-01-01T00:00:00Z', '2024-01-01T00:00:00Z', 1, 7);
INSERT INTO InputField VALUES (1, '7', 'my answer');
";

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "wol-cli-import-{tag}-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

fn write_user_data_db(path: &Path) {
    let connection = Connection::open(path).expect("create userData.db");
    connection.execute_batch(SCHEMA).expect("apply schema");
    connection.execute_batch(SEED).expect("seed rows");
}

/// A minimal but structurally real `.jwlibrary` archive.
fn write_jwlibrary_zip(dir: &Path) -> PathBuf {
    let zip_path = dir.join("backup.jwlibrary");
    let db_path = dir.join("userData.db");
    write_user_data_db(&db_path);

    let file = std::fs::File::create(&zip_path).expect("create zip");
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    zip.start_file("userData.db", options).expect("start userData.db");
    let db_bytes = std::fs::read(&db_path).expect("read userData.db");
    std::io::Write::write_all(&mut zip, &db_bytes).expect("write userData.db");
    zip.start_file("manifest.json", options).expect("start manifest");
    std::io::Write::write_all(&mut zip, br#"{"version":1,"type":0}"#).expect("write manifest");
    zip.finish().expect("finish zip");
    std::fs::remove_file(&db_path).expect("drop staging db");
    zip_path
}

fn wol(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_wol"))
        .args(["--no-color", "--ascii"])
        .args(args)
        .output()
        .expect("run the wol binary")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

#[test]
fn import_then_export_round_trips_a_jwlibrary_backup() {
    let dir = scratch_dir("roundtrip");
    let zip = write_jwlibrary_zip(&dir);
    let data_dir = dir.join("data");
    let data_dir_arg = data_dir.to_str().expect("utf-8 data dir");
    let zip_arg = zip.to_str().expect("utf-8 zip path");

    let imported = wol(&["--offline", "--data-dir", data_dir_arg, "import", zip_arg]);
    assert!(
        imported.status.success(),
        "import failed: {}\n{}",
        stderr(&imported),
        stdout(&imported)
    );
    assert!(
        stdout(&imported).contains("highlights") && stdout(&imported).contains("replaced"),
        "import should report a replaced-mode summary: {}",
        stdout(&imported)
    );

    // The CLI must write the shared state file the desktop app also reads.
    let state = std::fs::read_to_string(data_dir.join("state.json")).expect("state.json written");
    assert!(state.contains("wolMarks"), "state.json should carry marks: {state}");

    let exported = dir.join("marks.json");
    let export = wol(&[
        "--offline",
        "--data-dir",
        data_dir_arg,
        "export",
        exported.to_str().expect("utf-8 export path"),
        "--format",
        "json",
    ]);
    assert!(
        export.status.success(),
        "export failed: {}\n{}",
        stderr(&export),
        stdout(&export)
    );

    let value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&exported).expect("read export")).expect("parse export");
    assert_eq!(value["highlightCount"], 2);
    assert_eq!(value["noteCount"], 2, "one paragraph note + one input field");
    assert_eq!(value["docs"]["1102026210"]["7"], serde_json::json!([[3, 9, 1]]));
    assert_eq!(value["docs"]["1102026210"]["1"], serde_json::json!([[0, 2, 4]]));
    assert_eq!(
        value["notes"]["docs"]["1102026210"]["7"][0]["content"],
        "Body text"
    );
    assert_eq!(value["inputFields"]["docs"]["1102026210"]["7"], "my answer");

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn repeated_merge_import_does_not_duplicate_marks() {
    let dir = scratch_dir("merge");
    let zip = write_jwlibrary_zip(&dir);
    let data_dir = dir.join("data");
    let data_dir_arg = data_dir.to_str().expect("utf-8 data dir");
    let zip_arg = zip.to_str().expect("utf-8 zip path");

    for _ in 0..2 {
        let output = wol(&[
            "--offline",
            "--data-dir",
            data_dir_arg,
            "import",
            zip_arg,
            "--merge",
        ]);
        assert!(output.status.success(), "merge import failed: {}", stderr(&output));
    }

    let exported = dir.join("merged.json");
    let export = wol(&[
        "--offline",
        "--data-dir",
        data_dir_arg,
        "export",
        exported.to_str().expect("utf-8 export path"),
        "--format",
        "json",
    ]);
    assert!(export.status.success(), "export failed: {}", stderr(&export));

    let value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&exported).expect("read export")).expect("parse export");
    assert_eq!(value["highlightCount"], 2, "merging the same backup twice must be idempotent");
    assert_eq!(value["docs"]["1102026210"]["7"], serde_json::json!([[3, 9, 1]]));

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn import_of_a_zip_without_user_data_db_fails_loudly() {
    let dir = scratch_dir("invalid");
    let zip_path = dir.join("not-a-backup.jwlibrary");
    let file = std::fs::File::create(&zip_path).expect("create zip");
    let mut zip = ZipWriter::new(file);
    zip.start_file("manifest.json", SimpleFileOptions::default())
        .expect("start manifest");
    std::io::Write::write_all(&mut zip, b"{}").expect("write manifest");
    zip.finish().expect("finish zip");

    let data_dir = dir.join("data");
    let output = wol(&[
        "--offline",
        "--data-dir",
        data_dir.to_str().expect("utf-8 data dir"),
        "import",
        zip_path.to_str().expect("utf-8 zip path"),
    ]);

    assert!(!output.status.success(), "a backup without userData.db must not succeed");
    let message = format!("{}{}", stdout(&output), stderr(&output));
    assert!(
        message.to_lowercase().contains("userdata.db"),
        "error should name the missing file: {message}"
    );
    assert!(
        !data_dir.join("state.json").exists() || !std::fs::read_to_string(data_dir.join("state.json"))
            .unwrap_or_default()
            .contains("1102026210"),
        "a failed import must not overwrite existing marks"
    );

    std::fs::remove_dir_all(&dir).ok();
}
