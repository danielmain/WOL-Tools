use tauri::http::{Response, StatusCode};

use crate::cache::DiskCache;
use crate::rewrite::{rewrite_css, rewrite_html};
use crate::state::StateManager;
use crate::url_map::{map_wol_uri_to_canonical, CanonicalUrl};

#[derive(Clone)]
pub struct ProtocolHandler {
    cache: DiskCache,
    http_client: reqwest::Client,
    state_mgr: Option<StateManager>,
    base_host: Option<String>,
}

pub const DEFAULT_USER_AGENT: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15";

impl ProtocolHandler {
    pub fn new(cache: DiskCache) -> Self {
        Self::new_with_optional_state(cache, None)
    }

    pub fn new_with_base_host(cache: DiskCache, base_host: impl Into<String>) -> Self {
        let mut handler = Self::new_with_optional_state(cache, None);
        handler.base_host = Some(base_host.into());
        handler
    }

    pub fn new_with_state(cache: DiskCache, state_mgr: StateManager) -> Self {
        Self::new_with_optional_state(cache, Some(state_mgr))
    }

    pub fn new_with_optional_state(cache: DiskCache, state_mgr: Option<StateManager>) -> Self {
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .user_agent(DEFAULT_USER_AGENT)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self {
            cache,
            http_client,
            state_mgr,
            base_host: None,
        }
    }

    pub fn new_with_client(cache: DiskCache, http_client: reqwest::Client) -> Self {
        Self {
            cache,
            http_client,
            state_mgr: None,
            base_host: None,
        }
    }

    pub fn new_with_client_and_host(
        cache: DiskCache,
        http_client: reqwest::Client,
        base_host: impl Into<String>,
    ) -> Self {
        Self {
            cache,
            http_client,
            state_mgr: None,
            base_host: Some(base_host.into()),
        }
    }

    fn effective_base_host(&self) -> Option<String> {
        if let Some(mgr) = &self.state_mgr {
            if let Ok(settings) = mgr.get_settings() {
                if let Some(h) = settings.base_host {
                    if !h.trim().is_empty() {
                        return Some(h);
                    }
                }
            }
        }
        self.base_host.clone()
    }

    fn inject_cookies(&self, html: &str) -> String {
        let cookies = match &self.state_mgr {
            Some(mgr) => mgr.get_cookies().unwrap_or_else(|_| crate::state::default_cookies()),
            None => crate::state::default_cookies(),
        };
        inject_cookies_into_html(html, &cookies)
    }

    pub async fn handle_request(
        &self,
        uri: &tauri::http::Uri,
        _method: &tauri::http::Method,
    ) -> Response<Vec<u8>> {
        let Some(base_host) = self.effective_base_host() else {
            return Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .header("Content-Type", "text/plain; charset=utf-8")
                .body(Vec::from("Library URL not configured yet."))
                .unwrap_or_else(|_| default_error_response());
        };

        let canonical_url = match map_wol_uri_to_canonical(uri, &base_host) {
            Ok(url) => url,
            Err(err) => {
                eprintln!("[ProtocolHandler] URL mapping error: {err} for URI: {uri}");
                return Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .header("Content-Type", "text/plain; charset=utf-8")
                    .body(Vec::from(format!("Bad request: {err}")))
                    .unwrap_or_else(|_| default_error_response());
            }
        };

        let host = uri.host().unwrap_or(&base_host);

        // 1. Check disk cache
        match self.cache.get(&canonical_url) {
            Ok(Some((entry, bytes))) => {
                println!(
                    "[ProtocolHandler] Cache HIT: {} ({} bytes, type={})",
                    canonical_url,
                    bytes.len(),
                    entry.content_type
                );
                // Background revalidation can be spawned here (SWR)
                let this = self.clone();
                let url_clone = canonical_url.clone();
                let host_string = host.to_string();
                let etag_clone = entry.etag.clone();
                let last_mod_clone = entry.last_modified.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = this
                        .revalidate_in_background(
                            url_clone,
                            host_string,
                            etag_clone.as_deref(),
                            last_mod_clone.as_deref(),
                        )
                        .await;
                });

                let bytes_to_serve = if entry.content_type.contains("text/html") {
                    if let Ok(html) = std::str::from_utf8(&bytes) {
                        self.inject_cookies(html).into_bytes()
                    } else {
                        bytes
                    }
                } else {
                    bytes
                };

                return build_response(StatusCode::OK, &entry.content_type, bytes_to_serve);
            }
            Ok(None) => {
                println!("[ProtocolHandler] Cache MISS: {}", canonical_url);
            }
            Err(err) => {
                eprintln!("[ProtocolHandler] Cache read error: {err}");
            }
        }

        // 2. Fetch upstream
        match self.fetch_upstream(&canonical_url, host).await {
            Ok((status, content_type, bytes)) => {
                build_response(status, &content_type, bytes)
            }
            Err(err) => {
                eprintln!(
                    "[ProtocolHandler] Upstream fetch failed for {canonical_url}: {err}"
                );
                if uri.path().contains("/wol/") && !uri.path().contains("/wol/mp/") {
                    // Friendly offline page for HTML document navigations
                    build_offline_miss_page(&canonical_url)
                } else {
                    Response::builder()
                        .status(StatusCode::GATEWAY_TIMEOUT)
                        .header("Content-Type", "text/plain; charset=utf-8")
                        .body(Vec::from(format!("Network unavailable: {err}")))
                        .unwrap_or_else(|_| default_error_response())
                }
            }
        }
    }

    async fn fetch_upstream(
        &self,
        url: &CanonicalUrl,
        host: &str,
    ) -> Result<(StatusCode, String, Vec<u8>), reqwest::Error> {
        let resp = self.http_client.get(url.as_str()).send().await?;
        let status = StatusCode::from_u16(resp.status().as_u16())
            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);

        if !status.is_success() {
            eprintln!(
                "[fetch_upstream] Non-success status: {} for URL: {}, headers: {:?}",
                status,
                url,
                resp.headers()
            );
        }

        let content_type = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("application/octet-stream")
            .to_string();

        let etag = resp
            .headers()
            .get("etag")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);

        let last_modified = resp
            .headers()
            .get("last-modified")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);

        let raw_bytes = resp.bytes().await?.to_vec();

        // Rewrite HTML / CSS if needed
        let bytes_to_serve = if content_type.contains("text/html") {
            if let Ok(html) = std::str::from_utf8(&raw_bytes) {
                let rewritten = rewrite_html(html, host);
                self.inject_cookies(&rewritten).into_bytes()
            } else {
                raw_bytes
            }
        } else if content_type.contains("text/css") {
            if let Ok(css) = std::str::from_utf8(&raw_bytes) {
                rewrite_css(css, host).into_bytes()
            } else {
                raw_bytes
            }
        } else {
            raw_bytes
        };

        // Cache successful responses
        if status.is_success() {
            if let Err(e) = self.cache.put(
                url,
                &content_type,
                etag.as_deref(),
                last_modified.as_deref(),
                &bytes_to_serve,
            ) {
                eprintln!("[ProtocolHandler] Failed to save {url} to cache: {e}");
            }
        }

        Ok((status, content_type, bytes_to_serve))
    }

    async fn revalidate_in_background(
        &self,
        url: CanonicalUrl,
        host: String,
        etag: Option<&str>,
        last_modified: Option<&str>,
    ) -> Result<(), ()> {
        let mut req = self.http_client.get(url.as_str());
        if let Some(et) = etag {
            req = req.header("If-None-Match", et);
        }
        if let Some(lm) = last_modified {
            req = req.header("If-Modified-Since", lm);
        }

        let resp = match req.send().await {
            Ok(r) => r,
            Err(_) => return Ok(()),
        };

        if resp.status() == reqwest::StatusCode::NOT_MODIFIED {
            let _ = self.cache.touch(&url);
            return Ok(());
        }

        if resp.status().is_success() {
            let content_type = resp
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("application/octet-stream")
                .to_string();

            let new_etag = resp
                .headers()
                .get("etag")
                .and_then(|v| v.to_str().ok())
                .map(str::to_string);

            let new_last_mod = resp
                .headers()
                .get("last-modified")
                .and_then(|v| v.to_str().ok())
                .map(str::to_string);

            if let Ok(raw_bytes) = resp.bytes().await {
                let bytes_to_store = if content_type.contains("text/html") {
                    if let Ok(html) = std::str::from_utf8(&raw_bytes) {
                        let rewritten = rewrite_html(html, &host);
                        self.inject_cookies(&rewritten).into_bytes()
                    } else {
                        raw_bytes.to_vec()
                    }
                } else if content_type.contains("text/css") {
                    if let Ok(css) = std::str::from_utf8(&raw_bytes) {
                        rewrite_css(css, &host).into_bytes()
                    } else {
                        raw_bytes.to_vec()
                    }
                } else {
                    raw_bytes.to_vec()
                };

                let _ = self.cache.put(
                    &url,
                    &content_type,
                    new_etag.as_deref(),
                    new_last_mod.as_deref(),
                    &bytes_to_store,
                );
            }
        }

        Ok(())
    }
}

fn build_response(status: StatusCode, content_type: &str, body: Vec<u8>) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header("Content-Type", content_type)
        .header("Access-Control-Allow-Origin", "*")
        .body(body)
        .unwrap_or_else(|_| default_error_response())
}

fn build_offline_miss_page(url: &CanonicalUrl) -> Response<Vec<u8>> {
    let html = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <title>Not Available Offline - OfflineWOL</title>
  <style>
    body {{
      font-family: system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
      padding: 40px 20px;
      text-align: center;
      background: #f7f7f7;
      color: #333;
    }}
    .box {{
      max-width: 500px;
      margin: 40px auto;
      background: #ffffff;
      padding: 30px;
      border-radius: 8px;
      box-shadow: 0 2px 10px rgba(0,0,0,0.08);
    }}
    h1 {{ font-size: 20px; margin-bottom: 12px; color: #4a6da7; }}
    p {{ font-size: 14px; line-height: 1.5; color: #666; }}
    .url {{ font-family: monospace; font-size: 12px; color: #888; word-break: break-all; margin: 15px 0; }}
    button {{
      margin-top: 15px;
      padding: 8px 18px;
      background: #4a6da7;
      color: white;
      border: none;
      border-radius: 4px;
      cursor: pointer;
      font-size: 14px;
    }}
    button:hover {{ background: #3b5785; }}
  </style>
</head>
<body>
  <div class="box">
    <h1>Page Not Available Offline</h1>
    <p>This article has not been cached yet. Connect to the internet and visit this page to make it available offline.</p>
    <div class="url">{url}</div>
    <button onclick="window.history.back()">Go Back</button>
    <button onclick="location.reload()" style="margin-left: 8px;">Retry</button>
  </div>
</body>
</html>"#
    );

    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "text/html; charset=utf-8")
        .body(html.into_bytes())
        .unwrap_or_else(|_| default_error_response())
}

fn default_error_response() -> Response<Vec<u8>> {
    Response::builder()
        .status(StatusCode::INTERNAL_SERVER_ERROR)
        .header("Content-Type", "text/plain")
        .body(Vec::from("Internal server error"))
        .unwrap()
}

pub fn inject_cookies_into_html(
    html: &str,
    cookies: &std::collections::HashMap<String, String>,
) -> String {
    let json = serde_json::to_string(cookies).unwrap_or_else(|_| "{}".to_string());
    let script = format!(r#"<script id="__wol_cookies_bootstrap">window.__WOL_INITIAL_COOKIES = {json};</script>"#);

    // If an existing bootstrap script is present, replace it with updated cookies
    if let Some(start) = html.find(r#"<script id="__wol_cookies_bootstrap">"#) {
        if let Some(end_rel) = html[start..].find("</script>") {
            let end = start + end_rel + "</script>".len();
            return format!("{}{}{}", &html[..start], script, &html[end..]);
        }
    }

    // Otherwise insert right after <head> or <head ...>
    if let Some(pos) = html.find("<head>") {
        let idx = pos + "<head>".len();
        format!("{}{}{}", &html[..idx], script, &html[idx..])
    } else if let Some(pos) = html.find("<head ") {
        if let Some(end_head) = html[pos..].find('>') {
            let idx = pos + end_head + 1;
            format!("{}{}{}", &html[..idx], script, &html[idx..])
        } else {
            format!("{}{}", script, html)
        }
    } else {
        format!("{}{}", script, html)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inject_cookies_into_html() {
        let html = "<html><head><title>Test</title></head><body>Hello</body></html>";
        let mut cookies = std::collections::HashMap::new();
        cookies.insert("cookieConsent-FUNCTIONAL".to_string(), "true".to_string());
        let result = inject_cookies_into_html(html, &cookies);
        assert!(result.contains(r#"<script id="__wol_cookies_bootstrap">"#));
        assert!(result.contains(r#""cookieConsent-FUNCTIONAL":"true""#));
        assert!(result.contains("<head><script id=\"__wol_cookies_bootstrap\">"));

        // Replacing existing bootstrap script with updated cookies
        cookies.insert("fontScale".to_string(), "1.2".to_string());
        let updated = inject_cookies_into_html(&result, &cookies);
        assert_eq!(
            updated.matches(r#"<script id="__wol_cookies_bootstrap">"#).count(),
            1
        );
        assert!(updated.contains(r#""fontScale":"1.2""#));
    }
}
