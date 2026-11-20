use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use reqwest::{Client, Method, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
#[cfg(not(target_os = "ios"))]
use tokio::io::{AsyncReadExt, AsyncWriteExt};
#[cfg(not(target_os = "ios"))]
use tokio::net::TcpListener;
use tokio::sync::{Mutex, Notify};

use crate::state::{
    now_iso_utc, unix_seconds_now, GoogleDriveSyncConfig, NextcloudSyncConfig, StateManager,
    SyncProvider, SyncState,
};

fn google_credentials() -> Result<(String, Option<String>), String> {
    google_credentials_from(
        |key| std::env::var(key).ok(),
        option_env!("WOL_BUILTIN_GOOGLE_CLIENT_ID").unwrap_or(""),
        option_env!("WOL_BUILTIN_GOOGLE_CLIENT_SECRET").unwrap_or(""),
        // iOS uses a public PKCE client, which Google accepts without a secret.
        !cfg!(target_os = "ios"),
    )
}

/// Resolves the Google OAuth client without touching the environment directly.
///
/// Precedence per key: runtime `WOL_GOOGLE_CLIENT_ID`/`WOL_GOOGLE_CLIENT_SECRET`
/// (so a developer can point an existing build at their own client) → the client
/// baked in by `build.rs` from `.env` → an actionable error.
fn google_credentials_from(
    env: impl Fn(&str) -> Option<String>,
    builtin_id: &str,
    builtin_secret: &str,
    secret_required: bool,
) -> Result<(String, Option<String>), String> {
    let pick = |key: &str, builtin: &str| {
        env(key)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| builtin.trim().to_string())
    };
    let client_id = pick("WOL_GOOGLE_CLIENT_ID", builtin_id);
    let client_secret = pick("WOL_GOOGLE_CLIENT_SECRET", builtin_secret);
    if client_id.is_empty() {
        return Err(missing_google_credentials_message());
    }
    if client_secret.is_empty() {
        return if secret_required {
            Err(missing_google_credentials_message())
        } else {
            Ok((client_id, None))
        };
    }
    Ok((client_id, Some(client_secret)))
}

fn missing_google_credentials_message() -> String {
    "Google Drive sign-in is not configured in this build. Rebuild with \
     WOL_GOOGLE_CLIENT_ID and WOL_GOOGLE_CLIENT_SECRET in the environment (or in the \
     workspace .env, which build.rs reads). Nextcloud sync needs no Google credentials."
        .to_string()
}
pub const GOOGLE_AUTH_URL: &str = "https://accounts.google.com/o/oauth2/auth";
pub const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
pub const GOOGLE_DRIVE_SCOPE: &str = "https://www.googleapis.com/auth/drive.file";
pub const SYNC_FILE_NAME: &str = "wol-highlights-sync.json";
pub const DEFAULT_REMOTE_PATH: &str = "OfflineWOL/wol-highlights-sync.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncEnvelope {
    pub format: String,
    pub version: u32,
    pub updated_at: String,
    pub device_id: String,
    pub wol_marks: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    pub provider: SyncProvider,
    pub connected: bool,
    pub account_label: Option<String>,
    pub server_url: Option<String>,
    pub remote_path: Option<String>,
    pub last_synced_at: Option<String>,
    pub local_updated_at: Option<String>,
    pub last_error: Option<String>,
    pub syncing: bool,
    pub changed_local: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncAction {
    NoOp,
    PullRemote,
    PushLocal,
    MergeAndPush,
}

pub fn decide_sync_action(
    local_has_data: bool,
    local_updated_at: Option<&str>,
    last_synced_at: Option<&str>,
    remote_updated_at: Option<&str>,
) -> SyncAction {
    if remote_updated_at.is_none() && local_has_data {
        return SyncAction::PushLocal;
    }
    let local_dirty = match (local_updated_at, last_synced_at) {
        (Some(loc), Some(synced)) => loc > synced,
        (_, None) => local_has_data,
        (None, _) => false,
    };

    let remote_newer = match (remote_updated_at, last_synced_at) {
        (Some(rem), Some(synced)) => rem > synced,
        (Some(_), None) => true,
        (None, _) => false,
    };

    match (local_dirty, remote_newer) {
        (false, false) => SyncAction::NoOp,
        (false, true) => SyncAction::PullRemote,
        (true, false) => SyncAction::PushLocal,
        (true, true) => {
            if !local_has_data && last_synced_at.is_none() {
                SyncAction::PullRemote
            } else {
                SyncAction::MergeAndPush
            }
        }
    }
}

pub fn base64_encode(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((input.len() + 2) / 3 * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((triple >> 18) & 0x3f) as usize] as char);
        out.push(TABLE[((triple >> 12) & 0x3f) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[((triple >> 6) & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(triple & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

pub fn base64url_encode(input: &[u8]) -> String {
    base64_encode(input)
        .replace('+', "-")
        .replace('/', "_")
        .trim_end_matches('=')
        .to_string()
}

pub fn pkce_challenge(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    base64url_encode(&digest)
}

pub fn normalize_nextcloud_server_url(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("Nextcloud server URL is required.".to_string());
    }
    let with_scheme = if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_string()
    } else {
        format!("https://{trimmed}")
    };
    let parsed = url::Url::parse(&with_scheme).map_err(|e| format!("Invalid server URL: {e}"))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| "Server URL must include a hostname.".to_string())?;
    let mut out = format!("{}://{}", parsed.scheme(), host);
    if let Some(port) = parsed.port() {
        out.push_str(&format!(":{port}"));
    }
    let path = parsed.path().trim_end_matches('/');
    if !path.is_empty() && path != "/" {
        out.push_str(path);
    }
    Ok(out)
}

fn encode_path_segments(path: &str) -> String {
    path.trim_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .map(|seg| url::form_urlencoded::byte_serialize(seg.as_bytes()).collect::<String>())
        .collect::<Vec<_>>()
        .join("/")
}

pub fn nextcloud_webdav_root_url(server_url: &str, username: &str) -> String {
    let base = server_url.trim_end_matches('/');
    let user_enc: String = url::form_urlencoded::byte_serialize(username.as_bytes()).collect();
    format!("{base}/remote.php/dav/files/{user_enc}/")
}

pub fn nextcloud_webdav_file_url(server_url: &str, username: &str, remote_path: &str) -> String {
    let root = nextcloud_webdav_root_url(server_url, username);
    let clean_path = encode_path_segments(if remote_path.trim().is_empty() {
        DEFAULT_REMOTE_PATH
    } else {
        remote_path
    });
    format!("{root}{clean_path}")
}

fn marks_has_content(marks: Option<&Value>) -> bool {
    let Some(v) = marks else {
        return false;
    };
    if v.is_null() {
        return false;
    }
    let hl = v.get("highlightCount").and_then(|x| x.as_u64()).unwrap_or(0);
    let notes = v.get("noteCount").and_then(|x| x.as_u64()).unwrap_or(0);
    if hl > 0 || notes > 0 {
        return true;
    }
    let has_docs = v
        .get("docs")
        .and_then(|x| x.as_object())
        .is_some_and(|m| !m.is_empty());
    let has_bible = v
        .get("bible")
        .and_then(|x| x.as_object())
        .is_some_and(|m| !m.is_empty());
    let has_notes_docs = v
        .pointer("/notes/docs")
        .and_then(|x| x.as_object())
        .is_some_and(|m| !m.is_empty());
    let has_notes_bible = v
        .pointer("/notes/bible")
        .and_then(|x| x.as_object())
        .is_some_and(|m| !m.is_empty());
    let has_inputs_docs = v
        .pointer("/inputFields/docs")
        .and_then(|x| x.as_object())
        .is_some_and(|m| !m.is_empty());
    let has_inputs_bible = v
        .pointer("/inputFields/bible")
        .and_then(|x| x.as_object())
        .is_some_and(|m| !m.is_empty());
    has_docs
        || has_bible
        || has_notes_docs
        || has_notes_bible
        || has_inputs_docs
        || has_inputs_bible
}

fn merge_range_list(a: Option<&Value>, b: Option<&Value>) -> Vec<Value> {
    let mut seen: BTreeMap<(i64, i64), i64> = BTreeMap::new();
    for src in [a, b].into_iter().flatten() {
        if let Some(arr) = src.as_array() {
            for item in arr {
                if let Some(tuple) = item.as_array() {
                    if tuple.len() >= 3 {
                        if let (Some(s), Some(e), Some(c)) =
                            (tuple[0].as_i64(), tuple[1].as_i64(), tuple[2].as_i64())
                        {
                            seen.insert((s, e), c);
                        }
                    }
                }
            }
        }
    }
    seen.into_iter()
        .map(|((s, e), c)| json!([s, e, c]))
        .collect()
}

fn merge_nested_ranges(a: Option<&Value>, b: Option<&Value>) -> Map<String, Value> {
    let mut out = Map::new();
    let mut keys: HashSet<String> = HashSet::new();
    for src in [a, b].into_iter().flatten() {
        if let Some(obj) = src.as_object() {
            keys.extend(obj.keys().cloned());
        }
    }
    for key in keys {
        let a_inner = a.and_then(|v| v.get(&key)).and_then(|v| v.as_object());
        let b_inner = b.and_then(|v| v.get(&key)).and_then(|v| v.as_object());
        let mut inner_keys: HashSet<String> = HashSet::new();
        if let Some(m) = a_inner {
            inner_keys.extend(m.keys().cloned());
        }
        if let Some(m) = b_inner {
            inner_keys.extend(m.keys().cloned());
        }
        let mut merged_inner = Map::new();
        for ik in inner_keys {
            let merged_list = merge_range_list(
                a_inner.and_then(|m| m.get(&ik)),
                b_inner.and_then(|m| m.get(&ik)),
            );
            if !merged_list.is_empty() {
                merged_inner.insert(ik, Value::Array(merged_list));
            }
        }
        if !merged_inner.is_empty() {
            out.insert(key, Value::Object(merged_inner));
        }
    }
    out
}

fn note_dedup_key(note: &Value) -> String {
    if let Some(guid) = note.get("guid").and_then(|v| v.as_str()) {
        if !guid.trim().is_empty() {
            return format!("guid:{guid}");
        }
    }
    let title = note.get("title").and_then(|v| v.as_str()).unwrap_or("");
    let content = note.get("content").and_then(|v| v.as_str()).unwrap_or("");
    format!("tc:{title}\u{0}{content}")
}

fn note_timestamp(note: &Value) -> &str {
    note.get("lastModified")
        .and_then(|v| v.as_str())
        .unwrap_or("")
}

fn merge_note_list(a: Option<&Value>, b: Option<&Value>) -> Vec<Value> {
    let mut map: HashMap<String, Value> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for src in [a, b].into_iter().flatten() {
        let items: Vec<&Value> = if let Some(arr) = src.as_array() {
            arr.iter().collect()
        } else if src.is_object() {
            vec![src]
        } else {
            vec![]
        };
        for item in items {
            let title = item.get("title").and_then(|v| v.as_str()).unwrap_or("");
            let content = item.get("content").and_then(|v| v.as_str()).unwrap_or("");
            if title.trim().is_empty() && content.trim().is_empty() {
                continue;
            }
            let key = note_dedup_key(item);
            if let Some(existing) = map.get(&key) {
                if note_timestamp(item) >= note_timestamp(existing) {
                    map.insert(key.clone(), item.clone());
                }
            } else {
                order.push(key.clone());
                map.insert(key, item.clone());
            }
        }
    }
    order.into_iter().filter_map(|k| map.remove(&k)).collect()
}

fn merge_nested_notes(a: Option<&Value>, b: Option<&Value>) -> Map<String, Value> {
    let mut out = Map::new();
    let mut keys: HashSet<String> = HashSet::new();
    for src in [a, b].into_iter().flatten() {
        if let Some(obj) = src.as_object() {
            keys.extend(obj.keys().cloned());
        }
    }
    for key in keys {
        let a_inner = a.and_then(|v| v.get(&key)).and_then(|v| v.as_object());
        let b_inner = b.and_then(|v| v.get(&key)).and_then(|v| v.as_object());
        let mut inner_keys: HashSet<String> = HashSet::new();
        if let Some(m) = a_inner {
            inner_keys.extend(m.keys().cloned());
        }
        if let Some(m) = b_inner {
            inner_keys.extend(m.keys().cloned());
        }
        let mut merged_inner = Map::new();
        for ik in inner_keys {
            let merged_list = merge_note_list(
                a_inner.and_then(|m| m.get(&ik)),
                b_inner.and_then(|m| m.get(&ik)),
            );
            if !merged_list.is_empty() {
                merged_inner.insert(ik, Value::Array(merged_list));
            }
        }
        if !merged_inner.is_empty() {
            out.insert(key, Value::Object(merged_inner));
        }
    }
    out
}

fn merge_nested_inputs(a: Option<&Value>, b: Option<&Value>) -> Map<String, Value> {
    let mut out = Map::new();
    let mut keys: HashSet<String> = HashSet::new();
    for src in [a, b].into_iter().flatten() {
        if let Some(obj) = src.as_object() {
            keys.extend(obj.keys().cloned());
        }
    }
    for key in keys {
        let mut merged_inner = Map::new();
        if let Some(m) = a.and_then(|v| v.get(&key)).and_then(|v| v.as_object()) {
            for (k, v) in m {
                if v.as_str().is_some_and(|s| !s.trim().is_empty()) {
                    merged_inner.insert(k.clone(), v.clone());
                }
            }
        }
        if let Some(m) = b.and_then(|v| v.get(&key)).and_then(|v| v.as_object()) {
            for (k, v) in m {
                if v.as_str().is_some_and(|s| !s.trim().is_empty()) {
                    merged_inner.insert(k.clone(), v.clone());
                }
            }
        }
        if !merged_inner.is_empty() {
            out.insert(key, Value::Object(merged_inner));
        }
    }
    out
}

pub fn merge_marks(local: Option<&Value>, remote: Option<&Value>) -> Option<Value> {
    match (local.filter(|v| !v.is_null()), remote.filter(|v| !v.is_null())) {
        (None, None) => None,
        (Some(l), None) => Some(l.clone()),
        (None, Some(r)) => Some(r.clone()),
        (Some(l), Some(r)) => {
            let docs = merge_nested_ranges(l.get("docs"), r.get("docs"));
            let bible = merge_nested_ranges(l.get("bible"), r.get("bible"));

            let notes_docs = merge_nested_notes(l.pointer("/notes/docs"), r.pointer("/notes/docs"));
            let notes_bible =
                merge_nested_notes(l.pointer("/notes/bible"), r.pointer("/notes/bible"));

            let inputs_docs =
                merge_nested_inputs(l.pointer("/inputFields/docs"), r.pointer("/inputFields/docs"));
            let inputs_bible = merge_nested_inputs(
                l.pointer("/inputFields/bible"),
                r.pointer("/inputFields/bible"),
            );

            let mut highlight_count: u64 = 0;
            let mut color_counts: BTreeMap<String, u64> = BTreeMap::new();
            for group in [&docs, &bible] {
                for inner in group.values() {
                    if let Some(obj) = inner.as_object() {
                        for ranges in obj.values() {
                            if let Some(arr) = ranges.as_array() {
                                for range in arr {
                                    if let Some(t) = range.as_array() {
                                        if t.len() >= 3 {
                                            highlight_count += 1;
                                            let c = t[2].as_i64().unwrap_or(1).to_string();
                                            *color_counts.entry(c).or_insert(0) += 1;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            let mut note_count: u64 = 0;
            for group in [&notes_docs, &notes_bible] {
                for inner in group.values() {
                    if let Some(obj) = inner.as_object() {
                        for list in obj.values() {
                            if let Some(arr) = list.as_array() {
                                note_count += arr.len() as u64;
                            }
                        }
                    }
                }
            }
            for group in [&inputs_docs, &inputs_bible] {
                for inner in group.values() {
                    if let Some(obj) = inner.as_object() {
                        note_count += obj.len() as u64;
                    }
                }
            }

            let color_map: Map<String, Value> = color_counts
                .into_iter()
                .map(|(k, v)| (k, json!(v)))
                .collect();

            let file_name = l
                .get("fileName")
                .or_else(|| r.get("fileName"))
                .and_then(|v| v.as_str())
                .unwrap_or("Cloud Sync");
            let enabled = l
                .get("enabled")
                .and_then(|v| v.as_bool())
                .unwrap_or_else(|| r.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true));

            Some(json!({
                "version": 1,
                "fileName": file_name,
                "loadedAt": now_iso_utc(),
                "enabled": enabled,
                "highlightCount": highlight_count,
                "noteCount": note_count,
                "ignored": 0,
                "documentCount": docs.len(),
                "bibleChapterCount": bible.len(),
                "colorCounts": color_map,
                "docs": docs,
                "bible": bible,
                "notes": {
                    "docs": notes_docs,
                    "bible": notes_bible,
                },
                "inputFields": {
                    "docs": inputs_docs,
                    "bible": inputs_bible,
                }
            }))
        }
    }
}

async fn read_json<T: serde::de::DeserializeOwned>(resp: reqwest::Response) -> Result<T, String> {
    let bytes = resp.bytes().await.map_err(|e| format!("HTTP read failed: {e}"))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("Invalid JSON response: {e}"))
}

pub fn open_system_browser(url: &str) -> Result<(), String> {
    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
    {
        #[cfg(target_os = "macos")]
        let status = std::process::Command::new("open").arg(url).status();
        #[cfg(target_os = "linux")]
        let status = std::process::Command::new("xdg-open").arg(url).status();
        #[cfg(target_os = "windows")]
        let status = std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .status();

        match status {
            Ok(s) if s.success() => Ok(()),
            Ok(s) => Err(format!("Browser command exited with status {s}")),
            Err(e) => Err(format!("Failed to open system browser: {e}")),
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = url;
        Err("System browser opening is not supported on this platform without a host opener.".to_string())
    }
}

/// Invoked after a sync run that changed local marks, so a front-end can refresh
/// its views (the desktop app re-applies highlights in both webview windows).
pub type SyncListener = Arc<dyn Fn() + Send + Sync>;

/// Host-provided callback for opening an external URL (e.g. via `tauri-plugin-opener`).
pub type BrowserOpener = Arc<dyn Fn(&str) -> Result<(), String> + Send + Sync>;

/// Future returned by a host-provided interactive OAuth session (e.g. `ASWebAuthenticationSession` on iOS).
pub type OAuthSessionFuture =
    std::pin::Pin<Box<dyn std::future::Future<Output = Result<String, String>> + Send>>;

/// Host-provided callback that opens `(auth_url, callback_scheme)` and resolves to the full callback URL.
pub type OAuthSessionOpener = Arc<dyn Fn(&str, &str) -> OAuthSessionFuture + Send + Sync>;

#[derive(Clone)]
pub struct SyncManager {
    state: StateManager,
    client: Client,
    sync_lock: Arc<Mutex<()>>,
    syncing: Arc<AtomicBool>,
    debounce_notify: Arc<Notify>,
    debounce_started: Arc<AtomicBool>,
    listener: Arc<std::sync::Mutex<Option<SyncListener>>>,
    browser_opener: Arc<std::sync::Mutex<Option<BrowserOpener>>>,
    oauth_session_opener: Arc<std::sync::Mutex<Option<OAuthSessionOpener>>>,
    /// Handle to this very instance, so background tasks can own a `'static` clone.
    self_ref: std::sync::OnceLock<Arc<SyncManager>>,
}

impl SyncManager {
    pub fn new(state: StateManager) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent("OfflineWOL-Sync/1.0")
            .build()
            .unwrap_or_else(|_| Client::new());
        Self {
            state,
            client,
            sync_lock: Arc::new(Mutex::new(())),
            syncing: Arc::new(AtomicBool::new(false)),
            debounce_notify: Arc::new(Notify::new()),
            debounce_started: Arc::new(AtomicBool::new(false)),
            listener: Arc::new(std::sync::Mutex::new(None)),
            browser_opener: Arc::new(std::sync::Mutex::new(None)),
            oauth_session_opener: Arc::new(std::sync::Mutex::new(None)),
            self_ref: std::sync::OnceLock::new(),
        }
    }

    /// Register a callback fired whenever a sync changes local marks.
    pub fn set_listener(&self, listener: Option<SyncListener>) {
        if let Ok(mut slot) = self.listener.try_lock() {
            *slot = listener;
        }
    }

    /// Register a host-provided URL opener (used on mobile or when Tauri handles URLs).
    pub fn set_browser_opener(&self, opener: Option<BrowserOpener>) {
        if let Ok(mut slot) = self.browser_opener.try_lock() {
            *slot = opener;
        }
    }

    /// Register a host-provided interactive OAuth session handler (e.g. `ASWebAuthenticationSession` on iOS).
    pub fn set_oauth_session_opener(&self, opener: Option<OAuthSessionOpener>) {
        if let Ok(mut slot) = self.oauth_session_opener.try_lock() {
            *slot = opener;
        }
    }

    fn open_browser(&self, url: &str) -> Result<(), String> {
        let opener = self
            .browser_opener
            .try_lock()
            .ok()
            .and_then(|slot| slot.clone());
        match opener {
            Some(cb) => cb(url),
            None => open_system_browser(url),
        }
    }

    #[cfg(target_os = "ios")]
    async fn open_oauth_session(&self, auth_url: &str, callback_scheme: &str) -> Result<String, String> {
        let opener = self
            .oauth_session_opener
            .try_lock()
            .ok()
            .and_then(|slot| slot.clone())
            .ok_or_else(|| {
                "Google Drive OAuth requires an ASWebAuthenticationSession handler on iOS."
                    .to_string()
            })?;
        opener(auth_url, callback_scheme).await
    }

    fn notify_listener(&self) {
        let listener = self.listener.try_lock().ok().and_then(|slot| slot.clone());
        if let Some(listener) = listener {
            listener();
        }
    }

    pub fn status(&self) -> Result<SyncStatus, String> {
        let sync = self.state.get_sync_state().map_err(|e| e.to_string())?;
        Ok(self.build_status(&sync, false))
    }

    fn build_status(&self, sync: &SyncState, changed_local: bool) -> SyncStatus {
        match sync.provider {
            SyncProvider::None => SyncStatus {
                provider: SyncProvider::None,
                connected: false,
                account_label: None,
                server_url: None,
                remote_path: None,
                last_synced_at: sync.last_synced_at.clone(),
                local_updated_at: sync.local_updated_at.clone(),
                last_error: sync.last_error.clone(),
                syncing: self.syncing.load(Ordering::Relaxed),
                changed_local,
            },
            SyncProvider::Nextcloud => {
                let nc = sync.nextcloud.as_ref();
                SyncStatus {
                    provider: SyncProvider::Nextcloud,
                    connected: nc.is_some(),
                    account_label: nc.map(|c| c.username.clone()),
                    server_url: nc.map(|c| c.server_url.clone()),
                    remote_path: nc.map(|c| c.remote_path.clone()),
                    last_synced_at: sync.last_synced_at.clone(),
                    local_updated_at: sync.local_updated_at.clone(),
                    last_error: sync.last_error.clone(),
                    syncing: self.syncing.load(Ordering::Relaxed),
                    changed_local,
                }
            }
            SyncProvider::GoogleDrive => {
                let gd = sync.google_drive.as_ref();
                SyncStatus {
                    provider: SyncProvider::GoogleDrive,
                    connected: gd.is_some(),
                    account_label: gd
                        .and_then(|c| c.email.clone())
                        .or_else(|| Some("Google Drive".to_string())),
                    server_url: None,
                    remote_path: Some(SYNC_FILE_NAME.to_string()),
                    last_synced_at: sync.last_synced_at.clone(),
                    local_updated_at: sync.local_updated_at.clone(),
                    last_error: sync.last_error.clone(),
                    syncing: self.syncing.load(Ordering::Relaxed),
                    changed_local,
                }
            }
        }
    }

    /// Build a manager that knows its own `Arc`, enabling debounced/background sync.
    pub fn new_shared(state: StateManager) -> Arc<Self> {
        let manager = Arc::new(Self::new(state));
        let _ = manager.self_ref.set(Arc::clone(&manager));
        manager
    }

    fn owned_handle(&self) -> Option<Arc<Self>> {
        self.self_ref.get().map(Arc::clone)
    }

    pub fn schedule_debounced_sync(&self) {
        let is_configured = self
            .state
            .get_sync_state()
            .is_ok_and(|s| s.provider != SyncProvider::None);
        if !is_configured {
            return;
        }

        if !self.debounce_started.swap(true, Ordering::SeqCst) {
            // Only an `Arc`-owned manager can spawn its background loop safely.
            let Some(mgr) = self.owned_handle() else {
                self.debounce_started.store(false, Ordering::SeqCst);
                return;
            };
            if tokio::runtime::Handle::try_current().is_err() {
                self.debounce_started.store(false, Ordering::SeqCst);
                return;
            }
            tokio::spawn(async move {
                loop {
                    mgr.debounce_notify.notified().await;
                    // Coalesce rapid edits for 3 seconds
                    loop {
                        let res = tokio::time::timeout(
                            Duration::from_secs(3),
                            mgr.debounce_notify.notified(),
                        )
                        .await;
                        if res.is_err() {
                            break;
                        }
                    }
                    let _ = mgr.sync_now().await;
                }
            });
        }
        self.debounce_notify.notify_one();
    }

    pub fn disconnect(&self) -> Result<SyncStatus, String> {
        let updated = self
            .state
            .update_sync_state(|s| {
                s.provider = SyncProvider::None;
                s.nextcloud = None;
                s.google_drive = None;
                s.last_synced_at = None;
                s.last_error = None;
            })
            .map_err(|e| e.to_string())?;
        Ok(self.build_status(&updated, false))
    }

    pub async fn connect_nextcloud(
        &self,
        server_url: String,
        username: String,
        password: String,
        remote_path: Option<String>,
    ) -> Result<SyncStatus, String> {
        let server_url = normalize_nextcloud_server_url(&server_url)?;
        let username = username.trim().to_string();
        if username.is_empty() || password.is_empty() {
            return Err("Username and password/app password are required.".to_string());
        }
        let remote_path = remote_path
            .map(|p| p.trim().trim_start_matches('/').to_string())
            .filter(|p| !p.is_empty())
            .unwrap_or_else(|| DEFAULT_REMOTE_PATH.to_string());

        let cfg = NextcloudSyncConfig {
            server_url,
            username,
            password,
            remote_path,
        };

        // Verify credentials with WebDAV PROPFIND
        self.verify_nextcloud(&cfg).await?;

        self.state
            .update_sync_state(|s| {
                s.provider = SyncProvider::Nextcloud;
                s.nextcloud = Some(cfg);
                s.google_drive = None;
                s.last_synced_at = None;
                s.last_error = None;
            })
            .map_err(|e| e.to_string())?;

        self.sync_now().await
    }

    pub async fn connect_nextcloud_browser(
        &self,
        server_url: String,
        remote_path: Option<String>,
    ) -> Result<SyncStatus, String> {
        let server_url = normalize_nextcloud_server_url(&server_url)?;
        let remote_path = remote_path
            .map(|p| p.trim().trim_start_matches('/').to_string())
            .filter(|p| !p.is_empty())
            .unwrap_or_else(|| DEFAULT_REMOTE_PATH.to_string());

        let init_url = format!("{server_url}/index.php/login/v2");
        let resp = self
            .client
            .post(&init_url)
            .send()
            .await
            .map_err(|e| format!("Failed to reach Nextcloud Login Flow v2: {e}"))?;

        if !resp.status().is_success() {
            return Err(format!(
                "Nextcloud browser login returned HTTP {}. Try entering an App Password manually.",
                resp.status()
            ));
        }

        #[derive(Deserialize)]
        struct LoginFlowPoll {
            token: String,
            endpoint: String,
        }
        #[derive(Deserialize)]
        struct LoginFlowInit {
            poll: LoginFlowPoll,
            login: String,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct LoginFlowResult {
            server: String,
            login_name: String,
            app_password: String,
        }

        let flow: LoginFlowInit = read_json(resp).await?;

        self.open_browser(&flow.login)?;

        // Poll up to 3 minutes
        for _ in 0..120 {
            tokio::time::sleep(Duration::from_millis(1500)).await;
            let poll_resp = self
                .client
                .post(&flow.poll.endpoint)
                .form(&[("token", flow.poll.token.as_str())])
                .send()
                .await;

            if let Ok(r) = poll_resp {
                if r.status() == StatusCode::OK {
                    let creds: LoginFlowResult = read_json(r).await?;
                    let normalized = normalize_nextcloud_server_url(&creds.server)?;
                    let cfg = NextcloudSyncConfig {
                        server_url: normalized,
                        username: creds.login_name,
                        password: creds.app_password,
                        remote_path,
                    };
                    self.state
                        .update_sync_state(|s| {
                            s.provider = SyncProvider::Nextcloud;
                            s.nextcloud = Some(cfg);
                            s.google_drive = None;
                            s.last_synced_at = None;
                            s.last_error = None;
                        })
                        .map_err(|e| e.to_string())?;
                    return self.sync_now().await;
                }
            }
        }

        Err("Timed out waiting for Nextcloud browser sign-in.".to_string())
    }

    async fn verify_nextcloud(&self, cfg: &NextcloudSyncConfig) -> Result<(), String> {
        let root_url = nextcloud_webdav_root_url(&cfg.server_url, &cfg.username);
        let auth = format!(
            "Basic {}",
            base64_encode(format!("{}:{}", cfg.username, cfg.password).as_bytes())
        );
        let propfind = Method::from_bytes(b"PROPFIND")
            .map_err(|e| format!("Invalid WebDAV method: {e}"))?;
        let resp = self
            .client
            .request(propfind, &root_url)
            .header(AUTHORIZATION, auth)
            .header("Depth", "0")
            .send()
            .await
            .map_err(|e| format!("Could not connect to Nextcloud: {e}"))?;

        if resp.status() == StatusCode::UNAUTHORIZED {
            return Err("Nextcloud rejected username or password (401 Unauthorized).".to_string());
        }
        if !resp.status().is_success() && resp.status() != StatusCode::MULTI_STATUS {
            return Err(format!("Nextcloud WebDAV returned HTTP {}.", resp.status()));
        }
        Ok(())
    }

    async fn ensure_nextcloud_parent_dirs(&self, cfg: &NextcloudSyncConfig) -> Result<(), String> {
        let parts: Vec<&str> = cfg
            .remote_path
            .trim_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();
        if parts.len() <= 1 {
            return Ok(());
        }
        let auth = format!(
            "Basic {}",
            base64_encode(format!("{}:{}", cfg.username, cfg.password).as_bytes())
        );
        let mkcol = Method::from_bytes(b"MKCOL").map_err(|e| e.to_string())?;
        let mut current = nextcloud_webdav_root_url(&cfg.server_url, &cfg.username);
        for dir in &parts[..parts.len() - 1] {
            let seg: String = url::form_urlencoded::byte_serialize(dir.as_bytes()).collect();
            current.push_str(&seg);
            current.push('/');
            let _ = self
                .client
                .request(mkcol.clone(), &current)
                .header(AUTHORIZATION, auth.clone())
                .send()
                .await;
        }
        Ok(())
    }

    async fn nextcloud_download(&self, cfg: &NextcloudSyncConfig) -> Result<Option<SyncEnvelope>, String> {
        let file_url = nextcloud_webdav_file_url(&cfg.server_url, &cfg.username, &cfg.remote_path);
        let auth = format!(
            "Basic {}",
            base64_encode(format!("{}:{}", cfg.username, cfg.password).as_bytes())
        );
        let resp = self
            .client
            .get(&file_url)
            .header(AUTHORIZATION, auth)
            .send()
            .await
            .map_err(|e| format!("Nextcloud download failed: {e}"))?;

        if resp.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !resp.status().is_success() {
            return Err(format!("Nextcloud download HTTP {}", resp.status()));
        }
        let env: SyncEnvelope = read_json(resp).await?;
        Ok(Some(env))
    }

    async fn nextcloud_upload(
        &self,
        cfg: &NextcloudSyncConfig,
        envelope: &SyncEnvelope,
    ) -> Result<(), String> {
        self.ensure_nextcloud_parent_dirs(cfg).await?;
        let file_url = nextcloud_webdav_file_url(&cfg.server_url, &cfg.username, &cfg.remote_path);
        let auth = format!(
            "Basic {}",
            base64_encode(format!("{}:{}", cfg.username, cfg.password).as_bytes())
        );
        let body = serde_json::to_vec_pretty(envelope).map_err(|e| e.to_string())?;
        let resp = self
            .client
            .put(&file_url)
            .header(AUTHORIZATION, auth)
            .header(CONTENT_TYPE, "application/json")
            .body(body)
            .send()
            .await
            .map_err(|e| format!("Nextcloud upload failed: {e}"))?;

        if !resp.status().is_success() {
            return Err(format!("Nextcloud upload HTTP {}", resp.status()));
        }
        Ok(())
    }

    pub async fn connect_google_drive(&self) -> Result<SyncStatus, String> {
        let (client_id, client_secret) = google_credentials()?;
        let verifier = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        let state_token = uuid::Uuid::new_v4().simple().to_string();

        #[cfg(target_os = "ios")]
        let (redirect_uri, code) = {
            let callback_scheme = google_ios_callback_scheme(&client_id);
            let redirect_uri = format!("{callback_scheme}:/oauth2redirect");
            let auth_url = build_google_auth_url(&client_id, &redirect_uri, &verifier, &state_token)?;
            let callback_url = self
                .open_oauth_session(auth_url.as_str(), &callback_scheme)
                .await?;
            let code = extract_oauth_code_from_url(&callback_url, &state_token)?;
            (redirect_uri, code)
        };

        #[cfg(not(target_os = "ios"))]
        let (redirect_uri, code) = {
            let listener = TcpListener::bind("127.0.0.1:0")
                .await
                .map_err(|e| format!("Failed to bind local OAuth callback port: {e}"))?;
            let port = listener
                .local_addr()
                .map_err(|e| format!("Failed to read callback port: {e}"))?
                .port();

            let redirect_uri = format!("http://127.0.0.1:{port}");
            let auth_url = build_google_auth_url(&client_id, &redirect_uri, &verifier, &state_token)?;
            self.open_browser(auth_url.as_str())?;

            // Wait up to 3 minutes for the OAuth callback
            let code = tokio::time::timeout(
                Duration::from_secs(180),
                wait_for_oauth_code(listener, &state_token),
            )
            .await
            .map_err(|_| "Google sign-in timed out.".to_string())??;
            (redirect_uri, code)
        };

        #[derive(Deserialize)]
        struct TokenResponse {
            access_token: String,
            refresh_token: Option<String>,
            expires_in: Option<u64>,
        }

        let mut form: Vec<(&str, &str)> = vec![
            ("code", code.as_str()),
            ("client_id", client_id.as_str()),
            ("redirect_uri", redirect_uri.as_str()),
            ("grant_type", "authorization_code"),
            ("code_verifier", verifier.as_str()),
        ];
        if let Some(secret) = client_secret.as_deref() {
            form.push(("client_secret", secret));
        }

        let token_resp = self
            .client
            .post(GOOGLE_TOKEN_URL)
            .form(&form)
            .send()
            .await
            .map_err(|e| format!("Google token exchange failed: {e}"))?;

        if !token_resp.status().is_success() {
            let text = token_resp.text().await.unwrap_or_default();
            return Err(format!("Google token exchange error: {text}"));
        }

        let tokens: TokenResponse = read_json(token_resp).await?;

        let expires_at = unix_seconds_now() + tokens.expires_in.unwrap_or(3600);
        let refresh_token = tokens
            .refresh_token
            .ok_or_else(|| "Google did not return a refresh token.".to_string())?;

        let email = self.fetch_google_user_email(&tokens.access_token).await.ok();

        let cfg = GoogleDriveSyncConfig {
            access_token: tokens.access_token,
            refresh_token,
            expires_at,
            file_id: None,
            email,
        };

        self.state
            .update_sync_state(|s| {
                s.provider = SyncProvider::GoogleDrive;
                s.google_drive = Some(cfg);
                s.nextcloud = None;
                s.last_synced_at = None;
                s.last_error = None;
            })
            .map_err(|e| e.to_string())?;

        self.sync_now().await
    }

    async fn fetch_google_user_email(&self, access_token: &str) -> Result<String, String> {
        #[derive(Deserialize)]
        struct AboutUser {
            #[serde(rename = "emailAddress")]
            email_address: Option<String>,
        }
        #[derive(Deserialize)]
        struct AboutResp {
            user: Option<AboutUser>,
        }
        let resp = self
            .client
            .get("https://www.googleapis.com/drive/v3/about?fields=user(emailAddress)")
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("HTTP {}", resp.status()));
        }
        let parsed: AboutResp = read_json(resp).await?;
        parsed
            .user
            .and_then(|u| u.email_address)
            .ok_or_else(|| "No email returned".to_string())
    }

    async fn ensure_google_token(&self) -> Result<GoogleDriveSyncConfig, String> {
        let sync = self.state.get_sync_state().map_err(|e| e.to_string())?;
        let mut cfg = sync
            .google_drive
            .ok_or_else(|| "Google Drive is not connected.".to_string())?;

        if unix_seconds_now() + 60 < cfg.expires_at {
            return Ok(cfg);
        }

        let (client_id, client_secret) = google_credentials()?;

        #[derive(Deserialize)]
        struct RefreshResponse {
            access_token: String,
            expires_in: Option<u64>,
        }

        let mut form: Vec<(&str, &str)> = vec![
            ("client_id", client_id.as_str()),
            ("refresh_token", cfg.refresh_token.as_str()),
            ("grant_type", "refresh_token"),
        ];
        if let Some(secret) = client_secret.as_deref() {
            form.push(("client_secret", secret));
        }

        let resp = self
            .client
            .post(GOOGLE_TOKEN_URL)
            .form(&form)
            .send()
            .await
            .map_err(|e| format!("Google token refresh failed: {e}"))?;

        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("Google token refresh error: {text}"));
        }

        let refreshed: RefreshResponse = read_json(resp).await?;

        cfg.access_token = refreshed.access_token;
        cfg.expires_at = unix_seconds_now() + refreshed.expires_in.unwrap_or(3600);
        let saved = cfg.clone();
        self.state
            .update_sync_state(|s| {
                s.google_drive = Some(saved);
            })
            .map_err(|e| e.to_string())?;
        Ok(cfg)
    }

    async fn google_find_file_id(&self, access_token: &str) -> Result<Option<String>, String> {
        #[derive(Deserialize)]
        struct DriveFile {
            id: String,
        }
        #[derive(Deserialize)]
        struct FileList {
            files: Vec<DriveFile>,
        }
        let mut url = url::Url::parse("https://www.googleapis.com/drive/v3/files")
            .map_err(|e| e.to_string())?;
        let q = format!("name='{SYNC_FILE_NAME}' and trashed=false");
        url.query_pairs_mut()
            .append_pair("q", &q)
            .append_pair("spaces", "drive")
            .append_pair("fields", "files(id,name,modifiedTime)")
            .append_pair("orderBy", "modifiedTime desc");

        let resp = self
            .client
            .get(url)
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(|e| format!("Google Drive list failed: {e}"))?;

        if !resp.status().is_success() {
            return Err(format!("Google Drive list HTTP {}", resp.status()));
        }
        let list: FileList = read_json(resp).await?;
        Ok(list.files.into_iter().next().map(|f| f.id))
    }

    async fn google_download(
        &self,
        cfg: &mut GoogleDriveSyncConfig,
    ) -> Result<Option<SyncEnvelope>, String> {
        let file_id = match cfg.file_id.clone() {
            Some(id) => Some(id),
            None => self.google_find_file_id(&cfg.access_token).await?,
        };
        let Some(id) = file_id else {
            return Ok(None);
        };
        cfg.file_id = Some(id.clone());

        let url = format!("https://www.googleapis.com/drive/v3/files/{id}?alt=media");
        let resp = self
            .client
            .get(&url)
            .bearer_auth(&cfg.access_token)
            .send()
            .await
            .map_err(|e| format!("Google Drive download failed: {e}"))?;

        if resp.status() == StatusCode::NOT_FOUND {
            cfg.file_id = None;
            if let Some(found) = self.google_find_file_id(&cfg.access_token).await? {
                cfg.file_id = Some(found.clone());
                let retry_url =
                    format!("https://www.googleapis.com/drive/v3/files/{found}?alt=media");
                let retry = self
                    .client
                    .get(&retry_url)
                    .bearer_auth(&cfg.access_token)
                    .send()
                    .await
                    .map_err(|e| format!("Google Drive download failed: {e}"))?;
                if !retry.status().is_success() {
                    return Err(format!("Google Drive download HTTP {}", retry.status()));
                }
                let env: SyncEnvelope = read_json(retry).await?;
                return Ok(Some(env));
            }
            return Ok(None);
        }

        if !resp.status().is_success() {
            return Err(format!("Google Drive download HTTP {}", resp.status()));
        }
        let env: SyncEnvelope = read_json(resp).await?;
        Ok(Some(env))
    }

    async fn google_upload(
        &self,
        cfg: &mut GoogleDriveSyncConfig,
        envelope: &SyncEnvelope,
    ) -> Result<(), String> {
        let json_body = serde_json::to_string_pretty(envelope).map_err(|e| e.to_string())?;
        if let Some(id) = cfg.file_id.as_deref() {
            let url = format!("https://www.googleapis.com/upload/drive/v3/files/{id}?uploadType=media");
            let resp = self
                .client
                .patch(&url)
                .bearer_auth(&cfg.access_token)
                .header(CONTENT_TYPE, "application/json")
                .body(json_body)
                .send()
                .await
                .map_err(|e| format!("Google Drive update failed: {e}"))?;
            if !resp.status().is_success() {
                return Err(format!("Google Drive update HTTP {}", resp.status()));
            }
            return Ok(());
        }

        // Create new file with multipart/related
        let boundary = format!("wol_sync_{}", uuid::Uuid::new_v4().simple());
        let meta = json!({
            "name": SYNC_FILE_NAME,
            "mimeType": "application/json"
        });
        let multipart_body = format!(
            "--{boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n{meta}\r\n--{boundary}\r\nContent-Type: application/json\r\n\r\n{json_body}\r\n--{boundary}--"
        );
        let resp = self
            .client
            .post("https://www.googleapis.com/upload/drive/v3/files?uploadType=multipart&fields=id")
            .bearer_auth(&cfg.access_token)
            .header(
                CONTENT_TYPE,
                format!("multipart/related; boundary={boundary}"),
            )
            .body(multipart_body)
            .send()
            .await
            .map_err(|e| format!("Google Drive create failed: {e}"))?;

        if !resp.status().is_success() {
            return Err(format!("Google Drive create HTTP {}", resp.status()));
        }

        #[derive(Deserialize)]
        struct CreatedFile {
            id: String,
        }
        if let Ok(created) = read_json::<CreatedFile>(resp).await {
            cfg.file_id = Some(created.id);
        }
        Ok(())
    }

    pub async fn sync_now(&self) -> Result<SyncStatus, String> {
        let _guard = self.sync_lock.lock().await;
        self.syncing.store(true, Ordering::Relaxed);
        crate::log_line!("[CloudSync] Starting sync");
        let result = self.sync_now_inner().await;
        self.syncing.store(false, Ordering::Relaxed);
        match &result {
            Ok(status) => crate::log_line!("[CloudSync] Finished: changed_local={}", status.changed_local),
            Err(err) => {
                crate::log_line!("[CloudSync] Failed: {err}");
                let _ = self.state.update_sync_state(|s| s.last_error = Some(err.clone()));
            }
        }
        result
    }

    async fn sync_now_inner(&self) -> Result<SyncStatus, String> {
        let sync_state = self.state.get_sync_state().map_err(|e| e.to_string())?;
        if sync_state.provider == SyncProvider::None {
            return Ok(self.build_status(&sync_state, false));
        }

        let res: Result<bool, String> = match sync_state.provider {
            SyncProvider::None => Ok(false),
            SyncProvider::Nextcloud => {
                let cfg = sync_state
                    .nextcloud
                    .clone()
                    .ok_or_else(|| "Nextcloud is not configured.".to_string())?;
                let remote = self.nextcloud_download(&cfg).await?;
                self.reconcile_and_apply(
                    &sync_state,
                    remote,
                    |env| {
                        let cfg = cfg.clone();
                        let mgr = self.clone();
                        async move { mgr.nextcloud_upload(&cfg, &env).await }
                    },
                )
                .await
            }
            SyncProvider::GoogleDrive => {
                let mut cfg = self.ensure_google_token().await?;
                let remote = self.google_download(&mut cfg).await?;
                let changed = self
                    .reconcile_and_apply(&sync_state, remote, |env| {
                        let mgr = self.clone();
                        let mut cfg_up = cfg.clone();
                        async move {
                            mgr.google_upload(&mut cfg_up, &env).await?;
                            let _ = mgr.state.update_sync_state(|s| {
                                s.google_drive = Some(cfg_up);
                            });
                            Ok(())
                        }
                    })
                    .await?;
                if let Some(fid) = cfg.file_id.clone() {
                    let _ = self.state.update_sync_state(|s| {
                        if let Some(gd) = s.google_drive.as_mut() {
                            if gd.file_id.is_none() {
                                gd.file_id = Some(fid);
                            }
                        }
                    });
                }
                Ok(changed)
            }
        };

        match res {
            Ok(changed_local) => {
                if changed_local {
                    self.notify_listener();
                }
                let updated = self.state.get_sync_state().map_err(|e| e.to_string())?;
                Ok(self.build_status(&updated, changed_local))
            }
            Err(err) => {
                let _ = self.state.update_sync_state(|s| {
                    s.last_error = Some(err.clone());
                });
                Err(err)
            }
        }
    }

    async fn reconcile_and_apply<F, Fut>(
        &self,
        sync_state: &SyncState,
        remote: Option<SyncEnvelope>,
        upload: F,
    ) -> Result<bool, String>
    where
        F: FnOnce(SyncEnvelope) -> Fut,
        Fut: std::future::Future<Output = Result<(), String>>,
    {
        let local_marks = self.state.get_marks().map_err(|e| e.to_string())?;
        let local_has_data = marks_has_content(local_marks.as_ref());
        let remote_updated = remote.as_ref().map(|r| r.updated_at.as_str());

        let action = decide_sync_action(
            local_has_data,
            sync_state.local_updated_at.as_deref(),
            sync_state.last_synced_at.as_deref(),
            remote_updated,
        );

        crate::log_line!("[CloudSync] Decision: {:?}, local_data={}, remote_present={}", action, local_has_data, remote.is_some());
        match action {
            SyncAction::NoOp => {
                let now = now_iso_utc();
                self.state
                    .update_sync_state(|s| {
                        s.last_synced_at = Some(now);
                        s.last_error = None;
                    })
                    .map_err(|e| e.to_string())?;
                Ok(false)
            }
            SyncAction::PullRemote => {
                if let Some(rem) = remote {
                    self.state
                        .set_marks_from_sync(rem.wol_marks, rem.updated_at)
                        .map_err(|e| e.to_string())?;
                    return Ok(true);
                }
                Ok(false)
            }
            SyncAction::PushLocal => {
                let now = now_iso_utc();
                let env = SyncEnvelope {
                    format: "wol-highlights-sync".to_string(),
                    version: 1,
                    updated_at: now.clone(),
                    device_id: sync_state.device_id.clone(),
                    wol_marks: local_marks,
                };
                upload(env).await?;
                self.state
                    .update_sync_state(|s| {
                        s.local_updated_at = Some(now.clone());
                        s.last_synced_at = Some(now);
                        s.last_error = None;
                    })
                    .map_err(|e| e.to_string())?;
                Ok(false)
            }
            SyncAction::MergeAndPush => {
                let remote_marks = remote.as_ref().and_then(|r| r.wol_marks.as_ref());
                let merged = merge_marks(local_marks.as_ref(), remote_marks);
                let now = now_iso_utc();
                let env = SyncEnvelope {
                    format: "wol-highlights-sync".to_string(),
                    version: 1,
                    updated_at: now.clone(),
                    device_id: sync_state.device_id.clone(),
                    wol_marks: merged.clone(),
                };
                upload(env).await?;
                self.state
                    .set_marks_from_sync(merged, now)
                    .map_err(|e| e.to_string())?;
                Ok(true)
            }
        }
    }
}

fn build_google_auth_url(
    client_id: &str,
    redirect_uri: &str,
    verifier: &str,
    state_token: &str,
) -> Result<url::Url, String> {
    let challenge = pkce_challenge(verifier);
    let mut auth_url =
        url::Url::parse(GOOGLE_AUTH_URL).map_err(|e| format!("Invalid auth URL: {e}"))?;
    auth_url
        .query_pairs_mut()
        .append_pair("client_id", client_id)
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("response_type", "code")
        .append_pair("scope", GOOGLE_DRIVE_SCOPE)
        .append_pair("access_type", "offline")
        .append_pair("prompt", "consent")
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", state_token);
    Ok(auth_url)
}

#[cfg(target_os = "ios")]
fn google_ios_callback_scheme(client_id: &str) -> String {
    if let Ok(scheme) = std::env::var("WOL_GOOGLE_IOS_SCHEME") {
        let trimmed = scheme.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    if let Some(prefix) = client_id.strip_suffix(".apps.googleusercontent.com") {
        return format!("com.googleusercontent.apps.{prefix}");
    }
    "com.offlinewol.desktop".to_string()
}

#[cfg(target_os = "ios")]
fn extract_oauth_code_from_url(callback_url: &str, expected_state: &str) -> Result<String, String> {
    let parsed =
        url::Url::parse(callback_url).map_err(|e| format!("Invalid OAuth callback URL: {e}"))?;
    let params: HashMap<String, String> = parsed.query_pairs().into_owned().collect();

    if let Some(err) = params.get("error") {
        return Err(format!("Google sign-in failed: {err}"));
    }
    let state = params.get("state").map(String::as_str).unwrap_or("");
    if state != expected_state {
        return Err("OAuth state mismatch.".to_string());
    }
    params
        .get("code")
        .cloned()
        .ok_or_else(|| "OAuth callback missing code.".to_string())
}

#[cfg(not(target_os = "ios"))]
async fn wait_for_oauth_code(listener: TcpListener, expected_state: &str) -> Result<String, String> {
    loop {
        let (mut socket, _) = listener
            .accept()
            .await
            .map_err(|e| format!("OAuth callback accept failed: {e}"))?;
        let mut buf = [0u8; 4096];
        let n = socket
            .read(&mut buf)
            .await
            .map_err(|e| format!("OAuth callback read failed: {e}"))?;
        let req = String::from_utf8_lossy(&buf[..n]);
        let first_line = req.lines().next().unwrap_or("");
        let path = first_line.split_whitespace().nth(1).unwrap_or("/");

        if path.starts_with("/favicon") {
            let _ = socket
                .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n")
                .await;
            continue;
        }

        let parsed = url::Url::parse(&format!("http://127.0.0.1{path}"))
            .map_err(|e| format!("Invalid OAuth callback URL: {e}"))?;
        let params: HashMap<String, String> = parsed.query_pairs().into_owned().collect();

        if let Some(err) = params.get("error") {
            let html = format!(
                "<html><body style='font-family:system-ui;padding:40px;text-align:center'><h2>Sign-in Cancelled</h2><p>{err}</p><p>You can close this tab.</p></body></html>"
            );
            let resp = format!(
                "HTTP/1.1 400 Bad Request\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\r\n{html}",
                html.len()
            );
            let _ = socket.write_all(resp.as_bytes()).await;
            return Err(format!("Google sign-in failed: {err}"));
        }

        let state = params.get("state").map(String::as_str).unwrap_or("");
        if state != expected_state {
            let _ = socket
                .write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n")
                .await;
            return Err("OAuth state mismatch.".to_string());
        }

        let code = params
            .get("code")
            .cloned()
            .ok_or_else(|| "OAuth callback missing code.".to_string())?;

        let html = "<html><body style='font-family:system-ui;padding:40px;text-align:center'><h2>OfflineWOL Connected</h2><p>Google Drive sync is ready. You can close this browser tab and return to OfflineWOL.</p></body></html>";
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\r\n{html}",
            html.len()
        );
        let _ = socket.write_all(resp.as_bytes()).await;
        return Ok(code);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_and_base64_work() {
        assert_eq!(base64_encode(b"user:pass"), "dXNlcjpwYXNz");
        let challenge = pkce_challenge("test-verifier");
        assert!(!challenge.is_empty());
        assert!(!challenge.contains('='));
        assert!(!challenge.contains('+'));
        assert!(!challenge.contains('/'));
    }

    #[test]
    fn normalizes_nextcloud_urls() {
        assert_eq!(
            normalize_nextcloud_server_url("cloud.example.com/").unwrap(),
            "https://cloud.example.com"
        );
        assert_eq!(
            normalize_nextcloud_server_url("http://localhost:8080/nextcloud/").unwrap(),
            "http://localhost:8080/nextcloud"
        );
        assert_eq!(
            nextcloud_webdav_file_url(
                "https://cloud.example.com",
                "daniel@home",
                "OfflineWOL/wol-highlights-sync.json"
            ),
            "https://cloud.example.com/remote.php/dav/files/daniel%40home/OfflineWOL/wol-highlights-sync.json"
        );
    }

    #[test]
    fn sync_decision_matrix() {
        assert_eq!(decide_sync_action(true, None, None, None), SyncAction::PushLocal);
        assert_eq!(decide_sync_action(true, None, Some("2026-11-13T10:00:00Z"), None), SyncAction::PushLocal);
        assert_eq!(decide_sync_action(true, None, None, Some("2026-11-13T10:00:00Z")), SyncAction::MergeAndPush);
        assert_eq!(
            decide_sync_action(false, None, None, Some("2026-11-13T10:00:00Z")),
            SyncAction::PullRemote
        );
        assert_eq!(
            decide_sync_action(true, Some("2026-11-13T10:00:00Z"), None, None),
            SyncAction::PushLocal
        );
        assert_eq!(
            decide_sync_action(
                true,
                Some("2026-11-13T10:00:00Z"),
                Some("2026-11-13T10:00:00Z"),
                Some("2026-11-13T11:00:00Z")
            ),
            SyncAction::PullRemote
        );
        assert_eq!(
            decide_sync_action(
                true,
                Some("2026-11-13T12:00:00Z"),
                Some("2026-11-13T10:00:00Z"),
                Some("2026-11-13T11:00:00Z")
            ),
            SyncAction::MergeAndPush
        );
        assert_eq!(
            decide_sync_action(
                true,
                Some("2026-11-13T10:00:00Z"),
                Some("2026-11-13T10:00:00Z"),
                Some("2026-11-13T10:00:00Z")
            ),
            SyncAction::NoOp
        );
    }

    #[test]
    fn merge_marks_combines_highlights_notes_and_inputs() {
        let local = json!({
            "version": 1,
            "docs": {
                "100": { "2": [[0, 3, 1]] }
            },
            "bible": {},
            "notes": {
                "docs": {
                    "100": {
                        "2": [{ "guid": "n1", "title": "Local", "content": "Hello", "lastModified": "2026-11-13T10:00:00Z" }]
                    }
                },
                "bible": {}
            },
            "inputFields": {
                "docs": {
                    "100": { "q1": "Answer A" }
                },
                "bible": {}
            }
        });
        let remote = json!({
            "version": 1,
            "docs": {
                "100": { "2": [[0, 3, 2], [5, 8, 3]] },
                "200": { "4": [[1, 2, 1]] }
            },
            "bible": {},
            "notes": {
                "docs": {
                    "100": {
                        "2": [{ "guid": "n1", "title": "Remote", "content": "Updated", "lastModified": "2026-11-13T12:00:00Z" }]
                    }
                },
                "bible": {}
            },
            "inputFields": {
                "docs": {
                    "200": { "q2": "Answer B" }
                },
                "bible": {}
            }
        });

        let merged = merge_marks(Some(&local), Some(&remote)).unwrap();
        assert_eq!(merged["highlightCount"], 3);
        assert_eq!(merged["noteCount"], 3); // 1 merged note + 2 inputFields
        assert_eq!(merged["documentCount"], 2);
        assert_eq!(merged["notes"]["docs"]["100"]["2"][0]["title"], "Remote");
        assert_eq!(merged["inputFields"]["docs"]["100"]["q1"], "Answer A");
        assert_eq!(merged["inputFields"]["docs"]["200"]["q2"], "Answer B");
    }

    #[test]
    fn custom_browser_opener_receives_url() {
        let dir = std::env::temp_dir().join(format!("wol-sync-opener-{}", uuid::Uuid::new_v4()));
        let state = StateManager::load_or_init(&dir).expect("init state");
        let mgr = SyncManager::new(state);
        let seen = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let seen_clone = seen.clone();
        mgr.set_browser_opener(Some(Arc::new(move |url| {
            seen_clone.lock().unwrap().push(url.to_string());
            Ok(())
        })));
        mgr.open_browser("https://example.com/auth").expect("opener succeeds");
        assert_eq!(&*seen.lock().unwrap(), &["https://example.com/auth".to_string()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn fake_env(pairs: Vec<(&'static str, &'static str)>) -> impl Fn(&str) -> Option<String> {
        move |key: &str| {
            pairs
                .iter()
                .find(|(candidate, _)| *candidate == key)
                .map(|(_, value)| (*value).to_string())
        }
    }

    #[test]
    fn google_credentials_prefer_environment_over_builtin_client() {
        let env = fake_env(vec![
            ("WOL_GOOGLE_CLIENT_ID", "env-id.apps.googleusercontent.com"),
            ("WOL_GOOGLE_CLIENT_SECRET", "env-secret"),
        ]);
        let (id, secret) = google_credentials_from(env, "builtin-id", "builtin-secret", true)
            .expect("env credentials resolve");
        assert_eq!(id, "env-id.apps.googleusercontent.com");
        assert_eq!(secret.as_deref(), Some("env-secret"));
    }

    #[test]
    fn google_credentials_fall_back_to_the_bundled_client() {
        let env = fake_env(vec![("WOL_GOOGLE_CLIENT_ID", "   ")]);
        let (id, secret) =
            google_credentials_from(env, " builtin-id ", "builtin-secret", true).expect("builtin resolves");
        assert_eq!(id, "builtin-id");
        assert_eq!(secret.as_deref(), Some("builtin-secret"));
    }

    #[test]
    fn google_credentials_report_missing_client() {
        let env = fake_env(vec![]);
        let err = google_credentials_from(env, "", "", true).expect_err("no credentials configured");
        assert!(err.contains("Google Drive sign-in is not configured in this build"), "{err}");
    }

    #[test]
    fn google_credentials_allow_public_client_without_secret_on_ios() {
        let env = fake_env(vec![
            ("WOL_GOOGLE_CLIENT_ID", "ios-id.apps.googleusercontent.com"),
            ("WOL_GOOGLE_CLIENT_SECRET", ""),
        ]);
        let (id, secret) = google_credentials_from(env, "", "", false).expect("public client resolves");
        assert_eq!(id, "ios-id.apps.googleusercontent.com");
        assert_eq!(secret, None);
    }
}
