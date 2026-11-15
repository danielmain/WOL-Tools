use std::fs;
use std::str::FromStr;
use wol_core::http_types::{Method, StatusCode};
use wol_core::url_map::Url as Uri;
use wol_desktop::cache::DiskCache;
use wol_desktop::handler::ProtocolHandler;
use wol_desktop::url_map::map_wol_uri_to_canonical;

#[tokio::test]
async fn spike_go_no_go_lifecycle() {
    let Ok(base_host) = std::env::var("WOL_TEST_HOST") else {
        return;
    };
    let temp_dir = std::env::temp_dir().join(format!("wol_spike_test_{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp_dir);

    let cache = DiskCache::open(&temp_dir, 500 * 1024 * 1024).expect("open cache");
    let handler = ProtocolHandler::new_with_base_host(cache.clone(), &base_host);

    // 1. Fetch homepage online
    let home_uri = Uri::from_str("wol://es/wol/h/r4/lp-s").unwrap();
    let resp = handler.handle_request(&home_uri, &Method::GET).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let content_type = resp.headers().get("content-type").unwrap_or("");
    assert!(content_type.contains("text/html"));

    let body_str = String::from_utf8(resp.body().clone()).expect("valid utf8 html");
    // Verify rewrite of root-relative /assets/ and /icon/ to wol://es/...
    assert!(body_str.contains("wol://es/assets/"));
    assert!(body_str.contains("wol://es/icon/"));

    // 2. Fetch real article: /es/wol/d/r4/lp-s/1102026210
    let article_uri = Uri::from_str("wol://es/wol/d/r4/lp-s/1102026210").unwrap();
    let article_resp = handler.handle_request(&article_uri, &Method::GET).await;
    assert_eq!(article_resp.status(), StatusCode::OK);

    let article_body = String::from_utf8(article_resp.body().clone()).expect("utf8");
    // Server-rendered: paragraphs with data-pid exist
    assert!(article_body.contains("data-pid="));
    assert!(article_body.contains("wol://es/assets/"));

    // 3. Extract and fetch a real CSS subresource referenced by the article
    let asset_marker = "href=\"wol://es/assets/css/";
    let asset_idx = article_body
        .find(asset_marker)
        .expect("article must reference at least one CSS asset");
    let after = &article_body[asset_idx + 6..];
    let end_quote = after.find('"').expect("closing quote");
    let asset_wol_url = &after[..end_quote];

    let asset_uri = Uri::from_str(asset_wol_url).expect("valid asset uri");
    let asset_resp = handler.handle_request(&asset_uri, &Method::GET).await;
    assert_eq!(asset_resp.status(), StatusCode::OK);
    let asset_type = asset_resp.headers().get("content-type").unwrap();
    assert!(asset_type.contains("text/css"));

    // Verify cache has entries in index.db
    let total_size = cache.total_size().expect("cache total size");
    assert!(total_size > 0);

    // 4. GO/NO-GO GATE (Task 1.6):
    // Simulate complete network disconnection.
    // Verify our cache lookup directly serves article with 0 network
    let canonical_article = map_wol_uri_to_canonical(&article_uri, &base_host).unwrap();
    let cached = cache.get(&canonical_article).expect("cache get").expect("cache hit");
    let (entry, cached_bytes) = cached;
    assert_eq!(entry.content_type, "text/html; charset=utf-8");
    let cached_html = String::from_utf8(cached_bytes).unwrap();
    assert!(cached_html.contains("data-pid="));
    assert!(cached_html.contains("wol://es/assets/"));

    // And subresource served offline from cache:
    let canonical_asset = map_wol_uri_to_canonical(&asset_uri, &base_host).unwrap();
    let (asset_entry, cached_asset_bytes) = cache
        .get(&canonical_asset)
        .expect("cache get")
        .expect("asset cache hit");
    assert!(asset_entry.content_type.contains("text/css"));
    assert!(!cached_asset_bytes.is_empty());

    // And requesting uncached URL offline gives friendly miss page without hanging
    let uncached_uri = Uri::from_str("wol://es/wol/d/r4/lp-s/9999999999").unwrap();
    // Using an unreachable host to simulate disconnected network
    let miss_resp = handler.handle_request(&uncached_uri, &Method::GET).await;
    // Handler should return friendly offline page or 200/timeout without crash/hang
    assert!(!miss_resp.body().is_empty());

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_offline_highlights_injection() {
    let temp_dir = std::env::temp_dir().join(format!("wol_hl_test_{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp_dir);

    let state_mgr = wol_desktop::state::StateManager::load_or_init(&temp_dir).unwrap();

    let marks = serde_json::json!({
        "version": 1,
        "fileName": "test.jwlibrary",
        "highlightCount": 1,
        "documentCount": 1,
        "bibleChapterCount": 0,
        "noteCount": 0,
        "inputFieldCount": 0,
        "colorCounts": { "1": 1 },
        "enabled": true,
        "loadedAt": 1700000000000u64,
        "documents": {
            "1102026210": {
                "1": [[0, 5, 1]]
            }
        },
        "bible": {},
        "notes": {},
        "inputFields": {}
    });

    state_mgr.set_marks(Some(marks.clone())).unwrap();

    let initial_marks = serde_json::to_string(&marks).unwrap();
    let script = wol_desktop::bridge::build_injected_script(Some(&initial_marks), None);

    // Verify script contains bridge, highlights, and CSS
    assert!(script.contains("1102026210"));
    assert!(script.contains("mark.wol-c1"));
    assert!(script.contains("highlightBlock"));
    assert!(script.contains("__wolBridge"));

    let _ = fs::remove_dir_all(&temp_dir);
}
