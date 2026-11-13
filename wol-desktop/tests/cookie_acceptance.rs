use std::fs;
use std::str::FromStr;
use tauri::http::{Method, StatusCode, Uri};
use wol_desktop::bridge::build_injected_script;
use wol_desktop::cache::DiskCache;
use wol_desktop::handler::ProtocolHandler;
use wol_desktop::state::StateManager;

#[tokio::test]
async fn test_cookie_persistence_and_gdpr_prevention() {
    let temp_dir = std::env::temp_dir().join(format!("wol_cookie_test_{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp_dir);

    // 1. StateManager defaults include all 4 GDPR consent cookies
    let state_mgr = StateManager::load_or_init(&temp_dir).expect("init state manager");
    let cookies = state_mgr.get_cookies().expect("get cookies");
    assert_eq!(cookies.get("cookieConsent-STRICTLY_NECESSARY").map(String::as_str), Some("true"));
    assert_eq!(cookies.get("cookieConsent-FUNCTIONAL").map(String::as_str), Some("true"));
    assert_eq!(cookies.get("cookieConsent-DIAGNOSTIC").map(String::as_str), Some("true"));
    assert_eq!(cookies.get("cookieConsent-USAGE").map(String::as_str), Some("true"));

    // 2. Setting or modifying cookies persists to state.json
    state_mgr
        .update_cookie("fontScale".to_string(), Some("1.25".to_string()))
        .expect("save fontScale");
    state_mgr
        .update_cookie("cookieConsent-DIAGNOSTIC".to_string(), Some("false".to_string()))
        .expect("update cookieConsent");

    let state_mgr2 = StateManager::load_or_init(&temp_dir).expect("reload state manager");
    let reloaded_cookies = state_mgr2.get_cookies().expect("get reloaded cookies");
    assert_eq!(reloaded_cookies.get("fontScale").map(String::as_str), Some("1.25"));
    assert_eq!(reloaded_cookies.get("cookieConsent-DIAGNOSTIC").map(String::as_str), Some("false"));
    assert_eq!(reloaded_cookies.get("cookieConsent-FUNCTIONAL").map(String::as_str), Some("true"));

    // 3. Configure base_host and pre-populate cache; ProtocolHandler injects stored cookies
    let mut settings = state_mgr2.get_settings().expect("get settings");
    settings.base_host = Some("wol.example.org".to_string());
    settings.language = "es".to_string();
    settings.region = 4;
    settings.home = "https://wol.example.org/es/wol/h/r4/lp-s".to_string();
    state_mgr2.set_settings(settings).expect("save settings");

    let cache_dir = temp_dir.join("cache");
    let cache = DiskCache::open(&cache_dir, 50 * 1024 * 1024).expect("open cache");
    let canonical = wol_desktop::url_map::CanonicalUrl::new(
        "https://wol.example.org/es/wol/d/r4/lp-s/1102026210".to_string(),
    );
    cache
        .put(
            &canonical,
            "text/html; charset=utf-8",
            None,
            None,
            b"<html><head></head><body><p data-pid=\"1\">Hello</p></body></html>",
        )
        .expect("seed cache");

    let handler = ProtocolHandler::new_with_state(cache.clone(), state_mgr2);

    let article_uri = Uri::from_str("wol://es/wol/d/r4/lp-s/1102026210").unwrap();
    let resp = handler.handle_request(&article_uri, &Method::GET).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let html = String::from_utf8(resp.body().clone()).expect("valid utf8");
    assert!(html.contains(r#"<script id="__wol_cookies_bootstrap">"#));
    assert!(html.contains(r#""cookieConsent-FUNCTIONAL":"true""#));
    assert!(html.contains(r#""fontScale":"1.25""#));

    // 4. Cached pages also serve with updated cookies
    let resp_cached = handler.handle_request(&article_uri, &Method::GET).await;
    assert_eq!(resp_cached.status(), StatusCode::OK);
    let cached_html = String::from_utf8(resp_cached.body().clone()).expect("valid utf8");
    assert!(cached_html.contains(r#"<script id="__wol_cookies_bootstrap">"#));
    assert!(cached_html.contains(r#""fontScale":"1.25""#));

    // 5. Injected script contains the document.cookie hook and lnc-firstRunPopup suppression
    let script = build_injected_script(None, None);
    assert!(script.contains("__wolCookiesShimInstalled"));
    assert!(script.contains("cookieConsent-FUNCTIONAL"));
    assert!(script.contains(".lnc-firstRunPopup { display: none !important; }"));

    let _ = fs::remove_dir_all(&temp_dir);
}
