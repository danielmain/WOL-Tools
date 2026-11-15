//! Application data locations shared by every front-end.

use std::path::PathBuf;

pub const APP_ID: &str = "com.offlinewol.desktop";
pub const LEGACY_APP_ID: &str = "com.wolhighlights.desktop";

/// Resolve the app-data directory, migrating the legacy bundle identifier.
pub fn app_data_dir() -> PathBuf {
    if let Some(home) = std::env::var_os("HOME") {
        let app_support = PathBuf::from(home)
            .join("Library")
            .join("Application Support");
        let new_dir = app_support.join(APP_ID);
        let legacy_dir = app_support.join(LEGACY_APP_ID);
        if !new_dir.exists() && legacy_dir.exists() {
            let _ = std::fs::rename(&legacy_dir, &new_dir);
        }
        new_dir
    } else {
        PathBuf::from(".offline-wol-data")
    }
}

/// Allow every front-end to be pointed at an alternate data directory
/// (`WOL_DATA_DIR`), which keeps tests and the CLI hermetic.
pub fn app_data_dir_or_env() -> PathBuf {
    match std::env::var_os("WOL_DATA_DIR") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => app_data_dir(),
    }
}
