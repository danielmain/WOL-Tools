use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Manager, State};
#[cfg(desktop)]
use tauri_plugin_dialog::DialogExt;

use crate::cache::{CacheEntry, DiskCache};
use crate::crawler::{
    BibleCrawlResult, CrawlResult, Crawler, IssueCrawlResult, LibraryCrawlResult,
};
use crate::languages::{self, LanguageInfo};
use crate::state::{AppSettings, StateManager};
use crate::sync::{SyncManager, SyncStatus};
use crate::url_map::{map_wol_uri_to_canonical, CanonicalUrl};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheStats {
    pub total_size: u64,
    pub entry_count: u64,
    pub cap_bytes: u64,
}

#[tauri::command]
pub fn get_marks(state_mgr: State<'_, StateManager>) -> Result<Option<Value>, String> {
    state_mgr.get_marks().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_cookies(
    state_mgr: State<'_, StateManager>,
) -> Result<std::collections::HashMap<String, String>, String> {
    state_mgr.get_cookies().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_cookie(
    name: String,
    value: Option<String>,
    state_mgr: State<'_, StateManager>,
) -> Result<(), String> {
    state_mgr.update_cookie(name, value).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_marks(
    app: AppHandle,
    marks: Option<Value>,
    state_mgr: State<'_, StateManager>,
    sync_mgr: State<'_, Arc<SyncManager>>,
) -> Result<(), String> {
    state_mgr.set_marks(marks).map_err(|e| e.to_string())?;
    sync_mgr.schedule_debounced_sync();

    // Broadcast reapply to the main webview window
    if let Some(main_window) = app.get_webview_window("main") {
        let _ = main_window.eval(
            "if (window.__wolDispatchMessage) { window.__wolDispatchMessage({ type: 'wol-reapply' }); }"
        );
    }

    Ok(())
}

fn marks_has_exportable_data(marks: &Value) -> bool {
    let hl = marks
        .get("highlightCount")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let notes = marks
        .get("noteCount")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    if hl > 0 || notes > 0 {
        return true;
    }
    let has_docs = marks
        .get("docs")
        .and_then(|v| v.as_object())
        .is_some_and(|m| !m.is_empty());
    let has_bible = marks
        .get("bible")
        .and_then(|v| v.as_object())
        .is_some_and(|m| !m.is_empty());
    has_docs || has_bible
}

fn validated_export_marks(state_mgr: &StateManager) -> Result<(Value, i64), String> {
    let marks = state_mgr
        .get_marks()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "No highlights or notes to export.".to_string())?;

    if !marks_has_exportable_data(&marks) {
        return Err("No highlights or notes to export.".to_string());
    }

    let default_meps_lang = state_mgr
        .get_settings()
        .ok()
        .and_then(|s| {
            languages::find_by_locale(&s.language)
                .or_else(|| languages::find_by_symbol(&s.language))
        })
        .map(|lang| match lang.symbol.as_str() {
            "E" => 0,
            "S" => 1,
            _ => 0,
        })
        .unwrap_or(0);

    Ok((marks, default_meps_lang))
}

fn resolve_export_file_name(default_name: Option<String>) -> String {
    default_name
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "UserdataBackup_OfflineWOL.jwlibrary".to_string())
}

#[tauri::command]
pub async fn export_marks_to_file(
    app: AppHandle,
    default_name: Option<String>,
    state_mgr: State<'_, StateManager>,
) -> Result<Option<String>, String> {
    #[cfg(mobile)]
    {
        let _ = (app, default_name, state_mgr);
        return Err("Save dialog is not supported on mobile; use export_marks_to_bytes.".to_string());
    }

    #[cfg(desktop)]
    {
        let (marks, default_meps_lang) = validated_export_marks(&state_mgr)?;
        let file_name = resolve_export_file_name(default_name);

        let (tx, rx) = tokio::sync::oneshot::channel();
        app.dialog()
            .file()
            .set_title("Export JW Library Backup")
            .set_file_name(&file_name)
            .add_filter("JW Library Backup", &["jwlibrary"])
            .save_file(move |file_path| {
                let _ = tx.send(file_path);
            });

        let chosen = rx.await.map_err(|_| "Export dialog closed unexpectedly.".to_string())?;
        let Some(file_path) = chosen else {
            return Ok(None);
        };

        let mut path = file_path
            .into_path()
            .map_err(|e| format!("Invalid destination path: {e}"))?;
        if path.extension().and_then(|e| e.to_str()) != Some("jwlibrary") {
            path.set_extension("jwlibrary");
        }

        crate::jwlibrary_export::export_marks_to_jwlibrary(&marks, &path, default_meps_lang)?;

        Ok(Some(path.display().to_string()))
    }
}

#[tauri::command]
pub fn export_marks_to_bytes(
    default_name: Option<String>,
    state_mgr: State<'_, StateManager>,
) -> Result<Vec<u8>, String> {
    let (marks, default_meps_lang) = validated_export_marks(&state_mgr)?;
    let file_name = resolve_export_file_name(default_name);
    crate::jwlibrary_export::export_marks_to_jwlibrary_bytes(
        &marks,
        Some(&file_name),
        default_meps_lang,
    )
}

#[tauri::command]
pub fn dispatch_tab_message(
    app: AppHandle,
    _tab_id: Option<String>,
    message: Value,
) -> Result<Value, String> {
    if let Some(main_window) = app.get_webview_window("main") {
        let msg_json = serde_json::to_string(&message).unwrap_or_default();
        let eval_script = format!(
            "if (window.__wolDispatchMessage) {{ window.__wolDispatchMessage({msg_json}); }}"
        );
        let _ = main_window.eval(&eval_script);
    }
    // Return a default status object
    Ok(serde_json::json!({
        "applied": 1,
        "hasBackup": true,
        "enabled": true
    }))
}

// The IPC names are the function names, so they must match what the webview
// invokes (`ui/manager.js`, `ui/imported-library.js`). `tests/ui_command_contract.rs`
// enforces that; do not reintroduce a `_command` suffix here.
#[tauri::command]
pub fn list_imported_publications() -> Vec<serde_json::Value> {
    wol_core::publications::list_imported_publications()
}

#[tauri::command]
pub fn save_imported_publication(
    publication: wol_core::publications::ImportedPublication,
) -> Result<(), String> {
    wol_core::publications::save_imported_publication(publication)
}

/// Convert a core `ThemeMode` into Tauri's theme (the core crate stays Tauri-free).
#[cfg(desktop)]
pub fn theme_to_tauri(theme: wol_core::state::ThemeMode) -> Option<tauri::Theme> {
    match theme {
        wol_core::state::ThemeMode::System => None,
        wol_core::state::ThemeMode::Light => Some(tauri::Theme::Light),
        wol_core::state::ThemeMode::Dark => Some(tauri::Theme::Dark),
    }
}

#[tauri::command]
pub fn get_settings(state_mgr: State<'_, StateManager>) -> Result<AppSettings, String> {
    state_mgr.get_settings().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_settings(
    app: AppHandle,
    settings: AppSettings,
    state_mgr: State<'_, StateManager>,
) -> Result<(), String> {
    let theme = settings.theme;
    state_mgr.set_settings(settings).map_err(|e| e.to_string())?;

    #[cfg(desktop)]
    let tauri_theme = theme_to_tauri(theme);
    let theme_str = theme.as_str();

    if let Some(main_window) = app.get_webview_window("main") {
        #[cfg(desktop)]
        let _ = main_window.set_theme(tauri_theme);
        let _ = main_window.eval(&format!(
            "if (window.__wolApplyTheme) {{ window.__wolApplyTheme({theme_str:?}); }} if (window.__wolApplyManagerTheme) {{ window.__wolApplyManagerTheme({theme_str:?}); }}"
        ));
    }
    Ok(())
}

#[tauri::command]
pub fn get_languages() -> Vec<LanguageInfo> {
    languages::all_languages().to_vec()
}

#[tauri::command]
pub fn navigate_home(app: AppHandle, state_mgr: State<'_, StateManager>) -> Result<(), String> {
    let settings = state_mgr.get_settings().map_err(|e| e.to_string())?;
    if let Some(main_window) = app.get_webview_window("main") {
        if !settings.is_configured() {
            if let Ok(parsed) = url::Url::parse("tauri://localhost/setup.html") {
                let _ = main_window.navigate(parsed);
            }
            return Ok(());
        }
        let lang_info = languages::find_by_locale(&settings.language)
            .or_else(|| languages::find_by_symbol(&settings.language));
        let lib = lang_info
            .map(|l| l.lib.as_str())
            .unwrap_or(&settings.language);

        let wol_url = format!(
            "wol://{}/wol/h/r{}/lp-{}",
            settings.language, settings.region, lib
        );
        if let Ok(parsed) = url::Url::parse(&wol_url) {
            let _ = main_window.navigate(parsed);
        }
    }
    Ok(())
}

/// Parse `/<lang>/wol/h/r<region>/lp-<lib>` (or `/<lang>/wol/.../r<region>/lp-<lib>`)
/// into `(language, region, lib)`.
pub fn parse_wol_home_path(path: &str) -> Option<(String, u32, String)> {
    let segments: Vec<&str> = path.trim_matches('/').split('/').collect();
    // Expected: [<lang>, "wol", <kind>, "r<region>", "lp-<lib>", ...]
    if segments.len() >= 5 && segments[1] == "wol" {
        let lang = segments[0].trim();
        let region_str = segments[3].strip_prefix('r')?;
        let region: u32 = region_str.parse().ok()?;
        let lib = segments[4].strip_prefix("lp-")?.trim();
        if !lang.is_empty() && !lib.is_empty() {
            return Some((lang.to_string(), region, lib.to_string()));
        }
    }
    None
}

/// Normalize user-entered URL (adds `https://` if missing, validates scheme & host).
pub fn normalize_user_url(raw: &str) -> Result<url::Url, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("Please enter a URL.".to_string());
    }
    let with_scheme = if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        format!("https://{trimmed}")
    };
    let parsed = url::Url::parse(&with_scheme).map_err(|e| format!("Invalid URL: {e}"))?;
    match parsed.scheme() {
        "https" | "http" => {}
        other => return Err(format!("Unsupported URL scheme: {other}")),
    }
    let host = parsed.host_str().unwrap_or("");
    if host.is_empty() || !host.contains('.') {
        return Err("Please enter a valid domain URL (e.g. https://example.org/es).".to_string());
    }
    Ok(parsed)
}

#[tauri::command]
pub async fn setup_from_url(
    app: AppHandle,
    raw_url: String,
    state_mgr: State<'_, StateManager>,
) -> Result<AppSettings, String> {
    let parsed = normalize_user_url(&raw_url)?;
    let mut base_host = parsed
        .host_str()
        .ok_or_else(|| "URL is missing a host.".to_string())?
        .to_string();

    let resolved = if let Some(triple) = parse_wol_home_path(parsed.path()) {
        triple
    } else {
        // Follow redirect (e.g. https://<host>/es -> https://<host>/es/wol/h/r4/lp-s)
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .user_agent(wol_core::handler::DEFAULT_USER_AGENT)
            .build()
            .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

        let first_seg = parsed
            .path_segments()
            .and_then(|mut s| s.next())
            .unwrap_or("")
            .trim()
            .to_string();

        match client.get(parsed.as_str()).send().await {
            Ok(resp) => {
                let final_url = resp.url().clone();
                if let Some(final_host) = final_url.host_str() {
                    if !final_host.is_empty() {
                        base_host = final_host.to_string();
                    }
                }
                if let Some(triple) = parse_wol_home_path(final_url.path()) {
                    triple
                } else if let Some(lang_info) = languages::find_by_locale(&first_seg) {
                    (
                        lang_info.locale.clone(),
                        lang_info.region,
                        lang_info.lib.clone(),
                    )
                } else {
                    return Err(
                        "Could not detect language home path from that URL. Please enter a language URL (e.g. .../es or .../es/wol/h/r4/lp-s)."
                            .to_string(),
                    );
                }
            }
            Err(err) => {
                // Offline fallback: if the user typed /<locale> and it's in our languages table, use it
                if let Some(lang_info) = languages::find_by_locale(&first_seg) {
                    (
                        lang_info.locale.clone(),
                        lang_info.region,
                        lang_info.lib.clone(),
                    )
                } else {
                    return Err(format!(
                        "Could not connect to {base_host}: {err}. You can also enter the full home URL (e.g. https://{base_host}/es/wol/h/r4/lp-s)."
                    ));
                }
            }
        }
    };

    let (language, region, lib) = resolved;
    let home = format!("https://{base_host}/{language}/wol/h/r{region}/lp-{lib}");

    let mut settings = state_mgr.get_settings().unwrap_or_default();
    settings.base_host = Some(base_host);
    settings.language = language.clone();
    settings.region = region;
    settings.home = home;

    state_mgr
        .set_settings(settings.clone())
        .map_err(|e| e.to_string())?;

    if let Some(main_window) = app.get_webview_window("main") {
        let _ = main_window.set_title("OfflineWOL");
        let wol_url = format!("wol://{language}/wol/h/r{region}/lp-{lib}");
        if let Ok(target) = url::Url::parse(&wol_url) {
            let _ = main_window.navigate(target);
        }
    }

    Ok(settings)
}

#[tauri::command]
pub fn get_cache_stats(cache: State<'_, DiskCache>) -> Result<CacheStats, String> {
    let total_size = cache.total_size().map_err(|e| e.to_string())?;
    let entry_count = cache.entry_count().map_err(|e| e.to_string())?;
    let cap_bytes = cache.cap_bytes();
    Ok(CacheStats {
        total_size,
        entry_count,
        cap_bytes,
    })
}

#[tauri::command]
pub fn list_cache_entries(
    cache: State<'_, DiskCache>,
    limit: Option<usize>,
    offset: Option<usize>,
    filter: Option<String>,
) -> Result<Vec<CacheEntry>, String> {
    cache
        .list_entries(
            limit.unwrap_or(50),
            offset.unwrap_or(0),
            filter.as_deref(),
        )
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn clear_cache(cache: State<'_, DiskCache>) -> Result<(), String> {
    cache.clear_all().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn clear_cache_older_than(
    days: i64,
    cache: State<'_, DiskCache>,
) -> Result<usize, String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let cutoff = now - (days * 86400);
    cache.clear_older_than(cutoff).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn is_cached(url: String, cache: State<'_, DiskCache>) -> Result<bool, String> {
    let canonical = CanonicalUrl::new(url);
    cache.is_cached(&canonical).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn save_page_offline(
    url: String,
    crawler: State<'_, Crawler>,
) -> Result<CrawlResult, String> {
    let canonical = CanonicalUrl::new(url);
    crawler
        .crawl_page(&canonical)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn save_issue_offline(
    toc_url: String,
    crawler: State<'_, Crawler>,
) -> Result<IssueCrawlResult, String> {
    let canonical = CanonicalUrl::new(toc_url);
    crawler
        .crawl_issue(&canonical)
        .await
        .map_err(|e| e.to_string())
}

/// Normalize either a `wol://...` or `https://...` URL into a `CanonicalUrl`.
fn resolve_to_canonical(raw_url: &str, state_mgr: &StateManager) -> Result<CanonicalUrl, String> {
    if raw_url.starts_with("wol://") {
        let base_host = state_mgr
            .get_settings()
            .ok()
            .and_then(|s| s.base_host)
            .ok_or_else(|| "Library URL not configured yet.".to_string())?;
        let uri: wol_core::url_map::Url = raw_url.parse().map_err(|e| format!("invalid URI: {e}"))?;
        map_wol_uri_to_canonical(&uri, &base_host).map_err(|e| e.to_string())
    } else {
        Ok(CanonicalUrl::new(raw_url.to_string()))
    }
}

#[tauri::command]
pub async fn save_bible_offline(
    app: AppHandle,
    binav_url: String,
    crawler: State<'_, Crawler>,
    state_mgr: State<'_, StateManager>,
) -> Result<BibleCrawlResult, String> {
    let canonical = resolve_to_canonical(&binav_url, &state_mgr)?;
    crawler
        .crawl_bible(&canonical, |progress| {
            if let Ok(json) = serde_json::to_string(&progress) {
                if let Some(main_window) = app.get_webview_window("main") {
                    let _ = main_window.eval(&format!(
                        "if (window.__wolBibleCrawlProgress) {{ window.__wolBibleCrawlProgress({json}); }}"
                    ));
                }
            }
        })
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn cancel_bible_offline(crawler: State<'_, Crawler>) -> Result<(), String> {
    crawler.cancel_bible_crawl();
    Ok(())
}

#[tauri::command]
pub async fn save_library_offline(
    app: AppHandle,
    library_url: String,
    crawler: State<'_, Crawler>,
    state_mgr: State<'_, StateManager>,
) -> Result<LibraryCrawlResult, String> {
    let canonical = resolve_to_canonical(&library_url, &state_mgr)?;
    crawler
        .crawl_library(&canonical, |progress| {
            if let Ok(json) = serde_json::to_string(&progress) {
                if let Some(main_window) = app.get_webview_window("main") {
                    let _ = main_window.eval(&format!(
                        "if (window.__wolLibraryCrawlProgress) {{ window.__wolLibraryCrawlProgress({json}); }}"
                    ));
                }
            }
        })
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn cancel_library_offline(crawler: State<'_, Crawler>) -> Result<(), String> {
    crawler.cancel_library_crawl();
    Ok(())
}

fn normalize_manager_tab(tab: Option<&str>) -> Option<&'static str> {
    match tab.map(str::trim) {
        Some("highlights" | "tab-highlights") => Some("tab-highlights"),
        Some("settings" | "tab-settings") => Some("tab-settings"),
        Some("cache" | "tab-cache") => Some("tab-cache"),
        _ => None,
    }
}

fn app_page_url(app_url: &str) -> Result<url::Url, String> {
    #[cfg(target_os = "windows")]
    let raw = format!("http://tauri.localhost/{app_url}");
    #[cfg(not(target_os = "windows"))]
    let raw = format!("tauri://localhost/{app_url}");
    url::Url::parse(&raw).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn open_manager_window(app: AppHandle, tab: Option<String>) -> Result<(), String> {
    let target_tab = normalize_manager_tab(tab.as_deref());
    let app_url = match target_tab {
        Some(tab_id) => format!("index.html#{tab_id}"),
        None => "index.html".to_string(),
    };

    if let Some(win) = app.get_webview_window("main") {
        let parsed = app_page_url(&app_url)?;
        win.navigate(parsed).map_err(|e| e.to_string())?;
        #[cfg(desktop)]
        let _ = win.set_focus();
    }

    Ok(())
}

#[tauri::command]
pub fn get_sync_status(sync_mgr: State<'_, Arc<SyncManager>>) -> Result<SyncStatus, String> {
    sync_mgr.status()
}

#[tauri::command]
pub async fn sync_now(
    sync_mgr: State<'_, Arc<SyncManager>>,
) -> Result<SyncStatus, String> {
    sync_mgr.sync_now().await
}

#[tauri::command]
pub async fn connect_nextcloud(
    server_url: String,
    username: String,
    password: String,
    remote_path: Option<String>,
    sync_mgr: State<'_, Arc<SyncManager>>,
) -> Result<SyncStatus, String> {
    sync_mgr
        .connect_nextcloud(server_url, username, password, remote_path)
        .await
}

#[tauri::command]
pub async fn connect_nextcloud_browser(
    server_url: String,
    remote_path: Option<String>,
    sync_mgr: State<'_, Arc<SyncManager>>,
) -> Result<SyncStatus, String> {
    sync_mgr
        .connect_nextcloud_browser(server_url, remote_path)
        .await
}

#[tauri::command]
pub async fn connect_google_drive(
    sync_mgr: State<'_, Arc<SyncManager>>,
) -> Result<SyncStatus, String> {
    sync_mgr.connect_google_drive().await
}

#[tauri::command]
pub fn disconnect_sync(sync_mgr: State<'_, Arc<SyncManager>>) -> Result<SyncStatus, String> {
    sync_mgr.disconnect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_wol_home_path() {
        assert_eq!(
            parse_wol_home_path("/es/wol/h/r4/lp-s"),
            Some(("es".to_string(), 4, "s".to_string()))
        );
        assert_eq!(
            parse_wol_home_path("/en/wol/h/r1/lp-e/"),
            Some(("en".to_string(), 1, "e".to_string()))
        );
        assert_eq!(
            parse_wol_home_path("/cmn-Hans/wol/h/r23/lp-chs"),
            Some(("cmn-Hans".to_string(), 23, "chs".to_string()))
        );
        assert_eq!(parse_wol_home_path("/es"), None);
        assert_eq!(parse_wol_home_path("/"), None);
    }

    #[test]
    fn normalizes_user_urls() {
        assert_eq!(
            normalize_user_url("  https://example.org/es  ").unwrap().as_str(),
            "https://example.org/es"
        );
        assert_eq!(
            normalize_user_url("example.org/es/wol/h/r4/lp-s").unwrap().as_str(),
            "https://example.org/es/wol/h/r4/lp-s"
        );
        assert!(normalize_user_url("").is_err());
        assert!(normalize_user_url("ftp://example.org/es").is_err());
        assert!(normalize_user_url("localhost").is_err());
    }
}
