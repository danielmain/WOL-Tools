use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::Semaphore;

use crate::cache::{CacheError, DiskCache};
use crate::rewrite::{rewrite_css, rewrite_html};
use crate::url_map::CanonicalUrl;

#[derive(Debug, Error)]
pub enum CrawlerError {
    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("cache error: {0}")]
    Cache(#[from] CacheError),
    #[error("invalid url: {0}")]
    InvalidUrl(String),
    #[error("an offline download is already in progress")]
    AlreadyRunning,
    #[error("rate limited by server (HTTP {0})")]
    RateLimited(u16),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrawlProgress {
    pub total_resources: usize,
    pub completed_resources: usize,
    pub bytes_downloaded: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BibleCrawlProgress {
    pub phase: String,
    pub total_chapters: usize,
    pub completed_chapters: usize,
    pub failed_chapters: usize,
    pub bytes_downloaded: u64,
    pub done: bool,
    pub cancelled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BibleCrawlResult {
    pub binav_url: String,
    pub books_count: usize,
    pub total_chapters: usize,
    pub completed_chapters: usize,
    pub failed_chapters: usize,
    pub bytes_total: u64,
    pub cancelled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryCrawlProgress {
    pub phase: String,
    pub total_articles: usize,
    pub completed_articles: usize,
    pub failed_articles: usize,
    pub categories_scanned: usize,
    pub bytes_downloaded: u64,
    pub done: bool,
    pub cancelled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryCrawlResult {
    pub library_url: String,
    pub categories_count: usize,
    pub total_articles: usize,
    pub completed_articles: usize,
    pub failed_articles: usize,
    pub bytes_total: u64,
    pub cancelled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrawlResult {
    pub page_url: String,
    pub subresources_count: usize,
    pub downloaded_count: usize,
    pub bytes_total: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueCrawlResult {
    pub toc_url: String,
    pub articles_count: usize,
    pub total_subresources: usize,
    pub downloaded_count: usize,
    pub bytes_total: u64,
}

fn base_host_of(url: &CanonicalUrl) -> String {
    url::Url::parse(url.as_str())
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
        .unwrap_or_default()
}

/// Extract subresource URLs referenced in an HTML document
pub fn extract_subresource_urls(html: &str, base_host: &str) -> Vec<CanonicalUrl> {
    let mut urls = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // 1. Root-relative subresources and wol:// references
    let re = regex::Regex::new(r#"(?:src|href)=["']([^"']+)["']"#)
        .expect("static regex must compile");

    for cap in re.captures_iter(html) {
        let raw = &cap[1];
        let canonical_opt = if raw.starts_with("/assets/")
            || raw.starts_with("/icon/")
            || raw.starts_with("/img/")
            || (raw.starts_with('/') && raw.contains("/wol/mp/"))
        {
            Some(format!("https://{base_host}{raw}"))
        } else if raw.starts_with("wol://") {
            if let Ok(uri) = raw.parse::<tauri::http::Uri>() {
                crate::url_map::map_wol_uri_to_canonical(&uri, base_host)
                    .ok()
                    .map(|c| c.as_str().to_string())
            } else {
                None
            }
        } else {
            None
        };

        if let Some(c) = canonical_opt {
            if seen.insert(c.clone()) {
                urls.push(CanonicalUrl::new(c));
            }
        }
    }

    // 2. External CDN subresources in <link>, <script>, <img>
    let ext_re = regex::Regex::new(
        r#"<(?:link|script|img)\b[^>]*?\b(?:href|src)=["']((?:https?:)?//[^"']+)["']"#,
    )
    .expect("static ext subresource regex must compile");
    for cap in ext_re.captures_iter(html) {
        let raw = &cap[1];
        let canonical = if raw.starts_with("//") {
            format!("https:{raw}")
        } else {
            raw.to_string()
        };
        if seen.insert(canonical.clone()) {
            urls.push(CanonicalUrl::new(canonical));
        }
    }

    urls
}

/// Extract article links from a table-of-contents or library page
pub fn extract_issue_article_urls(html: &str, base_host: &str) -> Vec<CanonicalUrl> {
    let mut urls = Vec::new();
    let mut seen = HashSet::new();

    let re = regex::Regex::new(r#"href=["']([^"']*?/wol/d/r\d+/lp-[^"']+)["']"#)
        .expect("static regex must compile");

    for cap in re.captures_iter(html) {
        let raw = &cap[1];
        if let Some(path) = extract_wol_path(raw, base_host) {
            let clean = path.split('#').next().unwrap_or(&path);
            let canonical = format!("https://{base_host}{clean}");
            if seen.insert(canonical.clone()) {
                urls.push(CanonicalUrl::new(canonical));
            }
        }
    }

    urls
}

/// Normalize a raw `href` or canonical URL into a `/...` path on `base_host`.
fn extract_wol_path(raw: &str, base_host: &str) -> Option<String> {
    let without_frag = raw.split('#').next().unwrap_or(raw);
    let without_query = without_frag.split('?').next().unwrap_or(without_frag);
    let https_prefix = format!("https://{base_host}");
    let http_prefix = format!("http://{base_host}");
    if let Some(rest) = without_query.strip_prefix(&https_prefix) {
        Some(rest.to_string())
    } else if let Some(rest) = without_query.strip_prefix(&http_prefix) {
        Some(rest.to_string())
    } else if let Some(rest) = without_query.strip_prefix("wol://") {
        let trimmed = rest.trim_start_matches('/');
        if trimmed.is_empty() {
            None
        } else {
            Some(format!("/{trimmed}"))
        }
    } else if without_query.starts_with('/') {
        Some(without_query.to_string())
    } else {
        None
    }
}

/// Decode `%XX` sequences in a URL path so percent-encoded and raw UTF-8 paths compare equal.
fn percent_decode_path(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                out.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Extract child `/wol/library/...` URLs that are strictly nested under `parent_url`.
/// Breadcrumb links to ancestor categories and self-links are excluded.
pub fn extract_library_child_urls(
    html: &str,
    parent_url: &CanonicalUrl,
    base_host: &str,
) -> Vec<CanonicalUrl> {
    let Some(parent_path_raw) = extract_wol_path(parent_url.as_str(), base_host) else {
        return Vec::new();
    };
    let parent_decoded = percent_decode_path(parent_path_raw.trim_end_matches('/'));
    let child_prefix = format!("{parent_decoded}/");

    let mut urls = Vec::new();
    let mut seen = HashSet::new();

    let re = regex::Regex::new(r#"href=["']([^"']*?/wol/library/r\d+/lp-[^"']+)["']"#)
        .expect("static regex must compile");

    for cap in re.captures_iter(html) {
        let raw = &cap[1];
        let Some(path) = extract_wol_path(raw, base_host) else {
            continue;
        };
        let path_trimmed = path.trim_end_matches('/');
        let decoded = percent_decode_path(path_trimmed);
        if decoded.starts_with(&child_prefix) && decoded.len() > child_prefix.len() {
            let canonical = format!("https://{base_host}{path_trimmed}");
            if seen.insert(decoded) {
                urls.push(CanonicalUrl::new(canonical));
            }
        }
    }

    urls
}

/// Extract Bible book index URLs from a `binav` page
/// (e.g. `/es/wol/binav/r4/lp-s/nwtsty/1`).
pub fn extract_bible_book_urls(html: &str, base_host: &str) -> Vec<CanonicalUrl> {
    let mut urls = Vec::new();
    let mut seen = HashSet::new();

    let re = regex::Regex::new(
        r#"href=["'](?:https?://[^/"']+|wol://)?(/[^"'#]+/wol/binav/r\d+/lp-[^/"']+/[^/"']+/\d+)(?:#[^"']*)?["']"#,
    )
    .expect("static regex must compile");

    for cap in re.captures_iter(html) {
        let path = &cap[1];
        let canonical = format!("https://{base_host}{path}");
        if seen.insert(canonical.clone()) {
            urls.push(CanonicalUrl::new(canonical));
        }
    }

    urls
}

/// Extract Bible chapter URLs from a book page
/// (e.g. `/es/wol/b/r4/lp-s/nwtsty/1/1`).
pub fn extract_bible_chapter_urls(html: &str, base_host: &str) -> Vec<CanonicalUrl> {
    let mut urls = Vec::new();
    let mut seen = HashSet::new();

    let re = regex::Regex::new(
        r#"href=["'](?:https?://[^/"']+|wol://)?(/[^"'#]+/wol/b/r\d+/lp-[^/"']+/[^/"']+/\d+/\d+)(?:#[^"']*)?["']"#,
    )
    .expect("static regex must compile");

    for cap in re.captures_iter(html) {
        let path = &cap[1];
        let canonical = format!("https://{base_host}{path}");
        if seen.insert(canonical.clone()) {
            urls.push(CanonicalUrl::new(canonical));
        }
    }

    urls
}

/// Derive the language segment (e.g. `"es"`, `"en"`) from a canonical URL.
fn extract_lang_from_canonical(url: &CanonicalUrl) -> String {
    let first_seg = url::Url::parse(url.as_str())
        .ok()
        .and_then(|u| {
            u.path_segments()
                .and_then(|mut segs| segs.next().map(str::to_string))
        })
        .unwrap_or_default();
    if first_seg.is_empty() || first_seg == "wol" {
        "en".to_string()
    } else {
        first_seg
    }
}

struct RunningGuard(Arc<AtomicBool>);

impl Drop for RunningGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

pub struct Crawler {
    cache: DiskCache,
    client: reqwest::Client,
    semaphore: Arc<Semaphore>,
    bible_cancel: Arc<AtomicBool>,
    bible_running: Arc<AtomicBool>,
}

impl Crawler {
    pub fn new(cache: DiskCache) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .user_agent(crate::handler::DEFAULT_USER_AGENT)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self {
            cache,
            client,
            semaphore: Arc::new(Semaphore::new(4)), // Politeness: <= 4 concurrent
            bible_cancel: Arc::new(AtomicBool::new(false)),
            bible_running: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn new_with_client(cache: DiskCache, client: reqwest::Client) -> Self {
        Self {
            cache,
            client,
            semaphore: Arc::new(Semaphore::new(4)),
            bible_cancel: Arc::new(AtomicBool::new(false)),
            bible_running: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Request cancellation of any active Bible or library crawl.
    pub fn cancel_bible_crawl(&self) {
        self.bible_cancel.store(true, Ordering::SeqCst);
    }

    /// Request cancellation of any active library crawl.
    pub fn cancel_library_crawl(&self) {
        self.bible_cancel.store(true, Ordering::SeqCst);
    }

    /// Return whether a batch crawl (Bible or library) is currently running.
    pub fn is_bible_crawl_running(&self) -> bool {
        self.bible_running.load(Ordering::SeqCst)
    }

    /// Crawl and save a single page and all of its referenced subresources.
    pub async fn crawl_page(&self, page_url: &CanonicalUrl) -> Result<CrawlResult, CrawlerError> {
        let base_host = base_host_of(page_url);
        let (html_body, _content_type) = self.fetch_and_cache_resource(page_url, true).await?;
        let html_str = String::from_utf8_lossy(&html_body);
        let subresources = extract_subresource_urls(&html_str, &base_host);

        let mut downloaded_count = 0;
        let mut bytes_total = html_body.len() as u64;

        let mut join_set = tokio::task::JoinSet::new();

        for sub_url in subresources.clone() {
            let this_client = self.client.clone();
            let this_cache = self.cache.clone();
            let sem = self.semaphore.clone();
            let sub_host = base_host.clone();

            join_set.spawn(async move {
                let _permit = match sem.acquire().await {
                    Ok(p) => p,
                    Err(_) => return (sub_url, 0, false),
                };

                // Small delay for politeness
                tokio::time::sleep(Duration::from_millis(15)).await;

                // Check if already in cache
                if let Ok(Some((_, bytes))) = this_cache.get(&sub_url) {
                    return (sub_url, bytes.len() as u64, false);
                }

                // Fetch from network
                match this_client.get(sub_url.as_str()).send().await {
                    Ok(resp) => {
                        let status = resp.status();
                        if !status.is_success() {
                            return (sub_url, 0, false);
                        }
                        let content_type = resp
                            .headers()
                            .get(reqwest::header::CONTENT_TYPE)
                            .and_then(|v| v.to_str().ok())
                            .unwrap_or("application/octet-stream")
                            .to_string();
                        let etag = resp
                            .headers()
                            .get(reqwest::header::ETAG)
                            .and_then(|v| v.to_str().ok())
                            .map(str::to_string);
                        let last_mod = resp
                            .headers()
                            .get(reqwest::header::LAST_MODIFIED)
                            .and_then(|v| v.to_str().ok())
                            .map(str::to_string);

                        if let Ok(raw_bytes) = resp.bytes().await {
                            let mut data = raw_bytes.to_vec();
                            if content_type.contains("css") {
                                let text = String::from_utf8_lossy(&data);
                                data = rewrite_css(&text, &sub_host).into_bytes();
                            }
                            let _ = this_cache.put(
                                &sub_url,
                                &content_type,
                                etag.as_deref(),
                                last_mod.as_deref(),
                                &data,
                            );
                            (sub_url, data.len() as u64, true)
                        } else {
                            (sub_url, 0, false)
                        }
                    }
                    Err(_) => (sub_url, 0, false),
                }
            });
        }

        while let Some(res) = join_set.join_next().await {
            if let Ok((_sub_url, len, was_downloaded)) = res {
                bytes_total += len;
                if was_downloaded {
                    downloaded_count += 1;
                }
            }
        }

        Ok(CrawlResult {
            page_url: page_url.as_str().to_string(),
            subresources_count: subresources.len(),
            downloaded_count,
            bytes_total,
        })
    }

    /// Crawl a table of contents page and all child articles
    pub async fn crawl_issue(&self, toc_url: &CanonicalUrl) -> Result<IssueCrawlResult, CrawlerError> {
        let base_host = base_host_of(toc_url);
        let (html_body, _) = self.fetch_and_cache_resource(toc_url, true).await?;
        let html_str = String::from_utf8_lossy(&html_body);
        let article_urls = extract_issue_article_urls(&html_str, &base_host);

        let mut total_subresources = 0;
        let mut downloaded_count = 0;
        let mut bytes_total = html_body.len() as u64;

        for article_url in &article_urls {
            if let Ok(res) = self.crawl_page(article_url).await {
                total_subresources += res.subresources_count;
                downloaded_count += res.downloaded_count;
                bytes_total += res.bytes_total;
            }
        }

        Ok(IssueCrawlResult {
            toc_url: toc_url.as_str().to_string(),
            articles_count: article_urls.len(),
            total_subresources,
            downloaded_count,
            bytes_total,
        })
    }

    /// Crawl a Bible `binav` index (or single book page) and all of its chapters.
    /// Reuses the shared disk cache so interrupted crawls resume cleanly.
    pub async fn crawl_bible<F>(
        &self,
        binav_url: &CanonicalUrl,
        mut on_progress: F,
    ) -> Result<BibleCrawlResult, CrawlerError>
    where
        F: FnMut(BibleCrawlProgress),
    {
        if self
            .bible_running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err(CrawlerError::AlreadyRunning);
        }
        self.bible_cancel.store(false, Ordering::SeqCst);
        let _guard = RunningGuard(self.bible_running.clone());

        let mut seen_subresources: HashSet<String> = HashSet::new();
        let mut bytes_total: u64 = 0;

        on_progress(BibleCrawlProgress {
            phase: "indexing".to_string(),
            total_chapters: 0,
            completed_chapters: 0,
            failed_chapters: 0,
            bytes_downloaded: 0,
            done: false,
            cancelled: false,
        });

        let base_host = base_host_of(binav_url);
        let (root_html, _, root_from_cache) =
            self.fetch_and_cache_resource_tracked(binav_url, true).await?;
        if !root_from_cache {
            bytes_total += root_html.len() as u64;
        }
        let root_str = String::from_utf8_lossy(&root_html);
        bytes_total += self
            .cache_unseen_subresources(&root_str, &base_host, &mut seen_subresources)
            .await;

        let book_urls = extract_bible_book_urls(&root_str, &base_host);
        let mut chapter_urls: Vec<CanonicalUrl> = Vec::new();
        let mut seen_chapters: HashSet<String> = HashSet::new();
        let books_count;

        if book_urls.is_empty() {
            // User may have triggered on a single book page directly.
            books_count = 1;
            for ch in extract_bible_chapter_urls(&root_str, &base_host) {
                if seen_chapters.insert(ch.as_str().to_string()) {
                    chapter_urls.push(ch);
                }
            }
        } else {
            books_count = book_urls.len();
            for book_url in &book_urls {
                if self.bible_cancel.load(Ordering::SeqCst) {
                    on_progress(BibleCrawlProgress {
                        phase: "cancelled".to_string(),
                        total_chapters: chapter_urls.len(),
                        completed_chapters: 0,
                        failed_chapters: 0,
                        bytes_downloaded: bytes_total,
                        done: true,
                        cancelled: true,
                    });
                    return Ok(BibleCrawlResult {
                        binav_url: binav_url.as_str().to_string(),
                        books_count,
                        total_chapters: chapter_urls.len(),
                        completed_chapters: 0,
                        failed_chapters: 0,
                        bytes_total,
                        cancelled: true,
                    });
                }

                let (book_html, _, from_cache) = self
                    .fetch_and_cache_resource_tracked(book_url, true)
                    .await?;
                if !from_cache {
                    bytes_total += book_html.len() as u64;
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }
                let book_str = String::from_utf8_lossy(&book_html);
                bytes_total += self
                    .cache_unseen_subresources(&book_str, &base_host, &mut seen_subresources)
                    .await;

                for ch in extract_bible_chapter_urls(&book_str, &base_host) {
                    if seen_chapters.insert(ch.as_str().to_string()) {
                        chapter_urls.push(ch);
                    }
                }
            }
        }

        let total_chapters = chapter_urls.len();
        let mut completed_chapters: usize = 0;
        let mut failed_chapters: usize = 0;
        let mut cancelled = false;

        on_progress(BibleCrawlProgress {
            phase: "downloading".to_string(),
            total_chapters,
            completed_chapters,
            failed_chapters,
            bytes_downloaded: bytes_total,
            done: false,
            cancelled: false,
        });

        for (idx, chap_url) in chapter_urls.iter().enumerate() {
            if self.bible_cancel.load(Ordering::SeqCst) {
                cancelled = true;
                break;
            }

            match self.fetch_and_cache_resource_tracked(chap_url, true).await {
                Ok((chap_html, _, from_cache)) => {
                    completed_chapters += 1;
                    if !from_cache {
                        bytes_total += chap_html.len() as u64;
                        let chap_str = String::from_utf8_lossy(&chap_html);
                        bytes_total += self
                            .cache_unseen_subresources(&chap_str, &base_host, &mut seen_subresources)
                            .await;
                        tokio::time::sleep(Duration::from_millis(30)).await;
                    }
                }
                Err(CrawlerError::RateLimited(code)) => {
                    return Err(CrawlerError::RateLimited(code));
                }
                Err(_) => {
                    failed_chapters += 1;
                }
            }

            if (idx + 1) % 5 == 0 || idx + 1 == total_chapters {
                on_progress(BibleCrawlProgress {
                    phase: "downloading".to_string(),
                    total_chapters,
                    completed_chapters,
                    failed_chapters,
                    bytes_downloaded: bytes_total,
                    done: false,
                    cancelled: false,
                });
            }
        }

        on_progress(BibleCrawlProgress {
            phase: if cancelled { "cancelled" } else { "complete" }.to_string(),
            total_chapters,
            completed_chapters,
            failed_chapters,
            bytes_downloaded: bytes_total,
            done: true,
            cancelled,
        });

        Ok(BibleCrawlResult {
            binav_url: binav_url.as_str().to_string(),
            books_count,
            total_chapters,
            completed_chapters,
            failed_chapters,
            bytes_total,
            cancelled,
        })
    }

    /// Crawl a `/wol/library/...` publication or category page recursively.
    /// Discovers nested `/wol/library/...` child categories (ignoring ancestor breadcrumbs)
    /// and downloads every `/wol/d/...` article found, reusing the shared disk cache.
    pub async fn crawl_library<F>(
        &self,
        library_url: &CanonicalUrl,
        mut on_progress: F,
    ) -> Result<LibraryCrawlResult, CrawlerError>
    where
        F: FnMut(LibraryCrawlProgress),
    {
        const MAX_LIBRARY_CATEGORIES: usize = 500;

        if self
            .bible_running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err(CrawlerError::AlreadyRunning);
        }
        self.bible_cancel.store(false, Ordering::SeqCst);
        let _guard = RunningGuard(self.bible_running.clone());

        let mut seen_subresources: HashSet<String> = HashSet::new();
        let mut bytes_total: u64 = 0;

        on_progress(LibraryCrawlProgress {
            phase: "indexing".to_string(),
            total_articles: 0,
            completed_articles: 0,
            failed_articles: 0,
            categories_scanned: 0,
            bytes_downloaded: 0,
            done: false,
            cancelled: false,
        });

        let base_host = base_host_of(library_url);
        let root_key = extract_wol_path(library_url.as_str(), &base_host)
            .map(|p| percent_decode_path(p.trim_end_matches('/')))
            .unwrap_or_else(|| library_url.as_str().to_string());

        let mut queue: VecDeque<CanonicalUrl> = VecDeque::from([library_url.clone()]);
        let mut seen_categories: HashSet<String> = HashSet::from([root_key]);
        let mut article_urls: Vec<CanonicalUrl> = Vec::new();
        let mut seen_articles: HashSet<String> = HashSet::new();
        let mut categories_scanned: usize = 0;

        while let Some(cat_url) = queue.pop_front() {
            if self.bible_cancel.load(Ordering::SeqCst) {
                on_progress(LibraryCrawlProgress {
                    phase: "cancelled".to_string(),
                    total_articles: article_urls.len(),
                    completed_articles: 0,
                    failed_articles: 0,
                    categories_scanned,
                    bytes_downloaded: bytes_total,
                    done: true,
                    cancelled: true,
                });
                return Ok(LibraryCrawlResult {
                    library_url: library_url.as_str().to_string(),
                    categories_count: categories_scanned,
                    total_articles: article_urls.len(),
                    completed_articles: 0,
                    failed_articles: 0,
                    bytes_total,
                    cancelled: true,
                });
            }

            let is_root = categories_scanned == 0;
            let fetched = self.fetch_and_cache_resource_tracked(&cat_url, true).await;
            let (cat_html, _, from_cache) = match fetched {
                Ok(v) => v,
                Err(CrawlerError::RateLimited(code)) => return Err(CrawlerError::RateLimited(code)),
                Err(e) if is_root => return Err(e),
                Err(_) => continue,
            };

            categories_scanned += 1;
            if !from_cache {
                bytes_total += cat_html.len() as u64;
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
            let cat_str = String::from_utf8_lossy(&cat_html);
            bytes_total += self
                .cache_unseen_subresources(&cat_str, &base_host, &mut seen_subresources)
                .await;

            for art in extract_issue_article_urls(&cat_str, &base_host) {
                if seen_articles.insert(art.as_str().to_string()) {
                    article_urls.push(art);
                }
            }

            for child in extract_library_child_urls(&cat_str, &cat_url, &base_host) {
                if seen_categories.len() >= MAX_LIBRARY_CATEGORIES {
                    break;
                }
                let child_key = extract_wol_path(child.as_str(), &base_host)
                    .map(|p| percent_decode_path(p.trim_end_matches('/')))
                    .unwrap_or_else(|| child.as_str().to_string());
                if seen_categories.insert(child_key) {
                    queue.push_back(child);
                }
            }

            on_progress(LibraryCrawlProgress {
                phase: "indexing".to_string(),
                total_articles: article_urls.len(),
                completed_articles: 0,
                failed_articles: 0,
                categories_scanned,
                bytes_downloaded: bytes_total,
                done: false,
                cancelled: false,
            });
        }

        let total_articles = article_urls.len();
        let mut completed_articles: usize = 0;
        let mut failed_articles: usize = 0;
        let mut cancelled = false;

        on_progress(LibraryCrawlProgress {
            phase: "downloading".to_string(),
            total_articles,
            completed_articles,
            failed_articles,
            categories_scanned,
            bytes_downloaded: bytes_total,
            done: false,
            cancelled: false,
        });

        for (idx, art_url) in article_urls.iter().enumerate() {
            if self.bible_cancel.load(Ordering::SeqCst) {
                cancelled = true;
                break;
            }

            match self.fetch_and_cache_resource_tracked(art_url, true).await {
                Ok((art_html, _, from_cache)) => {
                    completed_articles += 1;
                    if !from_cache {
                        bytes_total += art_html.len() as u64;
                        let art_str = String::from_utf8_lossy(&art_html);
                        bytes_total += self
                            .cache_unseen_subresources(&art_str, &base_host, &mut seen_subresources)
                            .await;
                        tokio::time::sleep(Duration::from_millis(30)).await;
                    }
                }
                Err(CrawlerError::RateLimited(code)) => {
                    return Err(CrawlerError::RateLimited(code));
                }
                Err(_) => {
                    failed_articles += 1;
                }
            }

            if (idx + 1) % 5 == 0 || idx + 1 == total_articles {
                on_progress(LibraryCrawlProgress {
                    phase: "downloading".to_string(),
                    total_articles,
                    completed_articles,
                    failed_articles,
                    categories_scanned,
                    bytes_downloaded: bytes_total,
                    done: false,
                    cancelled: false,
                });
            }
        }

        on_progress(LibraryCrawlProgress {
            phase: if cancelled { "cancelled" } else { "complete" }.to_string(),
            total_articles,
            completed_articles,
            failed_articles,
            categories_scanned,
            bytes_downloaded: bytes_total,
            done: true,
            cancelled,
        });

        Ok(LibraryCrawlResult {
            library_url: library_url.as_str().to_string(),
            categories_count: categories_scanned,
            total_articles,
            completed_articles,
            failed_articles,
            bytes_total,
            cancelled,
        })
    }

    async fn cache_unseen_subresources(
        &self,
        html: &str,
        base_host: &str,
        seen: &mut HashSet<String>,
    ) -> u64 {
        let mut candidates = Vec::new();
        for sub in extract_subresource_urls(html, base_host) {
            if seen.insert(sub.as_str().to_string()) {
                candidates.push(sub);
            }
        }
        if candidates.is_empty() {
            return 0;
        }

        let mut join_set = tokio::task::JoinSet::new();
        for sub_url in candidates {
            let client = self.client.clone();
            let cache = self.cache.clone();
            let sem = self.semaphore.clone();
            let css_host = base_host.to_string();

            join_set.spawn(async move {
                let _permit = match sem.acquire().await {
                    Ok(p) => p,
                    Err(_) => return 0u64,
                };
                if let Ok(Some(_)) = cache.get(&sub_url) {
                    return 0;
                }
                tokio::time::sleep(Duration::from_millis(15)).await;
                let Ok(resp) = client.get(sub_url.as_str()).send().await else {
                    return 0;
                };
                if !resp.status().is_success() {
                    return 0;
                }
                let content_type = resp
                    .headers()
                    .get(reqwest::header::CONTENT_TYPE)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("application/octet-stream")
                    .to_string();
                let etag = resp
                    .headers()
                    .get(reqwest::header::ETAG)
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_string);
                let last_mod = resp
                    .headers()
                    .get(reqwest::header::LAST_MODIFIED)
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_string);
                let Ok(raw_bytes) = resp.bytes().await else {
                    return 0;
                };
                let raw_bytes = raw_bytes.to_vec();
                let stored_bytes = if content_type.contains("css") {
                    let text = String::from_utf8_lossy(&raw_bytes);
                    rewrite_css(&text, &css_host).into_bytes()
                } else {
                    raw_bytes
                };
                let len = stored_bytes.len() as u64;
                let _ = cache.put(
                    &sub_url,
                    &content_type,
                    etag.as_deref(),
                    last_mod.as_deref(),
                    &stored_bytes,
                );
                len
            });
        }

        let mut total = 0u64;
        while let Some(res) = join_set.join_next().await {
            if let Ok(bytes) = res {
                total += bytes;
            }
        }
        total
    }

    async fn fetch_and_cache_resource(
        &self,
        url: &CanonicalUrl,
        is_html: bool,
    ) -> Result<(Vec<u8>, String), CrawlerError> {
        let (bytes, ct, _) = self.fetch_and_cache_resource_tracked(url, is_html).await?;
        Ok((bytes, ct))
    }

    async fn fetch_and_cache_resource_tracked(
        &self,
        url: &CanonicalUrl,
        is_html: bool,
    ) -> Result<(Vec<u8>, String, bool), CrawlerError> {
        if let Ok(Some((entry, bytes))) = self.cache.get(url) {
            return Ok((bytes, entry.content_type, true));
        }

        let resp = self.client.get(url.as_str()).send().await?;
        let status = resp.status();
        if status.as_u16() == 403 || status.as_u16() == 429 {
            return Err(CrawlerError::RateLimited(status.as_u16()));
        }
        if !status.is_success() {
            return Err(CrawlerError::InvalidUrl(format!("HTTP {}", status)));
        }

        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("text/html; charset=utf-8")
            .to_string();
        let etag = resp
            .headers()
            .get(reqwest::header::ETAG)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let last_mod = resp
            .headers()
            .get(reqwest::header::LAST_MODIFIED)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);

        let raw_bytes = resp.bytes().await?.to_vec();
        let stored_bytes = if is_html {
            let text = String::from_utf8_lossy(&raw_bytes);
            let lang = extract_lang_from_canonical(url);
            rewrite_html(&text, &lang).into_bytes()
        } else if content_type.contains("css") {
            let text = String::from_utf8_lossy(&raw_bytes);
            let host = base_host_of(url);
            rewrite_css(&text, &host).into_bytes()
        } else {
            raw_bytes
        };

        self.cache.put(
            url,
            &content_type,
            etag.as_deref(),
            last_mod.as_deref(),
            &stored_bytes,
        )?;

        Ok((stored_bytes, content_type, false))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE_HOST: &str = "wol.example.org";

    #[test]
    fn extracts_subresources_from_html() {
        let sample = r#"
            <html>
                <head>
                    <link rel="stylesheet" href="/assets/css/style.css">
                    <script src="https://cdn.example.org/code/media-player/v3.3.0/js/media-player.js"></script>
                    <link rel="icon" href="/icon/favicon-32x32.png">
                </head>
                <body>
                    <img src="/assets/img/logo.png">
                    <img src="/es/wol/mp/r4/lp-s/nwtsty/2026/317">
                    <a href="/es/wol/d/r4/lp-s/1102026210">Article</a>
                </body>
            </html>
        "#;

        let urls = extract_subresource_urls(sample, BASE_HOST);
        assert_eq!(urls.len(), 5);
        assert!(urls.iter().any(|u| u.as_str() == "https://wol.example.org/assets/css/style.css"));
        assert!(urls.iter().any(|u| u.as_str() == "https://cdn.example.org/code/media-player/v3.3.0/js/media-player.js"));
        assert!(urls.iter().any(|u| u.as_str() == "https://wol.example.org/icon/favicon-32x32.png"));
        assert!(urls.iter().any(|u| u.as_str() == "https://wol.example.org/assets/img/logo.png"));
        assert!(urls.iter().any(|u| u.as_str() == "https://wol.example.org/es/wol/mp/r4/lp-s/nwtsty/2026/317"));
    }

    #[test]
    fn extracts_issue_articles_from_toc() {
        let sample = r#"
            <ul class="toc">
                <li><a href="/es/wol/d/r4/lp-s/1102026210#h=1:0">Article 1</a></li>
                <li><a href="/es/wol/d/r4/lp-s/1102026211">Article 2</a></li>
                <li><a href="/other/page">Non-article</a></li>
            </ul>
        "#;

        let articles = extract_issue_article_urls(sample, BASE_HOST);
        assert_eq!(articles.len(), 2);
        assert_eq!(articles[0].as_str(), "https://wol.example.org/es/wol/d/r4/lp-s/1102026210");
        assert_eq!(articles[1].as_str(), "https://wol.example.org/es/wol/d/r4/lp-s/1102026211");
    }

    #[test]
    fn extracts_bible_books_and_chapters() {
        let binav_html = r#"
            <ul>
                <li><a href="/es/wol/binav/r4/lp-s/nwtsty/1">Génesis</a></li>
                <li><a href="/es/wol/binav/r4/lp-s/nwtsty/2#top">Éxodo</a></li>
                <li><a href="/es/wol/binav/r4/lp-s/nwtsty/1">Génesis dup</a></li>
                <li><a href="/es/wol/publication/r4/lp-s/nwtsty/0">Intro</a></li>
            </ul>
        "#;
        let books = extract_bible_book_urls(binav_html, BASE_HOST);
        assert_eq!(books.len(), 2);
        assert_eq!(
            books[0].as_str(),
            "https://wol.example.org/es/wol/binav/r4/lp-s/nwtsty/1"
        );
        assert_eq!(
            books[1].as_str(),
            "https://wol.example.org/es/wol/binav/r4/lp-s/nwtsty/2"
        );

        let book_html = r#"
            <ul>
                <li><a href="/es/wol/b/r4/lp-s/nwtsty/1/1">1</a></li>
                <li><a href="/es/wol/b/r4/lp-s/nwtsty/1/2#v=1:1">2</a></li>
                <li><a href="/es/wol/b/r4/lp-s/nwtsty/1/1">1 dup</a></li>
            </ul>
        "#;
        let chapters = extract_bible_chapter_urls(book_html, BASE_HOST);
        assert_eq!(chapters.len(), 2);
        assert_eq!(
            chapters[0].as_str(),
            "https://wol.example.org/es/wol/b/r4/lp-s/nwtsty/1/1"
        );
        assert_eq!(
            chapters[1].as_str(),
            "https://wol.example.org/es/wol/b/r4/lp-s/nwtsty/1/2"
        );
    }

    #[test]
    fn extracts_lang_from_canonical_url() {
        assert_eq!(
            extract_lang_from_canonical(&CanonicalUrl::new(
                "https://wol.example.org/en/wol/b/r1/lp-e/nwtsty/1/1".to_string()
            )),
            "en"
        );
        assert_eq!(
            extract_lang_from_canonical(&CanonicalUrl::new(
                "https://wol.example.org/de/wol/binav/r10/lp-x".to_string()
            )),
            "de"
        );
    }

    #[test]
    fn extracts_library_children_and_ignores_breadcrumbs() {
        let parent = CanonicalUrl::new(
            "https://wol.example.org/es/wol/library/r4/lp-s/biblioteca/perspicacia".to_string(),
        );
        let html = r#"
            <nav>
                <a href="/es/wol/library/r4/lp-s/biblioteca">Biblioteca</a>
                <a href="/es/wol/library/r4/lp-s/biblioteca/perspicacia">Perspicacia</a>
            </nav>
            <ul class="directory">
                <li><a href="/es/wol/library/r4/lp-s/biblioteca/perspicacia/a">A</a></li>
                <li><a href="wol://es/wol/library/r4/lp-s/biblioteca/perspicacia/b#top">B</a></li>
                <li><a href="/es/wol/library/r4/lp-s/biblioteca/perspicacia/a/">A dup</a></li>
            </ul>
        "#;

        let children = extract_library_child_urls(html, &parent, BASE_HOST);
        assert_eq!(children.len(), 2);
        assert_eq!(
            children[0].as_str(),
            "https://wol.example.org/es/wol/library/r4/lp-s/biblioteca/perspicacia/a"
        );
        assert_eq!(
            children[1].as_str(),
            "https://wol.example.org/es/wol/library/r4/lp-s/biblioteca/perspicacia/b"
        );
    }

    #[test]
    fn extracts_library_children_handles_percent_encoding() {
        let parent = CanonicalUrl::new(
            "https://wol.example.org/es/wol/library/r4/lp-s/biblioteca/la-atalaya/la-atalaya-2027/edici%C3%B3n-de-estudio"
                .to_string(),
        );
        let html = r#"
            <a href="/es/wol/library/r4/lp-s/biblioteca">Biblioteca</a>
            <a href="/es/wol/library/r4/lp-s/biblioteca/la-atalaya/la-atalaya-2027/edición-de-estudio/enero">Enero</a>
        "#;

        let children = extract_library_child_urls(html, &parent, BASE_HOST);
        assert_eq!(children.len(), 1);
        assert_eq!(
            children[0].as_str(),
            "https://wol.example.org/es/wol/library/r4/lp-s/biblioteca/la-atalaya/la-atalaya-2027/edición-de-estudio/enero"
        );
    }
}
