use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::url_map::CanonicalUrl;

#[allow(dead_code)]
pub const DEFAULT_CACHE_CAP_BYTES: u64 = 2 * 1024 * 1024 * 1024; // 2 GB

#[derive(Debug, Error)]
pub enum CacheError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("lock poisoned")]
    LockPoisoned,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheEntry {
    pub url: CanonicalUrl,
    pub blob_path: String,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub content_type: String,
    pub size: u64,
    pub fetched_at: i64,
    pub checked_at: i64,
    pub last_used_at: i64,
}

#[derive(Clone)]
pub struct DiskCache {
    db: Arc<Mutex<Connection>>,
    blobs_dir: PathBuf,
    cap_bytes: u64,
}

pub trait CacheStore: Send + Sync {
    fn get(&self, url: &CanonicalUrl) -> Result<Option<(CacheEntry, Vec<u8>)>, CacheError>;
    fn put(
        &self,
        url: &CanonicalUrl,
        content_type: &str,
        etag: Option<&str>,
        last_modified: Option<&str>,
        body: &[u8],
    ) -> Result<CacheEntry, CacheError>;
    fn touch(&self, url: &CanonicalUrl) -> Result<(), CacheError>;
    fn total_size(&self) -> Result<u64, CacheError>;
}

impl CacheStore for DiskCache {
    fn get(&self, url: &CanonicalUrl) -> Result<Option<(CacheEntry, Vec<u8>)>, CacheError> {
        self.get(url)
    }

    fn put(
        &self,
        url: &CanonicalUrl,
        content_type: &str,
        etag: Option<&str>,
        last_modified: Option<&str>,
        body: &[u8],
    ) -> Result<CacheEntry, CacheError> {
        self.put(url, content_type, etag, last_modified, body)
    }

    fn touch(&self, url: &CanonicalUrl) -> Result<(), CacheError> {
        self.touch(url)
    }

    fn total_size(&self) -> Result<u64, CacheError> {
        self.total_size()
    }
}

impl DiskCache {
    pub fn open(cache_dir: &Path, cap_bytes: u64) -> Result<Self, CacheError> {
        fs::create_dir_all(cache_dir)?;
        let blobs_dir = cache_dir.join("blobs");
        fs::create_dir_all(&blobs_dir)?;

        let db_path = cache_dir.join("index.db");
        let conn = Connection::open(db_path)?;

        // Set up WAL mode for concurrency and create schema
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             CREATE TABLE IF NOT EXISTS entries (
                 url           TEXT PRIMARY KEY,
                 path          TEXT NOT NULL,
                 etag          TEXT,
                 last_modified TEXT,
                 content_type  TEXT NOT NULL,
                 size          INTEGER NOT NULL,
                 fetched_at    INTEGER NOT NULL,
                 checked_at    INTEGER NOT NULL,
                 last_used_at  INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_entries_last_used ON entries (last_used_at);",
        )?;

        Ok(Self {
            db: Arc::new(Mutex::new(conn)),
            blobs_dir,
            cap_bytes,
        })
    }

    pub fn get(&self, url: &CanonicalUrl) -> Result<Option<(CacheEntry, Vec<u8>)>, CacheError> {
        let now = now_epoch_secs();
        let db = self.db.lock().map_err(|_| CacheError::LockPoisoned)?;

        let entry = db
            .query_row(
                "SELECT url, path, etag, last_modified, content_type, size, fetched_at, checked_at, last_used_at
                 FROM entries WHERE url = ?1",
                params![url.as_str()],
                |row| {
                    let url_str: String = row.get(0)?;
                    let size_signed: i64 = row.get(5)?;
                    let size = if size_signed > 0 { size_signed as u64 } else { 0 };
                    Ok(CacheEntry {
                        url: CanonicalUrl::new(url_str),
                        blob_path: row.get(1)?,
                        etag: row.get(2)?,
                        last_modified: row.get(3)?,
                        content_type: row.get(4)?,
                        size,
                        fetched_at: row.get(6)?,
                        checked_at: row.get(7)?,
                        last_used_at: row.get(8)?,
                    })
                },
            )
            .optional()?;

        let mut entry = match entry {
            Some(e) => e,
            None => return Ok(None),
        };

        let file_path = self.blobs_dir.join(&entry.blob_path);
        let bytes = match fs::read(&file_path) {
            Ok(b) => b,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                // Stale DB row pointing to missing file: remove row
                let _ = db.execute("DELETE FROM entries WHERE url = ?1", params![url.as_str()]);
                return Ok(None);
            }
            Err(err) => return Err(CacheError::Io(err)),
        };

        // Update last_used_at for LRU
        let _ = db.execute(
            "UPDATE entries SET last_used_at = ?1 WHERE url = ?2",
            params![now, url.as_str()],
        );
        entry.last_used_at = now;

        Ok(Some((entry, bytes)))
    }

    pub fn put(
        &self,
        url: &CanonicalUrl,
        content_type: &str,
        etag: Option<&str>,
        last_modified: Option<&str>,
        body: &[u8],
    ) -> Result<CacheEntry, CacheError> {
        let blob_name = hash_url_sha256(url.as_str());
        let final_path = self.blobs_dir.join(&blob_name);
        let tmp_path = self.blobs_dir.join(format!("{blob_name}.tmp.{}", std::process::id()));

        // Atomic write: write to .tmp then rename
        {
            let mut file = File::create(&tmp_path)?;
            file.write_all(body)?;
            file.sync_all()?;
        }
        fs::rename(&tmp_path, &final_path)?;

        let now = now_epoch_secs();
        let size_signed = i64::try_from(body.len()).unwrap_or(i64::MAX);

        let entry = CacheEntry {
            url: url.clone(),
            blob_path: blob_name.clone(),
            etag: etag.map(str::to_string),
            last_modified: last_modified.map(str::to_string),
            content_type: content_type.to_string(),
            size: body.len() as u64,
            fetched_at: now,
            checked_at: now,
            last_used_at: now,
        };

        {
            let db = self.db.lock().map_err(|_| CacheError::LockPoisoned)?;
            db.execute(
                "INSERT INTO entries (url, path, etag, last_modified, content_type, size, fetched_at, checked_at, last_used_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(url) DO UPDATE SET
                   path = excluded.path,
                   etag = excluded.etag,
                   last_modified = excluded.last_modified,
                   content_type = excluded.content_type,
                   size = excluded.size,
                   fetched_at = excluded.fetched_at,
                   checked_at = excluded.checked_at,
                   last_used_at = excluded.last_used_at",
                params![
                    url.as_str(),
                    blob_name,
                    etag,
                    last_modified,
                    content_type,
                    size_signed,
                    now,
                    now,
                    now,
                ],
            )?;
        }

        self.evict_if_over_cap()?;

        Ok(entry)
    }

    pub fn touch(&self, url: &CanonicalUrl) -> Result<(), CacheError> {
        let now = now_epoch_secs();
        let db = self.db.lock().map_err(|_| CacheError::LockPoisoned)?;
        db.execute(
            "UPDATE entries SET checked_at = ?1, last_used_at = ?1 WHERE url = ?2",
            params![now, url.as_str()],
        )?;
        Ok(())
    }

    pub fn total_size(&self) -> Result<u64, CacheError> {
        let db = self.db.lock().map_err(|_| CacheError::LockPoisoned)?;
        let total: i64 = db.query_row("SELECT COALESCE(SUM(size), 0) FROM entries", [], |row| {
            row.get(0)
        })?;
        Ok(total.max(0) as u64)
    }

    fn evict_if_over_cap(&self) -> Result<(), CacheError> {
        let mut total = self.total_size()?;
        if total <= self.cap_bytes {
            return Ok(());
        }

        let db = self.db.lock().map_err(|_| CacheError::LockPoisoned)?;
        while total > self.cap_bytes {
            // Find oldest by last_used_at
            let row: Option<(String, String, i64)> = db
                .query_row(
                    "SELECT url, path, size FROM entries ORDER BY last_used_at ASC LIMIT 1",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?;

            match row {
                Some((url, path, size)) => {
                    db.execute("DELETE FROM entries WHERE url = ?1", params![url])?;
                    let blob_file = self.blobs_dir.join(&path);
                    let _ = fs::remove_file(blob_file);
                    let size_u64 = if size > 0 { size as u64 } else { 0 };
                    total = total.saturating_sub(size_u64);
                }
                None => break,
            }
        }
        Ok(())
    }

    /// Evict least-recently-used entries until the cache fits its cap.
    pub fn prune_to_cap(&self) -> Result<(), CacheError> {
        self.evict_if_over_cap()
    }

    pub fn cap_bytes(&self) -> u64 {
        self.cap_bytes
    }

    pub fn set_cap_bytes(&mut self, cap: u64) {
        self.cap_bytes = cap;
    }

    pub fn entry_count(&self) -> Result<u64, CacheError> {
        let db = self.db.lock().map_err(|_| CacheError::LockPoisoned)?;
        let count: i64 = db.query_row("SELECT COUNT(*) FROM entries", [], |r| r.get(0))?;
        Ok(count.max(0) as u64)
    }

    pub fn list_entries(
        &self,
        limit: usize,
        offset: usize,
        filter: Option<&str>,
    ) -> Result<Vec<CacheEntry>, CacheError> {
        let db = self.db.lock().map_err(|_| CacheError::LockPoisoned)?;
        let limit_i64 = limit as i64;
        let offset_i64 = offset as i64;

        let mut entries = Vec::new();

        if let Some(f) = filter.filter(|s| !s.trim().is_empty()) {
            let pattern = format!("%{}%", f.trim());
            let mut stmt = db.prepare(
                "SELECT url, path, etag, last_modified, content_type, size, fetched_at, checked_at, last_used_at
                 FROM entries
                 WHERE url LIKE ?1
                 ORDER BY last_used_at DESC
                 LIMIT ?2 OFFSET ?3",
            )?;
            let rows = stmt.query_map(params![pattern, limit_i64, offset_i64], |row| {
                let url_str: String = row.get(0)?;
                let size_signed: i64 = row.get(5)?;
                Ok(CacheEntry {
                    url: CanonicalUrl::new(url_str),
                    blob_path: row.get(1)?,
                    etag: row.get(2)?,
                    last_modified: row.get(3)?,
                    content_type: row.get(4)?,
                    size: size_signed.max(0) as u64,
                    fetched_at: row.get(6)?,
                    checked_at: row.get(7)?,
                    last_used_at: row.get(8)?,
                })
            })?;
            for r in rows {
                entries.push(r?);
            }
        } else {
            let mut stmt = db.prepare(
                "SELECT url, path, etag, last_modified, content_type, size, fetched_at, checked_at, last_used_at
                 FROM entries
                 ORDER BY last_used_at DESC
                 LIMIT ?1 OFFSET ?2",
            )?;
            let rows = stmt.query_map(params![limit_i64, offset_i64], |row| {
                let url_str: String = row.get(0)?;
                let size_signed: i64 = row.get(5)?;
                Ok(CacheEntry {
                    url: CanonicalUrl::new(url_str),
                    blob_path: row.get(1)?,
                    etag: row.get(2)?,
                    last_modified: row.get(3)?,
                    content_type: row.get(4)?,
                    size: size_signed.max(0) as u64,
                    fetched_at: row.get(6)?,
                    checked_at: row.get(7)?,
                    last_used_at: row.get(8)?,
                })
            })?;
            for r in rows {
                entries.push(r?);
            }
        }

        Ok(entries)
    }

    pub fn is_cached(&self, url: &CanonicalUrl) -> Result<bool, CacheError> {
        let db = self.db.lock().map_err(|_| CacheError::LockPoisoned)?;
        let exists: bool = db
            .query_row(
                "SELECT 1 FROM entries WHERE url = ?1",
                params![url.as_str()],
                |_| Ok(true),
            )
            .optional()?
            .unwrap_or(false);
        Ok(exists)
    }

    pub fn clear_all(&self) -> Result<(), CacheError> {
        let db = self.db.lock().map_err(|_| CacheError::LockPoisoned)?;
        let mut stmt = db.prepare("SELECT path FROM entries")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        for path in rows {
            if let Ok(p) = path {
                let _ = fs::remove_file(self.blobs_dir.join(p));
            }
        }
        db.execute("DELETE FROM entries", [])?;
        let _ = db.execute("VACUUM", []);
        Ok(())
    }

    pub fn clear_older_than(&self, older_than_epoch_secs: i64) -> Result<usize, CacheError> {
        let db = self.db.lock().map_err(|_| CacheError::LockPoisoned)?;
        let mut count = 0;
        let mut stmt = db.prepare("SELECT path FROM entries WHERE last_used_at < ?1")?;
        let rows = stmt.query_map(params![older_than_epoch_secs], |r| r.get::<_, String>(0))?;
        for path in rows {
            if let Ok(p) = path {
                let _ = fs::remove_file(self.blobs_dir.join(p));
                count += 1;
            }
        }
        db.execute(
            "DELETE FROM entries WHERE last_used_at < ?1",
            params![older_than_epoch_secs],
        )?;
        Ok(count)
    }
}

fn hash_url_sha256(url: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(url.as_bytes());
    let hash = hasher.finalize();
    format!("{hash:x}")
}

fn now_epoch_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_put_and_get_roundtrip() {
        let temp_dir = std::env::temp_dir().join(format!("wol_cache_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_dir);

        let cache = DiskCache::open(&temp_dir, 1024 * 1024).unwrap();
        let url = CanonicalUrl::new("https://wol.example.org/es/wol/d/r4/lp-s/1102026210".to_string());
        let body = b"<html>article content</html>";

        let entry = cache
            .put(&url, "text/html; charset=utf-8", Some("\"etag123\""), None, body)
            .unwrap();
        assert_eq!(entry.size, body.len() as u64);
        assert_eq!(entry.content_type, "text/html; charset=utf-8");

        let fetched = cache.get(&url).unwrap();
        assert!(fetched.is_some());
        let (cached_entry, cached_bytes) = fetched.unwrap();
        assert_eq!(cached_bytes, body);
        assert_eq!(cached_entry.url, url);
        assert_eq!(cached_entry.etag.as_deref(), Some("\"etag123\""));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn cache_eviction_when_cap_exceeded() {
        let temp_dir = std::env::temp_dir().join(format!("wol_cache_evict_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_dir);

        // Cap at 100 bytes
        let cache = DiskCache::open(&temp_dir, 100).unwrap();
        let url1 = CanonicalUrl::new("https://wol.example.org/1".to_string());
        let url2 = CanonicalUrl::new("https://wol.example.org/2".to_string());

        let payload_60b = vec![b'a'; 60];
        cache
            .put(&url1, "text/plain", None, None, &payload_60b)
            .unwrap();

        // Putting second 60b exceeds 100b cap, triggering LRU eviction of url1
        cache
            .put(&url2, "text/plain", None, None, &payload_60b)
            .unwrap();

        let total = cache.total_size().unwrap();
        assert!(total <= 100);

        let hit1 = cache.get(&url1).unwrap();
        assert!(hit1.is_none());

        let hit2 = cache.get(&url2).unwrap();
        assert!(hit2.is_some());

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn cache_inspector_operations() {
        let temp_dir = std::env::temp_dir().join(format!("wol_cache_insp_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_dir);

        let cache = DiskCache::open(&temp_dir, 1024 * 1024).unwrap();
        let url1 = CanonicalUrl::new("https://wol.example.org/es/wol/d/r4/lp-s/1".to_string());
        let url2 = CanonicalUrl::new("https://wol.example.org/assets/css/main.css".to_string());

        cache.put(&url1, "text/html", None, None, b"doc1").unwrap();
        cache.put(&url2, "text/css", None, None, b"css").unwrap();

        assert_eq!(cache.entry_count().unwrap(), 2);
        assert!(cache.is_cached(&url1).unwrap());
        assert!(cache.is_cached(&url2).unwrap());
        assert!(!cache.is_cached(&CanonicalUrl::new("https://wol.example.org/missing".to_string())).unwrap());

        let list = cache.list_entries(10, 0, None).unwrap();
        assert_eq!(list.len(), 2);

        let filtered = cache.list_entries(10, 0, Some("assets")).unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].url, url2);

        // Clear all
        cache.clear_all().unwrap();
        assert_eq!(cache.entry_count().unwrap(), 0);
        assert_eq!(cache.total_size().unwrap(), 0);
        assert!(!cache.is_cached(&url1).unwrap());

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
