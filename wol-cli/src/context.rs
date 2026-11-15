//! Loading a page for the terminal: disk cache first, network second.
//!
//! The CLI is a read-only client of the same `DiskCache` the desktop app fills,
//! so anything browsed in OfflineWOL is instantly readable in `wol read`, and
//! `wol read --offline` never touches the network at all.

use std::path::PathBuf;

use wol_core::cache::DiskCache;
use wol_core::dirs;
use wol_core::handler::{ProtocolHandler, DEFAULT_USER_AGENT};
use wol_core::http_types::Method;
use wol_core::state::StateManager;
use wol_core::url_map::{map_wol_uri_to_canonical, CanonicalUrl, Url};

const FALLBACK_HOST: &str = "wol.jw.org";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// Served from the on-disk cache without any network access.
    Cached,
    /// Served from the cache, then revalidated in the background.
    CacheRevalidated,
    /// Fetched from the network and stored.
    Live,
}

impl Source {
    pub fn label(self) -> &'static str {
        match self {
            Source::Cached => "cached",
            Source::CacheRevalidated => "cached (revalidating)",
            Source::Live => "live",
        }
    }
}

#[derive(Debug, Clone)]
pub struct LoadedPage {
    pub canonical: CanonicalUrl,
    pub html: String,
    pub source: Source,
}

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("could not map {0:?} to a WOL address: {1}")]
    BadUrl(String, String),
    #[error("no cached copy of {0} and offline mode is on (run it once online to cache it)")]
    OfflineMiss(CanonicalUrl),
    #[error("network request failed: {0}")]
    Network(String),
    #[error("upstream returned HTTP {0} for {1}")]
    Status(u16, CanonicalUrl),
    #[error("cache error: {0}")]
    Cache(String),
}

/// Environment shared by every subcommand.
#[derive(Clone)]
pub struct Context {
    pub data_dir: PathBuf,
    pub cache: DiskCache,
    pub state: StateManager,
}

impl Context {
    pub fn open() -> Result<Self, Box<dyn std::error::Error>> {
        let data_dir = dirs::app_data_dir_or_env();
        Ok(Self::open_in(data_dir)?)
    }

    pub fn open_in(data_dir: PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        let state = StateManager::load_or_init(&data_dir)?;
        let cap = state.get_settings().map(|s| s.cache_cap_bytes).unwrap_or(u64::MAX);
        let cache = DiskCache::open(&data_dir.join("cache"), cap)?;
        Ok(Self {
            data_dir,
            cache,
            state,
        })
    }

    pub fn base_host(&self) -> String {
        self.state
            .get_settings()
            .ok()
            .and_then(|settings| settings.base_host)
            .filter(|host| !host.trim().is_empty())
            .unwrap_or_else(|| FALLBACK_HOST.to_string())
    }

    /// Turn user input (`wol://…`, `/en/wol/d/…`, full URL, or a bare id) into a canonical URL.
    pub fn canonicalize(&self, target: &str) -> Result<CanonicalUrl, LoadError> {
        let base_host = self.base_host();
        let trimmed = target.trim();
        if trimmed.is_empty() {
            return Err(LoadError::BadUrl(target.to_string(), "empty reference".into()));
        }

        let (uri, host) = if let Ok(url) = Url::parse(trimmed) {
            match url.scheme() {
                "wol" => (url.clone(), base_host.clone()),
                "http" | "https" => {
                    let host = url.host_str().unwrap_or(&base_host).to_string();
                    let wol = format!("wol://{host}{}{}", url.path(), query_suffix(&url));
                    (Url::parse(&wol).map_err(|err| LoadError::BadUrl(trimmed.into(), err.to_string()))?, host)
                }
                other => {
                    return Err(LoadError::BadUrl(trimmed.into(), format!("unsupported scheme {other}")))
                }
            }
        } else {
            // Bare document id, path, or `en/wol/d/…`.
            let path = if trimmed.chars().all(|c| c.is_ascii_digit()) {
                let settings = self.state.get_settings().unwrap_or_default();
                let lib = wol_core::languages::find_by_locale(&settings.language)
                    .or_else(|| wol_core::languages::find_by_symbol(&settings.language))
                    .map(|language| language.lib.clone())
                    .unwrap_or_else(|| "lp-e".to_string());
                format!("/{}/wol/d/r{}/{lib}/{trimmed}", settings.language, settings.region)
            } else if trimmed.starts_with('/') {
                trimmed.to_string()
            } else {
                format!("/{trimmed}")
            };
            let wol = format!("wol://{base_host}{path}");
            (
                Url::parse(&wol).map_err(|err| LoadError::BadUrl(trimmed.into(), err.to_string()))?,
                base_host.clone(),
            )
        };

        map_wol_uri_to_canonical(&uri, &host).map_err(|err| LoadError::BadUrl(trimmed.into(), err.to_string()))
    }

    /// Cache-first load with optional network revalidation.
    pub async fn load(&self, target: &str, offline: bool) -> Result<LoadedPage, LoadError> {
        let canonical = self.canonicalize(target)?;
        match self.cache.get(&canonical) {
            Ok(Some((_, bytes))) => {
                let html = String::from_utf8_lossy(&bytes).into_owned();
                if offline {
                    return Ok(LoadedPage {
                        canonical,
                        html,
                        source: Source::Cached,
                    });
                }
                self.spawn_revalidation(canonical.clone());
                return Ok(LoadedPage {
                    canonical,
                    html,
                    source: Source::CacheRevalidated,
                });
            }
            Ok(None) => {}
            Err(err) => return Err(LoadError::Cache(err.to_string())),
        }

        if offline {
            return Err(LoadError::OfflineMiss(canonical));
        }
        self.fetch(canonical).await
    }

    fn spawn_revalidation(&self, canonical: CanonicalUrl) {
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let handler = ProtocolHandler::new(self.cache.clone());
        handle.spawn(async move {
            let _ = handler.revalidate(&canonical).await;
        });
    }

    /// Always hit the network (used by `wol cache crawl` and `--refresh`).
    pub async fn fetch(&self, canonical: CanonicalUrl) -> Result<LoadedPage, LoadError> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .user_agent(DEFAULT_USER_AGENT)
            .build()
            .map_err(|err| LoadError::Network(err.to_string()))?;
        let response = client
            .get(canonical.as_str())
            .send()
            .await
            .map_err(|err| LoadError::Network(err.to_string()))?;
        let status = response.status();
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("text/html; charset=utf-8")
            .to_string();
        let etag = response
            .headers()
            .get("etag")
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        let last_modified = response
            .headers()
            .get("last-modified")
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        if !status.is_success() {
            return Err(LoadError::Status(status.as_u16(), canonical));
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|err| LoadError::Network(err.to_string()))?
            .to_vec();
        self.cache
            .put(
                &canonical,
                &content_type,
                etag.as_deref(),
                last_modified.as_deref(),
                &bytes,
            )
            .map_err(|err| LoadError::Cache(err.to_string()))?;
        Ok(LoadedPage {
            canonical,
            html: String::from_utf8_lossy(&bytes).into_owned(),
            source: Source::Live,
        })
    }

    /// Fetch a non-HTML resource and remember the content type.
    pub async fn fetch_and_cache(&self, target: &CanonicalUrl) -> Result<Method, LoadError> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .user_agent(DEFAULT_USER_AGENT)
            .build()
            .map_err(|err| LoadError::Network(err.to_string()))?;
        let response = client
            .get(target.as_str())
            .send()
            .await
            .map_err(|err| LoadError::Network(err.to_string()))?;
        let status = response.status();
        if !status.is_success() {
            return Err(LoadError::Status(status.as_u16(), target.clone()));
        }
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("application/octet-stream")
            .to_string();
        let etag = response
            .headers()
            .get("etag")
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        let last_modified = response
            .headers()
            .get("last-modified")
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        let bytes = response
            .bytes()
            .await
            .map_err(|err| LoadError::Network(err.to_string()))?
            .to_vec();
        self.cache
            .put(target, &content_type, etag.as_deref(), last_modified.as_deref(), &bytes)
            .map_err(|err| LoadError::Cache(err.to_string()))?;
        Ok(Method::GET)
    }
}

fn query_suffix(url: &Url) -> String {
    match url.query() {
        Some(query) if !query.is_empty() => format!("?{query}"),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> (Context, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "wol-cli-ctx-{}-{}",
            std::process::id(),
            uuid_like()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        (Context::open_in(dir.clone()).expect("context"), dir)
    }

    /// Unique scratch-directory suffix. The clock alone collides when tests run
    /// in parallel threads, which made `canonicalizes_*` flaky.
    fn uuid_like() -> String {
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::time::{SystemTime, UNIX_EPOCH};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        format!("{nanos}-{}", COUNTER.fetch_add(1, Ordering::Relaxed))
    }

    #[test]
    fn canonicalizes_absolute_wol_and_https_urls() {
        let (context, dir) = context();
        let from_wol = context.canonicalize("wol://es/wol/d/r4/lp-s/1102026210").unwrap();
        let from_https = context
            .canonicalize("https://wol.jw.org/es/wol/d/r4/lp-s/1102026210")
            .unwrap();
        assert_eq!(from_wol.as_str(), "https://wol.jw.org/es/wol/d/r4/lp-s/1102026210");
        assert_eq!(from_https.as_str(), from_wol.as_str());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn canonicalizes_bare_paths_and_sorts_query() {
        let (context, dir) = context();
        let path = context.canonicalize("/es/wol/s/r4/lp-s?q=a&p=1").unwrap();
        assert_eq!(
            path.as_str(),
            "https://wol.jw.org/es/wol/s/r4/lp-s?p=1&q=a"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn rejects_unknown_schemes() {
        let (context, dir) = context();
        assert!(matches!(
            context.canonicalize("ftp://example.org/x"),
            Err(LoadError::BadUrl(_, _))
        ));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn offline_mode_reports_a_miss_without_network() {
        let (context, dir) = context();
        let result = context
            .load("/en/wol/d/r1/lp-e/999999999", true)
            .await;
        assert!(matches!(result, Err(LoadError::OfflineMiss(_))));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn cached_pages_are_served_offline() {
        let (context, dir) = context();
        let canonical = context.canonicalize("/en/wol/d/r1/lp-e/123456").unwrap();
        context
            .cache
            .put(&canonical, "text/html; charset=utf-8", None, None, b"<p data-pid=\"1\">Hi</p>")
            .unwrap();
        let page = context.load("/en/wol/d/r1/lp-e/123456", true).await.unwrap();
        assert!(page.html.contains("data-pid"));
        assert_eq!(page.source, Source::Cached);
        let _ = std::fs::remove_dir_all(dir);
    }
}
