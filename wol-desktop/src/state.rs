use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppStateData {
    pub version: u32,
    pub settings: AppSettings,
    #[serde(rename = "wolMarks")]
    pub wol_marks: Option<Value>,
    #[serde(default = "default_cookies")]
    pub cookies: std::collections::HashMap<String, String>,
}

impl Default for AppStateData {
    fn default() -> Self {
        Self {
            version: 1,
            settings: AppSettings::default(),
            wol_marks: None,
            cookies: default_cookies(),
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
        self.save_locked(&lock)?;
        Ok(())
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
