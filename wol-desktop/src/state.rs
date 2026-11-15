use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StateError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("lock poisoned")]
    LockPoisoned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    #[default]
    System,
    Light,
    Dark,
}

impl ThemeMode {
    pub fn as_str(self) -> &'static str {
        match self {
            ThemeMode::System => "system",
            ThemeMode::Light => "light",
            ThemeMode::Dark => "dark",
        }
    }

    pub fn to_tauri_theme(self) -> Option<tauri::Theme> {
        match self {
            ThemeMode::System => None,
            ThemeMode::Light => Some(tauri::Theme::Light),
            ThemeMode::Dark => Some(tauri::Theme::Dark),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    #[serde(default, rename = "baseHost")]
    pub base_host: Option<String>,
    pub language: String,
    pub region: u32,
    pub home: String,
    #[serde(rename = "cacheCapBytes")]
    pub cache_cap_bytes: u64,
    #[serde(default)]
    pub theme: ThemeMode,
}

impl AppSettings {
    pub fn is_configured(&self) -> bool {
        self.base_host
            .as_deref()
            .is_some_and(|h| !h.trim().is_empty())
            && !self.home.trim().is_empty()
            && !self.language.trim().is_empty()
    }
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            base_host: None,
            language: String::new(),
            region: 0,
            home: String::new(),
            cache_cap_bytes: 2 * 1024 * 1024 * 1024,
            theme: ThemeMode::default(),
        }
    }
}

pub fn default_cookies() -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    map.insert("cookieConsent-STRICTLY_NECESSARY".to_string(), "true".to_string());
    map.insert("cookieConsent-FUNCTIONAL".to_string(), "true".to_string());
    map.insert("cookieConsent-DIAGNOSTIC".to_string(), "true".to_string());
    map.insert("cookieConsent-USAGE".to_string(), "true".to_string());
    map
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SyncProvider {
    #[default]
    None,
    Nextcloud,
    #[serde(rename = "googledrive")]
    GoogleDrive,
}

fn default_remote_path() -> String {
    "OfflineWOL/wol-highlights-sync.json".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct NextcloudSyncConfig {
    pub server_url: String,
    pub username: String,
    pub password: String,
    #[serde(default = "default_remote_path")]
    pub remote_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GoogleDriveSyncConfig {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: u64,
    #[serde(default)]
    pub file_id: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncState {
    #[serde(default)]
    pub provider: SyncProvider,
    #[serde(default)]
    pub device_id: String,
    #[serde(default)]
    pub local_updated_at: Option<String>,
    #[serde(default)]
    pub last_synced_at: Option<String>,
    #[serde(default)]
    pub last_error: Option<String>,
    #[serde(default)]
    pub nextcloud: Option<NextcloudSyncConfig>,
    #[serde(default)]
    pub google_drive: Option<GoogleDriveSyncConfig>,
}

impl Default for SyncState {
    fn default() -> Self {
        Self {
            provider: SyncProvider::None,
            device_id: uuid::Uuid::new_v4().to_string(),
            local_updated_at: None,
            last_synced_at: None,
            last_error: None,
            nextcloud: None,
            google_drive: None,
        }
    }
}

pub fn unix_seconds_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn now_iso_utc() -> String {
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    let millis = dur.subsec_millis();
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let hour = rem / 3_600;
    let minute = (rem % 3_600) / 60;
    let second = rem % 60;

    // Howard Hinnant's civil_from_days algorithm (UTC)
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };

    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppStateData {
    pub version: u32,
    pub settings: AppSettings,
    #[serde(rename = "wolMarks")]
    pub wol_marks: Option<Value>,
    #[serde(default = "default_cookies")]
    pub cookies: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub sync: SyncState,
}

impl Default for AppStateData {
    fn default() -> Self {
        Self {
            version: 1,
            settings: AppSettings::default(),
            wol_marks: None,
            cookies: default_cookies(),
            sync: SyncState::default(),
        }
    }
}

#[derive(Clone)]
pub struct StateManager {
    path: PathBuf,
    data: Arc<Mutex<AppStateData>>,
}

impl StateManager {
    pub fn load_or_init(app_data_dir: &Path) -> Result<Self, StateError> {
        fs::create_dir_all(app_data_dir)?;
        let path = app_data_dir.join("state.json");

        let mut data = if path.exists() {
            let bytes = fs::read(&path)?;
            serde_json::from_slice(&bytes).unwrap_or_else(|_| AppStateData::default())
        } else {
            let default_data = AppStateData::default();
            let json_bytes = serde_json::to_vec_pretty(&default_data)?;
            let tmp_path = app_data_dir.join(format!("state.json.tmp.{}", std::process::id()));
            {
                let mut f = File::create(&tmp_path)?;
                f.write_all(&json_bytes)?;
                f.sync_all()?;
            }
            fs::rename(&tmp_path, &path)?;
            default_data
        };

        if data.cookies.is_empty() {
            data.cookies = default_cookies();
        }
        if data.sync.device_id.trim().is_empty() {
            data.sync.device_id = uuid::Uuid::new_v4().to_string();
        }

        // Backfill base_host for legacy state.json files that already have a full home URL.
        if data.settings.base_host.is_none() && !data.settings.home.is_empty() {
            if let Ok(parsed) = url::Url::parse(&data.settings.home) {
                if let Some(host) = parsed.host_str() {
                    data.settings.base_host = Some(host.to_string());
                }
            }
        }

        Ok(Self {
            path,
            data: Arc::new(Mutex::new(data)),
        })
    }

    pub fn get_cookies(&self) -> Result<std::collections::HashMap<String, String>, StateError> {
        let lock = self.data.lock().map_err(|_| StateError::LockPoisoned)?;
        Ok(lock.cookies.clone())
    }

    pub fn set_cookies(
        &self,
        cookies: std::collections::HashMap<String, String>,
    ) -> Result<(), StateError> {
        let mut lock = self.data.lock().map_err(|_| StateError::LockPoisoned)?;
        lock.cookies = cookies;
        self.save_locked(&lock)?;
        Ok(())
    }

    pub fn update_cookie(&self, name: String, value: Option<String>) -> Result<(), StateError> {
        let mut lock = self.data.lock().map_err(|_| StateError::LockPoisoned)?;
        if let Some(val) = value {
            lock.cookies.insert(name, val);
        } else {
            lock.cookies.remove(&name);
        }
        self.save_locked(&lock)?;
        Ok(())
    }

    pub fn get_marks(&self) -> Result<Option<Value>, StateError> {
        let lock = self.data.lock().map_err(|_| StateError::LockPoisoned)?;
        Ok(lock.wol_marks.clone())
    }

    pub fn set_marks(&self, marks: Option<Value>) -> Result<(), StateError> {
        let mut lock = self.data.lock().map_err(|_| StateError::LockPoisoned)?;
        lock.wol_marks = marks;
        lock.sync.local_updated_at = Some(now_iso_utc());
        self.save_locked(&lock)?;
        Ok(())
    }

    pub fn set_marks_from_sync(
        &self,
        marks: Option<Value>,
        synced_at: String,
    ) -> Result<(), StateError> {
        let mut lock = self.data.lock().map_err(|_| StateError::LockPoisoned)?;
        lock.wol_marks = marks;
        lock.sync.local_updated_at = Some(synced_at.clone());
        lock.sync.last_synced_at = Some(synced_at);
        lock.sync.last_error = None;
        self.save_locked(&lock)?;
        Ok(())
    }

    pub fn get_sync_state(&self) -> Result<SyncState, StateError> {
        let lock = self.data.lock().map_err(|_| StateError::LockPoisoned)?;
        Ok(lock.sync.clone())
    }

    pub fn update_sync_state<F>(&self, f: F) -> Result<SyncState, StateError>
    where
        F: FnOnce(&mut SyncState),
    {
        let mut lock = self.data.lock().map_err(|_| StateError::LockPoisoned)?;
        f(&mut lock.sync);
        self.save_locked(&lock)?;
        Ok(lock.sync.clone())
    }

    pub fn get_settings(&self) -> Result<AppSettings, StateError> {
        let lock = self.data.lock().map_err(|_| StateError::LockPoisoned)?;
        Ok(lock.settings.clone())
    }

    pub fn set_settings(&self, settings: AppSettings) -> Result<(), StateError> {
        let mut lock = self.data.lock().map_err(|_| StateError::LockPoisoned)?;
        lock.settings = settings;
        self.save_locked(&lock)?;
        Ok(())
    }

    fn save_locked(&self, data: &AppStateData) -> Result<(), StateError> {
        let json_bytes = serde_json::to_vec_pretty(data)?;
        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        let tmp_path = parent.join(format!("state.json.tmp.{}", std::process::id()));
        {
            let mut f = File::create(&tmp_path)?;
            f.write_all(&json_bytes)?;
            f.sync_all()?;
        }
        fs::rename(&tmp_path, &self.path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_manager_roundtrip() {
        let temp_dir = std::env::temp_dir().join(format!("wol_state_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_dir);

        let mgr = StateManager::load_or_init(&temp_dir).unwrap();
        assert!(mgr.get_marks().unwrap().is_none());
        let default_ck = mgr.get_cookies().unwrap();
        assert_eq!(default_ck.get("cookieConsent-FUNCTIONAL").unwrap(), "true");

        let sample_marks = serde_json::json!({
            "version": 1,
            "fileName": "sample.jwlibrary",
            "highlightCount": 5
        });

        mgr.set_marks(Some(sample_marks.clone())).unwrap();
        mgr.update_cookie("fontScale".to_string(), Some("1.5".to_string())).unwrap();

        // Reload from disk to verify persistence
        let mgr2 = StateManager::load_or_init(&temp_dir).unwrap();
        let loaded = mgr2.get_marks().unwrap();
        assert_eq!(loaded, Some(sample_marks));
        let loaded_ck = mgr2.get_cookies().unwrap();
        assert_eq!(loaded_ck.get("fontScale").unwrap(), "1.5");
        assert_eq!(loaded_ck.get("cookieConsent-FUNCTIONAL").unwrap(), "true");
        assert_eq!(mgr2.get_settings().unwrap().theme, ThemeMode::System);

        let mut updated_settings = mgr2.get_settings().unwrap();
        updated_settings.theme = ThemeMode::Dark;
        mgr2.set_settings(updated_settings).unwrap();

        let mgr3 = StateManager::load_or_init(&temp_dir).unwrap();
        assert_eq!(mgr3.get_settings().unwrap().theme, ThemeMode::Dark);

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
