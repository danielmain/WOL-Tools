use std::fmt;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CanonicalUrl(String);

impl CanonicalUrl {
    pub fn new(url: String) -> Self {
        Self(url)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CanonicalUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum UrlMappingError {
    #[error("missing host in URI: {0}")]
    MissingHost(String),
    #[error("unsupported scheme: {0}")]
    UnsupportedScheme(String),
    #[error("invalid URI: {0}")]
    #[allow(dead_code)]
    InvalidUri(String),
}

/// Map a wol:// request URI to its canonical upstream https:// URL using the configured `base_host`.
pub fn map_wol_uri_to_canonical(
    uri: &tauri::http::Uri,
    base_host: &str,
) -> Result<CanonicalUrl, UrlMappingError> {
    let scheme = uri.scheme_str().unwrap_or("");
    if scheme != "wol" {
        return Err(UrlMappingError::UnsupportedScheme(scheme.to_string()));
    }

    let host = uri
        .host()
        .ok_or_else(|| UrlMappingError::MissingHost(uri.to_string()))?;

    let path = uri.path();
    let query_str = uri.query();

    let sorted_query = query_str.map(sort_query_string);

    let (upstream_host, upstream_path) = resolve_host_and_path(host, path, base_host);

    let canonical = match sorted_query {
        Some(q) if !q.is_empty() => format!("https://{upstream_host}{upstream_path}?{q}"),
        _ => format!("https://{upstream_host}{upstream_path}"),
    };

    Ok(CanonicalUrl(canonical))
}

fn resolve_host_and_path<'a>(
    host: &'a str,
    path: &'a str,
    base_host: &'a str,
) -> (&'a str, String) {
    if let Some(rest) = path.strip_prefix("/__cdn__/") {
        if let Some((cdn_host, subpath)) = rest.split_once('/') {
            return (cdn_host, format!("/{subpath}"));
        }
    }

    if host == base_host {
        return (base_host, path.to_string());
    }

    // External domain hosts (e.g. CDN hosts) always contain a dot, whereas language locales do not.
    if host.contains('.') {
        return (host, path.to_string());
    }

    // Subresources referenced from root on the configured base host
    if path.starts_with("/assets/")
        || path.starts_with("/icon/")
        || path.starts_with("/download/")
        || path.starts_with("/img/")
    {
        return (base_host, path.to_string());
    }

    // If browser resolved a root-relative link like /es/wol/... on host "es",
    // the path is "/es/wol/..."
    let host_prefix = format!("/{host}/");
    if path.starts_with(&host_prefix) {
        return (base_host, path.to_string());
    }

    // Navigation path on language host (e.g. host="es", path="/wol/h/r4/lp-s")
    if path.starts_with("/wol/") {
        return (base_host, format!("/{host}{path}"));
    }

    // Default fallback: prepend host if not already present
    (base_host, format!("/{host}{path}"))
}

fn sort_query_string(query: &str) -> String {
    let mut pairs: Vec<&str> = query.split('&').filter(|s| !s.is_empty()).collect();
    pairs.sort_unstable();
    pairs.join("&")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;
    use tauri::http::Uri;

    const BASE_HOST: &str = "wol.example.org";

    #[test]
    fn maps_language_home_navigation() {
        let uri = Uri::from_str("wol://es/wol/h/r4/lp-s").unwrap();
        let canonical = map_wol_uri_to_canonical(&uri, BASE_HOST).unwrap();
        assert_eq!(canonical.as_str(), "https://wol.example.org/es/wol/h/r4/lp-s");
    }

    #[test]
    fn maps_article_navigation() {
        let uri = Uri::from_str("wol://es/wol/d/r4/lp-s/1102026210").unwrap();
        let canonical = map_wol_uri_to_canonical(&uri, BASE_HOST).unwrap();
        assert_eq!(
            canonical.as_str(),
            "https://wol.example.org/es/wol/d/r4/lp-s/1102026210"
        );
    }

    #[test]
    fn maps_root_relative_assets_and_icons() {
        let uri_asset = Uri::from_str("wol://es/assets/css/test.css").unwrap();
        assert_eq!(
            map_wol_uri_to_canonical(&uri_asset, BASE_HOST).unwrap().as_str(),
            "https://wol.example.org/assets/css/test.css"
        );

        let uri_icon = Uri::from_str("wol://es/icon/favicon.ico").unwrap();
        assert_eq!(
            map_wol_uri_to_canonical(&uri_icon, BASE_HOST).unwrap().as_str(),
            "https://wol.example.org/icon/favicon.ico"
        );
    }

    #[test]
    fn maps_cdn_resources() {
        let uri_cdn_direct =
            Uri::from_str("wol://cdn.example.org/code/media-player/v3.3.0/media-player.js").unwrap();
        assert_eq!(
            map_wol_uri_to_canonical(&uri_cdn_direct, BASE_HOST).unwrap().as_str(),
            "https://cdn.example.org/code/media-player/v3.3.0/media-player.js"
        );

        let uri_cdn_proxied =
            Uri::from_str("wol://es/__cdn__/cdn.example.org/code/media-player/v3.3.0/media-player.js")
                .unwrap();
        assert_eq!(
            map_wol_uri_to_canonical(&uri_cdn_proxied, BASE_HOST).unwrap().as_str(),
            "https://cdn.example.org/code/media-player/v3.3.0/media-player.js"
        );
    }

    #[test]
    fn maps_base_host_directly() {
        let uri = Uri::from_str("wol://wol.example.org/es/wol/h/r4/lp-s").unwrap();
        assert_eq!(
            map_wol_uri_to_canonical(&uri, BASE_HOST).unwrap().as_str(),
            "https://wol.example.org/es/wol/h/r4/lp-s"
        );
    }

    #[test]
    fn deduplicates_host_when_browser_resolves_root_relative_path() {
        let uri = Uri::from_str("wol://es/es/wol/d/r4/lp-s/1102026210").unwrap();
        assert_eq!(
            map_wol_uri_to_canonical(&uri, BASE_HOST).unwrap().as_str(),
            "https://wol.example.org/es/wol/d/r4/lp-s/1102026210"
        );
    }

    #[test]
    fn sorts_query_parameters() {
        let uri = Uri::from_str("wol://es/wol/s/r4/lp-s?q=faith&sort=rel&p=1").unwrap();
        assert_eq!(
            map_wol_uri_to_canonical(&uri, BASE_HOST).unwrap().as_str(),
            "https://wol.example.org/es/wol/s/r4/lp-s?p=1&q=faith&sort=rel"
        );
    }

    #[test]
    fn maps_root_img_subresources_without_lang_prefix() {
        let uri = Uri::from_str("wol://es/img/wol-sprites-2021.1@2x.png").unwrap();
        assert_eq!(
            map_wol_uri_to_canonical(&uri, BASE_HOST).unwrap().as_str(),
            "https://wol.example.org/img/wol-sprites-2021.1@2x.png"
        );
    }
}
