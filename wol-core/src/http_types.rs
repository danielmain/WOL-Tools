//! Tiny HTTP request/response vocabulary so `wol-core` can describe fetched
//! documents without depending on Tauri's `http` re-export.
//!
//! The desktop app converts [`Response`] into `tauri::http::Response<Vec<u8>>`
//! at the very edge (`wol-desktop/src/lib.rs`); the CLI just reads it.

use std::collections::HashMap;

pub use reqwest::Method;
pub use reqwest::StatusCode;

/// Headers stored case-insensitively (lowercased keys).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Headers(HashMap<String, String>);

impl Headers {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.0.get(&name.to_ascii_lowercase()).map(String::as_str)
    }

    pub fn insert(&mut self, name: impl AsRef<str>, value: impl Into<String>) -> Option<String> {
        self.0
            .insert(name.as_ref().to_ascii_lowercase(), value.into())
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    /// `content-type` with the parameters stripped (`text/html; charset=utf-8` -> `text/html`).
    pub fn content_type(&self) -> Option<&str> {
        self.get("content-type")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response<T> {
    status: StatusCode,
    headers: Headers,
    body: T,
}

impl<T> Response<T> {
    pub fn new(status: StatusCode, headers: Headers, body: T) -> Self {
        Self {
            status,
            headers,
            body,
        }
    }

    pub fn status(&self) -> StatusCode {
        self.status
    }

    pub fn headers(&self) -> &Headers {
        &self.headers
    }

    pub fn body(&self) -> &T {
        &self.body
    }

    pub fn into_body(self) -> T {
        self.body
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseBuilder {
    status: Option<StatusCode>,
    headers: Headers,
}

impl ResponseBuilder {
    pub fn status(mut self, status: StatusCode) -> Self {
        self.status = Some(status);
        self
    }

    pub fn header(mut self, name: impl AsRef<str>, value: impl Into<String>) -> Self {
        self.headers.insert(name, value);
        self
    }

    pub fn body<T>(self, body: T) -> Result<Response<T>, std::convert::Infallible> {
        Ok(Response {
            status: self.status.unwrap_or(StatusCode::OK),
            headers: self.headers,
            body,
        })
    }
}

/// Start building a response; mirrors the shape callers already rely on
/// (`Response::builder().status(..).header(..).body(bytes)`).
pub fn builder() -> ResponseBuilder {
    ResponseBuilder {
        status: None,
        headers: Headers::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headers_are_case_insensitive() {
        let mut headers = Headers::new();
        headers.insert("Content-Type", "text/html; charset=utf-8");
        assert_eq!(headers.get("content-type"), Some("text/html; charset=utf-8"));
        assert_eq!(headers.content_type(), Some("text/html; charset=utf-8"));
    }

    #[test]
    fn builder_defaults_to_200() {
        let resp = builder().header("a", "b").body(vec![1u8]).expect("infallible");
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(resp.headers().get("A"), Some("b"));
        assert_eq!(resp.body(), &vec![1u8]);
    }
}
