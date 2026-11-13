use std::fs;
use wol_desktop::cache::DiskCache;
use wol_desktop::crawler::Crawler;
use wol_desktop::languages;
use wol_desktop::state::{AppSettings, StateManager};
use wol_desktop::url_map::CanonicalUrl;

#[test]
fn test_ui_files_and_wasm_exist_for_manager() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let ui_dir = std::path::Path::new(manifest_dir).join("ui");

    assert!(ui_dir.join("index.html").exists(), "ui/index.html must exist");
    assert!(ui_dir.join("setup.html").exists(), "ui/setup.html must exist");
    assert!(ui_dir.join("manager.js").exists(), "ui/manager.js must exist");
    assert!(ui_dir.join("manager.css").exists(), "ui/manager.css must exist");
    assert!(ui_dir.join("popup.css").exists(), "ui/popup.css must exist");

    let lib_dir = ui_dir.join("lib");
    assert!(lib_dir.join("sql-wasm.js").exists(), "ui/lib/sql-wasm.js must exist");
    assert!(lib_dir.join("sql-wasm.wasm").exists(), "ui/lib/sql-wasm.wasm must exist");
    assert!(lib_dir.join("backup.js").exists(), "ui/lib/backup.js must exist");
}

#[test]
fn test_languages_table_coverage() {
    let langs = languages::all_languages();
    assert!(langs.len() >= 950, "Expected at least 950 languages, found {}", langs.len());

    let es = languages::find_by_locale("es").expect("es must be found");
    assert_eq!(es.symbol, "S");
    assert_eq!(es.region, 4);
    assert_eq!(es.lib, "s");
    assert_eq!(es.home, "/es/wol/h/r4/lp-s");

    let en = languages::find_by_symbol("E").expect("en must be found");
    assert_eq!(en.locale, "en");
    assert_eq!(en.region, 1);
    assert_eq!(en.home, "/en/wol/h/r1/lp-e");
}

#[test]
fn test_settings_save_and_retrieve() {
    let temp_dir = std::env::temp_dir().join(format!("wol_settings_test_{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp_dir);

    let state_mgr = StateManager::load_or_init(&temp_dir).expect("init state manager");
    let initial = state_mgr.get_settings().expect("get default settings");
    assert!(initial.base_host.is_none());
    assert!(!initial.is_configured());

    let updated = AppSettings {
        base_host: Some("wol.example.org".to_string()),
        language: "en".to_string(),
        region: 1,
        home: "https://wol.example.org/en/wol/h/r1/lp-e".to_string(),
        cache_cap_bytes: 1024 * 1024 * 1024,
        theme: wol_desktop::state::ThemeMode::Dark,
    };
    state_mgr.set_settings(updated.clone()).expect("set settings");

    let loaded = state_mgr.get_settings().expect("reload settings");
    assert_eq!(loaded.base_host.as_deref(), Some("wol.example.org"));
    assert!(loaded.is_configured());
    assert_eq!(loaded.language, "en");
    assert_eq!(loaded.region, 1);
    assert_eq!(loaded.home, "https://wol.example.org/en/wol/h/r1/lp-e");
    assert_eq!(loaded.cache_cap_bytes, 1024 * 1024 * 1024);
    assert_eq!(loaded.theme, wol_desktop::state::ThemeMode::Dark);

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_crawler_page_offline_saving() {
    let Ok(base_host) = std::env::var("WOL_TEST_HOST") else {
        return;
    };
    let temp_dir = std::env::temp_dir().join(format!("wol_crawler_test_{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp_dir);

    let cache = DiskCache::open(&temp_dir, 500 * 1024 * 1024).expect("open cache");
    let crawler = Crawler::new(cache.clone());

    let article_url = CanonicalUrl::new(format!("https://{base_host}/es/wol/d/r4/lp-s/1102026210"));
    let crawl_res = crawler.crawl_page(&article_url).await.expect("crawl page");

    assert_eq!(crawl_res.page_url, article_url.as_str());
    assert!(crawl_res.subresources_count > 0, "must extract subresources");
    assert!(cache.is_cached(&article_url).expect("is_cached"), "article must be cached");

    // Check Cache Inspector methods reflect crawled content
    let entry_count = cache.entry_count().expect("entry_count");
    assert!(entry_count > 0);

    let entries = cache.list_entries(50, 0, None).expect("list_entries");
    assert!(!entries.is_empty());

    let filtered = cache.list_entries(50, 0, Some("assets")).expect("list filtered");
    // Subresources include assets
    assert!(!filtered.is_empty());

    let _ = fs::remove_dir_all(&temp_dir);
}
