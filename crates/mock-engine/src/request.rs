//! The engine's view of an incoming HTTP request, independent of any HTTP library.

use percent_encoding::percent_decode_str;
use workbench_core::RequestId;

#[derive(Debug, Clone)]
pub struct MockRequest {
    pub request_id: RequestId,
    /// Method exactly as received, e.g. `GET`.
    pub method: String,
    /// Raw (still percent-encoded) path without the query string.
    pub path: String,
    /// Decoded query pairs in arrival order.
    pub query: Vec<(String, String)>,
    /// Header pairs; names are compared case-insensitively.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl MockRequest {
    /// Builds a request from a method and a `path?query` string.
    #[must_use]
    pub fn new(method: impl Into<String>, path_and_query: &str) -> Self {
        let (path, query) = match path_and_query.split_once('?') {
            Some((p, q)) => (p, parse_query(q)),
            None => (path_and_query, Vec::new()),
        };
        Self {
            request_id: RequestId::new(),
            method: method.into(),
            path: path.to_owned(),
            query,
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    #[must_use]
    pub fn with_body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = body.into();
        self
    }

    /// First value of a header, case-insensitive.
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// First value of a query parameter.
    #[must_use]
    pub fn query_param(&self, name: &str) -> Option<&str> {
        self.query
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

/// Parses `application/x-www-form-urlencoded` query strings (`+` means space).
#[must_use]
pub fn parse_query(raw: &str) -> Vec<(String, String)> {
    let decode = |s: &str| {
        percent_decode_str(&s.replace('+', " "))
            .decode_utf8_lossy()
            .into_owned()
    };
    raw.split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| match pair.split_once('=') {
            Some((k, v)) => (decode(k), decode(v)),
            None => (decode(pair), String::new()),
        })
        .collect()
}

/// Splits a raw path into decoded segments, ignoring a trailing slash.
pub(crate) fn path_segments(path: &str) -> Vec<String> {
    let trimmed = path.strip_prefix('/').unwrap_or(path);
    let trimmed = trimmed.strip_suffix('/').unwrap_or(trimmed);
    if trimmed.is_empty() {
        return Vec::new();
    }
    trimmed
        .split('/')
        .map(|s| percent_decode_str(s).decode_utf8_lossy().into_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_query_pairs() {
        let r = MockRequest::new("GET", "/users?page=2&q=alex+morgan&flag&e=%40x");
        assert_eq!(r.path, "/users");
        assert_eq!(r.query_param("page"), Some("2"));
        assert_eq!(r.query_param("q"), Some("alex morgan"));
        assert_eq!(r.query_param("flag"), Some(""));
        assert_eq!(r.query_param("e"), Some("@x"));
    }

    #[test]
    fn headers_are_case_insensitive() {
        let r = MockRequest::new("GET", "/").with_header("X-Mode", "a");
        assert_eq!(r.header("x-mode"), Some("a"));
    }

    #[test]
    fn splits_and_decodes_segments() {
        assert_eq!(path_segments("/"), Vec::<String>::new());
        assert_eq!(path_segments("/users/"), vec!["users"]);
        assert_eq!(path_segments("/files/a%20b"), vec!["files", "a b"]);
    }
}
