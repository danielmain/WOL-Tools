//! `wol-core` — GUI-agnostic engine shared by the Tauri desktop app
//! ("OfflineWOL") and the pure-terminal `wol-cli`.
//!
//! Nothing in this crate depends on Tauri, a windowing system, or a terminal;
//! only tokio/reqwest/rusqlite/serde and friends. That keeps it wrappable by
//! UniFFI (Swift/Kotlin), Tauri Mobile, or any future front-end.

pub mod backup;
pub mod cache;
pub mod crawler;
pub mod dirs;
pub mod dom;
pub mod handler;
pub mod highlight;
pub mod http_types;
pub mod jwlibrary_export;
pub mod languages;
pub mod publications;
pub mod rewrite;
pub mod state;
pub mod sync;
pub mod tokenize;
pub mod url_map;

#[doc(hidden)]
#[macro_export]
macro_rules! log_line {
    ($($arg:tt)*) => {
        if $crate::logging::enabled() {
            eprintln!($($arg)*);
        }
    };
}

pub mod logging {
    use std::sync::atomic::{AtomicBool, Ordering};

    static ENABLED: AtomicBool = AtomicBool::new(true);

    pub fn enabled() -> bool {
        ENABLED.load(Ordering::Relaxed)
    }

    pub fn set_enabled(value: bool) {
        ENABLED.store(value, Ordering::Relaxed);
    }
}

pub use cache::{CacheEntry, DiskCache};
pub use backup::{import_backup, BackupError, ImportFormat, ImportSource, Span, StoredIndex};
pub use dom::{Block, BlockKind, Document};
pub use state::{AppSettings, StateManager};

/// Default application-data directory (`~/Library/Application Support/com.offlinewol.desktop`
/// on macOS). Renames the legacy `com.wolhighlights.desktop` directory on first use.
pub fn dirs_fallback_app_data() -> std::path::PathBuf {
    dirs::app_data_dir()
}
