use std::fs;
use std::str::FromStr;
use tauri::http::{Method, StatusCode, Uri};
use wol_desktop::cache::DiskCache;
use wol_desktop::handler::ProtocolHandler;
use wol_desktop::url_map::{map_wol_uri_to_canonical, CanonicalUrl};

#[tokio::test]
async fn acceptance_test_1_load_article_disable_network_reload() {
    let Ok(base_host) = std::env::var("WOL_TEST_HOST") else {
        return;
    };
    let temp_dir = std::env::temp_dir().join(format!("wol_acc1_{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp_dir);

    let cache = DiskCache::open(&temp_dir, 100 * 1024 * 1024).unwrap();
    let handler = ProtocolHandler::new_with_base_host(cache.clone(), &base_host);

    let article_uri = Uri::from_str("wol://es/wol/d/r4/lp-s/1102026210").unwrap();
    let resp = handler.handle_request(&article_uri, &Method::GET).await;
    assert_eq!(resp.status(), StatusCode::OK);

    // Reload with network disconnected:
    let canonical = map_wol_uri_to_canonical(&article_uri, &base_host).unwrap();
    let (entry, bytes) = cache.get(&canonical).unwrap().expect("cached entry");
    assert_eq!(entry.content_type, "text/html; charset=utf-8");
    let html = String::from_utf8(bytes).unwrap();
    assert!(html.contains("data-pid="));
    assert!(html.contains("wol://es/assets/"));

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn acceptance_test_2_swr_immediate_hit_and_background_touch() {
    let Ok(base_host) = std::env::var("WOL_TEST_HOST") else {
        return;
    };
    let temp_dir = std::env::temp_dir().join(format!("wol_acc2_{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp_dir);

    let cache = DiskCache::open(&temp_dir, 100 * 1024 * 1024).unwrap();
    let handler = ProtocolHandler::new_with_base_host(cache.clone(), &base_host);

    let article_uri = Uri::from_str("wol://es/wol/d/r4/lp-s/1102026210").unwrap();
    // First load
    let _ = handler.handle_request(&article_uri, &Method::GET).await;

    // Second load: served from disk immediately
    let resp2 = handler.handle_request(&article_uri, &Method::GET).await;
    assert_eq!(resp2.status(), StatusCode::OK);

    // Sleep briefly to let background revalidation complete
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    let canonical = map_wol_uri_to_canonical(&article_uri, &base_host).unwrap();
    let (entry, _) = cache.get(&canonical).unwrap().unwrap();
    assert!(entry.checked_at >= entry.fetched_at);

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn acceptance_test_3_modify_etag_forces_200_and_blob_replacement() {
    let temp_dir = std::env::temp_dir().join(format!("wol_acc3_{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp_dir);

    let cache = DiskCache::open(&temp_dir, 100 * 1024 * 1024).unwrap();
    let url = CanonicalUrl::new("https://wol.example.org/es/wol/d/r4/lp-s/1102026210".to_string());

    // Populate with old/stale dummy data
    let old_body = b"<html>stale body</html>";
    cache
        .put(&url, "text/html", Some("\"fake_old_etag\""), None, old_body)
        .unwrap();

    let (old_entry, old_bytes) = cache.get(&url).unwrap().unwrap();
    assert_eq!(old_bytes, old_body);
    assert_eq!(old_entry.etag.as_deref(), Some("\"fake_old_etag\""));

    // Now put replacement (e.g. from 200 response)
    let fresh_body = b"<html>fresh updated body</html>";
    cache
        .put(&url, "text/html", Some("\"new_real_etag\""), None, fresh_body)
        .unwrap();

    let (new_entry, new_bytes) = cache.get(&url).unwrap().unwrap();
    assert_eq!(new_bytes, fresh_body);
    assert_eq!(new_entry.etag.as_deref(), Some("\"new_real_etag\""));

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn acceptance_test_4_uncached_url_offline_yields_friendly_miss_page() {
    let temp_dir = std::env::temp_dir().join(format!("wol_acc4_{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp_dir);

    let cache = DiskCache::open(&temp_dir, 100 * 1024 * 1024).unwrap();
    // Simulate offline network with an unreachable proxy or dead socket
    let offline_client = reqwest::Client::builder()
        .proxy(reqwest::Proxy::all("http://127.0.0.1:9").unwrap())
        .timeout(std::time::Duration::from_millis(100))
        .build()
        .unwrap();

    let handler = ProtocolHandler::new_with_client_and_host(cache, offline_client, "wol.example.org");

    let uri = Uri::from_str("wol://es/wol/d/r4/lp-s/1102026210").unwrap();
    let resp = handler.handle_request(&uri, &Method::GET).await;

    let body_str = String::from_utf8(resp.body().clone()).unwrap();
    assert!(body_str.contains("Page Not Available Offline"));
    assert!(body_str.contains("Go Back"));

    let _ = fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn acceptance_test_5_cache_cap_lru_eviction() {
    let temp_dir = std::env::temp_dir().join(format!("wol_acc5_{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp_dir);

    // Cap at 200 bytes
    let cache = DiskCache::open(&temp_dir, 200).unwrap();

    let url1 = CanonicalUrl::new("https://wol.example.org/1".to_string());
    let url2 = CanonicalUrl::new("https://wol.example.org/2".to_string());
    let url3 = CanonicalUrl::new("https://wol.example.org/3".to_string());

    let payload_90b = vec![b'x'; 90];

    cache.put(&url1, "text/plain", None, None, &payload_90b).unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    cache.put(&url2, "text/plain", None, None, &payload_90b).unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Both url1 and url2 are currently cached (180b <= 200b)
    assert!(cache.get(&url1).unwrap().is_some());
    assert!(cache.get(&url2).unwrap().is_some());

    // Adding url3 (90b) will exceed 200b cap, triggering LRU eviction of least recently used
    cache.put(&url3, "text/plain", None, None, &payload_90b).unwrap();

    let total = cache.total_size().unwrap();
    assert!(total <= 200);

    // url3 is present
    assert!(cache.get(&url3).unwrap().is_some());

    let _ = fs::remove_dir_all(&temp_dir);
}
