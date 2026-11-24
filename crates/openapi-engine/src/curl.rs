//! cURL command import (spec §4.8).
//!
//! The command is parsed, never executed: no network access, and `@file`
//! arguments are not read. Credentials (authorization headers, `-u`,
//! cookies, token-like query parameters and body fields) are replaced with
//! `[REDACTED]` unless the caller explicitly keeps them.

use serde::Serialize;
use serde_json::Value;
use thiserror::Error;
use workbench_domain::{AuthSimulation, Endpoint, Method, ParamSpec, PathTemplate};

/// Longest command accepted.
pub const MAX_COMMAND_BYTES: usize = 1024 * 1024;
pub const REDACTED: &str = "[REDACTED]";

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CurlError {
    #[error("command is longer than {MAX_COMMAND_BYTES} bytes")]
    TooLong,
    #[error("not a curl command (it must start with `curl`)")]
    NotCurl,
    #[error("unterminated {0} quote")]
    Unterminated(char),
    #[error("option `{0}` needs a value")]
    MissingValue(String),
    #[error("no URL in the command")]
    NoUrl,
    #[error("invalid URL `{0}`")]
    InvalidUrl(String),
    #[error("method `{0}` is not supported; use GET, POST, PUT, PATCH or DELETE")]
    UnsupportedMethod(String),
}

#[derive(Debug, Clone, Copy, Default)]
pub struct CurlOptions {
    /// Keep credential values instead of redacting them.
    pub keep_credentials: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NameValue {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", content = "content", rename_all = "snake_case")]
pub enum CurlBody {
    Json(Value),
    Text(String),
}

/// What the command would send.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CurlRequest {
    pub method: Method,
    /// Without user credentials; redacted query values stay redacted.
    pub url: String,
    pub scheme: String,
    pub host: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    pub path: String,
    pub query: Vec<NameValue>,
    pub headers: Vec<NameValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<CurlBody>,
    /// What was redacted, e.g. `header Authorization`.
    pub redacted: Vec<String>,
    /// Options that were ignored or only partly understood.
    pub warnings: Vec<String>,
}

const SENSITIVE_HEADERS: &[&str] = &[
    "authorization",
    "proxy-authorization",
    "cookie",
    "x-api-key",
    "api-key",
    "apikey",
    "x-auth-token",
    "x-csrf-token",
    "x-access-token",
];
const SENSITIVE_WORDS: &[&str] = &[
    "password",
    "passwd",
    "secret",
    "token",
    "api_key",
    "apikey",
    "api-key",
    "private_key",
    "card_number",
    "cvv",
    "signature",
    "sig",
    "auth",
    "key",
    "credential",
    "credentials",
];
/// Headers that describe the client rather than the API.
const CLIENT_HEADERS: &[&str] = &[
    "accept",
    "accept-encoding",
    "accept-language",
    "authorization",
    "cache-control",
    "connection",
    "content-length",
    "content-type",
    "cookie",
    "host",
    "origin",
    "pragma",
    "referer",
    "user-agent",
];
const API_KEY_HEADERS: &[&str] = &["x-api-key", "api-key", "apikey", "x-auth-token"];

/// Options that take no value.
const FLAGS: &[&str] = &[
    "-s",
    "--silent",
    "-S",
    "--show-error",
    "-L",
    "--location",
    "-k",
    "--insecure",
    "-v",
    "--verbose",
    "-i",
    "--include",
    "-f",
    "--fail",
    "--fail-with-body",
    "--compressed",
    "-N",
    "--no-buffer",
    "--http1.0",
    "--http1.1",
    "--http2",
    "--http2-prior-knowledge",
    "--http3",
    "-g",
    "--globoff",
    "-#",
    "--progress-bar",
    "-4",
    "--ipv4",
    "-6",
    "--ipv6",
    "--path-as-is",
    "-j",
    "--junk-session-cookies",
    "--location-trusted",
    "-Z",
    "--parallel",
    "-n",
    "--netrc",
    "-O",
    "--remote-name",
    "-J",
    "--remote-header-name",
    "-q",
    "--disable",
    "--raw",
];
/// Options that take a value that does not affect the request.
const IGNORED_WITH_VALUE: &[&str] = &[
    "-o",
    "--output",
    "-m",
    "--max-time",
    "--connect-timeout",
    "--retry",
    "--retry-delay",
    "--retry-max-time",
    "-w",
    "--write-out",
    "--resolve",
    "--cacert",
    "--capath",
    "--limit-rate",
    "--max-redirs",
    "-c",
    "--cookie-jar",
    "--max-filesize",
    "-y",
    "--speed-time",
    "-Y",
    "--speed-limit",
    "--interface",
    "--dns-servers",
    "--connect-to",
    "--trace",
    "--trace-ascii",
    "--stderr",
    "--proto",
    "--proto-redir",
    "--expect100-timeout",
];
/// Short options that take a value (for `-XPOST` and `-sXPOST`).
const SHORT_WITH_VALUE: &str = "XHdubAeoFmwxTcrEKyY";

/// Splits a shell command line into words: single and double quotes,
/// `$'...'`, backslash escapes, and line continuations from bash (`\`),
/// cmd (`^`) and PowerShell (`` ` ``).
fn tokenize(input: &str) -> Result<Vec<String>, CurlError> {
    let mut text = input.replace("\r\n", "\n");
    for cont in ["\\\n", "^\n", "`\n"] {
        text = text.replace(cont, " ");
    }
    let mut words = Vec::new();
    let mut current = String::new();
    let mut in_word = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut current));
                    in_word = false;
                }
            }
            '\'' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(c) => current.push(c),
                        None => return Err(CurlError::Unterminated('\'')),
                    }
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(c @ ('"' | '\\' | '$' | '`')) => current.push(c),
                            Some('\n') => {}
                            Some(c) => {
                                current.push('\\');
                                current.push(c);
                            }
                            None => return Err(CurlError::Unterminated('"')),
                        },
                        Some(c) => current.push(c),
                        None => return Err(CurlError::Unterminated('"')),
                    }
                }
            }
            '$' if chars.peek() == Some(&'\'') => {
                chars.next();
                in_word = true;
                ansi_c(&mut chars, &mut current)?;
            }
            '\\' => {
                in_word = true;
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            c => {
                in_word = true;
                current.push(c);
            }
        }
    }
    if in_word {
        words.push(current);
    }
    Ok(words)
}

fn ansi_c(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    out: &mut String,
) -> Result<(), CurlError> {
    loop {
        match chars.next() {
            Some('\'') => return Ok(()),
            Some('\\') => {
                let c = chars.next().ok_or(CurlError::Unterminated('\''))?;
                match c {
                    'n' => out.push('\n'),
                    't' => out.push('\t'),
                    'r' => out.push('\r'),
                    '0' => out.push('\0'),
                    'x' | 'u' | 'U' => {
                        let max = match c {
                            'x' => 2,
                            'u' => 4,
                            _ => 8,
                        };
                        let mut hex = String::new();
                        while hex.len() < max && chars.peek().is_some_and(char::is_ascii_hexdigit) {
                            hex.extend(chars.next());
                        }
                        match u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                            Some(ch) => out.push(ch),
                            None => {
                                out.push('\\');
                                out.push(c);
                                out.push_str(&hex);
                            }
                        }
                    }
                    other => out.push(other),
                }
            }
            Some(c) => out.push(c),
            None => return Err(CurlError::Unterminated('\'')),
        }
    }
}

fn is_sensitive_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    SENSITIVE_HEADERS.contains(&lower.as_str())
        || SENSITIVE_WORDS.contains(&lower.as_str())
        || lower
            .split(['-', '_', '.'])
            .any(|part| SENSITIVE_WORDS.contains(&part))
        || ["token", "secret", "password", "apikey"]
            .iter()
            .any(|w| lower.contains(w))
}

fn redact_json(value: &mut Value, path: &str, redacted: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (k, v) in map.iter_mut() {
                let here = if path.is_empty() {
                    k.clone()
                } else {
                    format!("{path}.{k}")
                };
                if is_sensitive_name(k) && !v.is_object() && !v.is_array() && !v.is_null() {
                    *v = Value::String(REDACTED.to_owned());
                    redacted.push(format!("body field {here}"));
                } else {
                    redact_json(v, &here, redacted);
                }
            }
        }
        Value::Array(items) => {
            for (i, v) in items.iter_mut().enumerate() {
                redact_json(v, &format!("{path}[{i}]"), redacted);
            }
        }
        _ => {}
    }
}

fn percent_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn base64(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in input.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(TABLE[((n >> (18 - 6 * i)) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[derive(Default)]
struct Raw {
    method: Option<String>,
    url: Option<String>,
    headers: Vec<(String, String)>,
    data: Vec<String>,
    json: bool,
    get: bool,
    user: Option<String>,
    cookie: Option<String>,
    warnings: Vec<String>,
}

/// Parses a cURL command. Nothing is executed or read from disk.
pub fn parse(command: &str, options: CurlOptions) -> Result<CurlRequest, CurlError> {
    if command.len() > MAX_COMMAND_BYTES {
        return Err(CurlError::TooLong);
    }
    let words = tokenize(command.trim())?;
    let mut it = words.into_iter();
    match it.next() {
        Some(w) if w.eq_ignore_ascii_case("curl") || w.eq_ignore_ascii_case("curl.exe") => {}
        _ => return Err(CurlError::NotCurl),
    }

    let mut raw = Raw::default();
    let mut rest: Vec<String> = it.collect();
    rest.reverse();
    while let Some(word) = rest.pop() {
        let (opt, inline) = match word.split_once('=') {
            Some((o, v)) if o.starts_with("--") => (o.to_owned(), Some(v.to_owned())),
            _ => (word.clone(), None),
        };
        // Combined short options such as `-sSL` or `-XPOST`.
        if !opt.starts_with("--") && opt.starts_with('-') && opt.len() > 2 {
            let mut expanded = Vec::new();
            let body: Vec<char> = opt[1..].chars().collect();
            let mut i = 0;
            while i < body.len() {
                let c = body[i];
                if SHORT_WITH_VALUE.contains(c) {
                    expanded.push(format!("-{c}"));
                    let tail: String = body[i + 1..].iter().collect();
                    if !tail.is_empty() {
                        expanded.push(tail);
                    }
                    break;
                }
                expanded.push(format!("-{c}"));
                i += 1;
            }
            for w in expanded.into_iter().rev() {
                rest.push(w);
            }
            continue;
        }
        let mut value = |name: &str| -> Result<String, CurlError> {
            inline
                .clone()
                .or_else(|| rest.pop())
                .ok_or_else(|| CurlError::MissingValue(name.to_owned()))
        };
        match opt.as_str() {
            "-X" | "--request" => raw.method = Some(value(&opt)?),
            "-H" | "--header" => {
                let h = value(&opt)?;
                if let Some(file) = h.strip_prefix('@') {
                    raw.warnings
                        .push(format!("headers from file `{file}` were not read"));
                } else if let Some((name, v)) = h.split_once(':') {
                    let v = v.trim();
                    if !v.is_empty() {
                        raw.headers.push((name.trim().to_owned(), v.to_owned()));
                    }
                } else if let Some(name) = h.strip_suffix(';') {
                    raw.headers.push((name.trim().to_owned(), String::new()));
                }
            }
            "-d" | "--data" | "--data-ascii" | "--data-binary" => {
                let d = value(&opt)?;
                if let Some(file) = d.strip_prefix('@') {
                    raw.warnings.push(format!(
                        "body file `{file}` was not read; add the body by hand"
                    ));
                } else {
                    raw.data.push(d);
                }
            }
            "--data-raw" => raw.data.push(value(&opt)?),
            "--data-urlencode" => {
                let d = value(&opt)?;
                let encoded = match d.split_once('=') {
                    Some((name, content)) if !name.contains('@') => {
                        format!("{name}={}", percent_encode(content))
                    }
                    Some(_) => {
                        raw.warnings
                            .push("`--data-urlencode` files were not read".into());
                        continue;
                    }
                    None if d.starts_with('@') => {
                        raw.warnings
                            .push("`--data-urlencode` files were not read".into());
                        continue;
                    }
                    None => percent_encode(&d),
                };
                raw.data.push(encoded);
            }
            "--json" => {
                let d = value(&opt)?;
                if let Some(file) = d.strip_prefix('@') {
                    raw.warnings.push(format!(
                        "body file `{file}` was not read; add the body by hand"
                    ));
                } else {
                    raw.data.push(d);
                }
                raw.json = true;
            }
            "-F" | "--form" | "--form-string" => {
                value(&opt)?;
                raw.warnings
                    .push("multipart form fields (`-F`) are not imported".into());
            }
            "-u" | "--user" => raw.user = Some(value(&opt)?),
            "-b" | "--cookie" => {
                let c = value(&opt)?;
                if c.contains('=') {
                    raw.cookie = Some(c);
                } else {
                    raw.warnings.push(format!("cookie file `{c}` was not read"));
                }
            }
            "-A" | "--user-agent" => raw.headers.push(("User-Agent".into(), value(&opt)?)),
            "-e" | "--referer" => raw.headers.push(("Referer".into(), value(&opt)?)),
            "--oauth2-bearer" => raw
                .headers
                .push(("Authorization".into(), format!("Bearer {}", value(&opt)?))),
            "-G" | "--get" => raw.get = true,
            "-I" | "--head" => raw.method = Some("HEAD".into()),
            "--url" => {
                let u = value(&opt)?;
                set_url(&mut raw, u);
            }
            "-x" | "--proxy" | "-E" | "--cert" | "--key" | "-K" | "--config" | "-T"
            | "--upload-file" => {
                value(&opt)?;
                raw.warnings.push(format!("`{opt}` was ignored"));
            }
            o if FLAGS.contains(&o) => {}
            o if IGNORED_WITH_VALUE.contains(&o) => {
                value(&opt)?;
            }
            o if o.starts_with('-') && o.len() > 1 => {
                raw.warnings
                    .push(format!("unknown option `{o}` was ignored"));
            }
            _ => set_url(&mut raw, word),
        }
    }
    build(raw, options)
}

fn set_url(raw: &mut Raw, url: String) {
    if raw.url.is_some() {
        raw.warnings.push(format!(
            "only the first URL is imported; `{url}` was ignored"
        ));
    } else {
        raw.url = Some(url);
    }
}

fn build(mut raw: Raw, options: CurlOptions) -> Result<CurlRequest, CurlError> {
    let keep = options.keep_credentials;
    let mut redacted = Vec::new();
    let given = raw.url.take().ok_or(CurlError::NoUrl)?;
    let mut url = url::Url::parse(&given)
        .or_else(|_| url::Url::parse(&format!("http://{given}")))
        .map_err(|_| CurlError::InvalidUrl(given.clone()))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(CurlError::InvalidUrl(given));
    }
    if !url.username().is_empty() || url.password().is_some() {
        let user = format!("{}:{}", url.username(), url.password().unwrap_or_default());
        raw.user.get_or_insert(user);
        let _ = url.set_username("");
        let _ = url.set_password(None);
    }

    let mut headers: Vec<NameValue> = Vec::new();
    if let Some(user) = raw.user.take() {
        let value = if keep {
            format!("Basic {}", base64(user.as_bytes()))
        } else {
            redacted.push("basic credentials (-u)".to_owned());
            format!("Basic {REDACTED}")
        };
        headers.push(NameValue {
            name: "Authorization".into(),
            value,
        });
    }
    if let Some(cookie) = raw.cookie.take() {
        raw.headers.push(("Cookie".into(), cookie));
    }
    if raw.json {
        for (name, value) in [
            ("Content-Type", "application/json"),
            ("Accept", "application/json"),
        ] {
            if !raw
                .headers
                .iter()
                .any(|(n, _)| n.eq_ignore_ascii_case(name))
            {
                raw.headers.push((name.into(), value.into()));
            }
        }
    }
    for (name, value) in raw.headers {
        let value = if !keep && is_sensitive_name(&name) && !value.is_empty() {
            redacted.push(format!("header {name}"));
            match value.split_once(' ') {
                Some((scheme, _)) if name.eq_ignore_ascii_case("authorization") => {
                    format!("{scheme} {REDACTED}")
                }
                _ => REDACTED.to_owned(),
            }
        } else {
            value
        };
        headers.push(NameValue { name, value });
    }

    let data = (!raw.data.is_empty()).then(|| raw.data.join("&"));
    let mut query: Vec<NameValue> = url
        .query_pairs()
        .map(|(n, v)| NameValue {
            name: n.into_owned(),
            value: v.into_owned(),
        })
        .collect();
    let mut body = None;
    if let Some(data) = data {
        if raw.get {
            query.extend(
                url::form_urlencoded::parse(data.as_bytes()).map(|(n, v)| NameValue {
                    name: n.into_owned(),
                    value: v.into_owned(),
                }),
            );
        } else {
            body = Some(match serde_json::from_str::<Value>(&data) {
                Ok(mut json) if json.is_object() || json.is_array() => {
                    if !keep {
                        redact_json(&mut json, "", &mut redacted);
                    }
                    CurlBody::Json(json)
                }
                _ => CurlBody::Text(if keep {
                    data
                } else {
                    redact_form(&data, &mut redacted)
                }),
            });
        }
    }
    if !keep {
        for q in &mut query {
            if is_sensitive_name(&q.name) && !q.value.is_empty() {
                q.value = REDACTED.to_owned();
                redacted.push(format!("query parameter {}", q.name));
            }
        }
    }
    url.set_fragment(None);
    url.set_query(None);
    if !query.is_empty() {
        url.query_pairs_mut()
            .extend_pairs(query.iter().map(|q| (&q.name, &q.value)));
    }

    let method_name = raw
        .method
        .unwrap_or_else(|| if body.is_some() { "POST" } else { "GET" }.to_owned());
    let method = method_name
        .parse::<Method>()
        .map_err(|_| CurlError::UnsupportedMethod(method_name.to_ascii_uppercase()))?;

    Ok(CurlRequest {
        method,
        scheme: url.scheme().to_owned(),
        host: url.host_str().unwrap_or_default().to_owned(),
        port: url.port(),
        path: url.path().to_owned(),
        url: url.into(),
        query,
        headers,
        body,
        redacted,
        warnings: raw.warnings,
    })
}

/// Redacts sensitive `name=value` pairs in a form-encoded body.
fn redact_form(data: &str, redacted: &mut Vec<String>) -> String {
    if !data.contains('=') {
        return data.to_owned();
    }
    data.split('&')
        .map(|pair| match pair.split_once('=') {
            Some((name, value)) if is_sensitive_name(name) && !value.is_empty() => {
                redacted.push(format!("body field {name}"));
                format!("{name}={REDACTED}")
            }
            _ => pair.to_owned(),
        })
        .collect::<Vec<_>>()
        .join("&")
}

fn looks_like_id(segment: &str) -> bool {
    let is_number = !segment.is_empty() && segment.bytes().all(|b| b.is_ascii_digit());
    let is_uuid = segment.len() == 36
        && segment.chars().enumerate().all(|(i, c)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                c == '-'
            } else {
                c.is_ascii_hexdigit()
            }
        });
    is_number || is_uuid
}

impl CurlRequest {
    /// An endpoint draft for this request. `base_path` (the project's) is
    /// removed from the front of the path, and numeric or UUID segments
    /// become path parameters (`/users/42` → `/users/:id`).
    pub fn to_endpoint(&self, base_path: Option<&str>) -> Result<Endpoint, String> {
        let mut path = self.path.as_str();
        if let Some(base) = base_path
            .map(|b| b.trim_end_matches('/'))
            .filter(|b| !b.is_empty())
            && let Some(rest) = path.strip_prefix(base)
            && (rest.is_empty() || rest.starts_with('/'))
        {
            path = if rest.is_empty() { "/" } else { rest };
        }
        let mut template = String::new();
        let mut ids = 0;
        for segment in path.split('/').filter(|s| !s.is_empty()) {
            template.push('/');
            if looks_like_id(segment) {
                ids += 1;
                template.push_str(&if ids == 1 {
                    ":id".to_owned()
                } else {
                    format!(":id{ids}")
                });
            } else {
                template.push_str(segment);
            }
        }
        if template.is_empty() {
            template.push('/');
        }
        let template = PathTemplate::parse(&template).map_err(|e| format!("path `{path}`: {e}"))?;

        let mut ep = Endpoint::new(format!("{} {template}", self.method), self.method, template);
        ep.request.query = self
            .query
            .iter()
            .map(|q| ParamSpec {
                name: q.name.clone(),
                required: false,
                description: String::new(),
            })
            .collect();
        for h in &self.headers {
            let lower = h.name.to_ascii_lowercase();
            if lower == "authorization" && h.value.to_ascii_lowercase().starts_with("bearer") {
                ep.auth = AuthSimulation::Bearer;
            } else if API_KEY_HEADERS.contains(&lower.as_str()) {
                ep.auth = AuthSimulation::ApiKey {
                    header: h.name.clone(),
                };
            } else if !CLIENT_HEADERS.contains(&lower.as_str()) && !lower.starts_with("sec-") {
                ep.request.headers.push(ParamSpec {
                    name: h.name.clone(),
                    required: false,
                    description: String::new(),
                });
            }
        }
        if let Some(CurlBody::Json(body)) = &self.body {
            ep.request.body_example = Some(body.clone());
        }
        ep.validate().map_err(|errors| {
            errors
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("; ")
        })?;
        Ok(ep)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn p(cmd: &str) -> CurlRequest {
        parse(cmd, CurlOptions::default()).unwrap()
    }

    #[test]
    fn parses_a_typical_post() {
        let r = p(
            r#"curl -X POST 'https://api.example.test/v1/users?debug=1' \
              -H 'Content-Type: application/json' \
              -H "Authorization: Bearer abc.def" \
              --data-raw '{"name":"Sari","password":"hunter2","profile":{"api_token":"t"}}'"#,
        );
        assert_eq!(r.method, Method::Post);
        assert_eq!(r.host, "api.example.test");
        assert_eq!(r.path, "/v1/users");
        assert_eq!(
            r.query,
            vec![NameValue {
                name: "debug".into(),
                value: "1".into()
            }]
        );
        assert_eq!(r.headers[1].value, "Bearer [REDACTED]");
        assert_eq!(
            r.body,
            Some(CurlBody::Json(json!({
                "name": "Sari", "password": REDACTED, "profile": { "api_token": REDACTED }
            })))
        );
        assert_eq!(r.redacted.len(), 3, "{:?}", r.redacted);
        assert!(!format!("{r:?}").contains("hunter2"));
        assert!(!format!("{r:?}").contains("abc.def"));
    }

    #[test]
    fn keeps_credentials_only_when_asked() {
        let r = parse(
            "curl -u alex:pw https://x.test/a -H 'X-Api-Key: k1'",
            CurlOptions {
                keep_credentials: true,
            },
        )
        .unwrap();
        assert_eq!(r.headers[0].value, format!("Basic {}", base64(b"alex:pw")));
        assert_eq!(r.headers[1].value, "k1");
        assert!(r.redacted.is_empty());

        let r = p("curl https://alex:pw@x.test/a?access_token=zzz -b 'session=s1'");
        assert_eq!(r.url, "https://x.test/a?access_token=%5BREDACTED%5D");
        assert_eq!(r.headers[0].value, "Basic [REDACTED]");
        assert_eq!(
            r.headers[1],
            NameValue {
                name: "Cookie".into(),
                value: REDACTED.into()
            }
        );
    }

    #[test]
    fn handles_shell_syntax_variants() {
        let r = p("curl -sSL -XPUT \"http://h.test/a\" -d $'{\"a\":\"it\\'s\"}'");
        assert_eq!(r.method, Method::Put);
        assert_eq!(r.body, Some(CurlBody::Json(json!({ "a": "it's" }))));

        // cmd and PowerShell continuations
        let r = p("curl ^\n  --request=PATCH ^\n  h.test/items/7");
        assert_eq!(r.method, Method::Patch);
        assert_eq!(r.url, "http://h.test/items/7");
        let r = p("curl.exe `\n  --json '{\"a\":1}' `\n  http://h.test/x");
        assert_eq!(r.method, Method::Post);
        assert!(r.headers.iter().any(|h| h.value == "application/json"));

        assert_eq!(
            parse("curl 'oops", CurlOptions::default()),
            Err(CurlError::Unterminated('\''))
        );
        assert_eq!(
            parse("wget x", CurlOptions::default()),
            Err(CurlError::NotCurl)
        );
        assert_eq!(
            parse("curl -s", CurlOptions::default()),
            Err(CurlError::NoUrl)
        );
        assert_eq!(
            parse("curl -I http://h.test", CurlOptions::default()),
            Err(CurlError::UnsupportedMethod("HEAD".into()))
        );
    }

    #[test]
    fn get_flag_moves_data_to_query_and_files_are_not_read() {
        let r = p("curl -G http://h.test/s -d q=rust -d page=2 --data-urlencode 'tag=a b'");
        assert_eq!(r.method, Method::Get);
        assert_eq!(r.body, None);
        let names: Vec<_> = r
            .query
            .iter()
            .map(|q| (q.name.as_str(), q.value.as_str()))
            .collect();
        assert_eq!(names, [("q", "rust"), ("page", "2"), ("tag", "a b")]);

        let r = p("curl -d @/etc/passwd http://h.test/x -F f=@a.png --frobnicate");
        assert_eq!(r.body, None);
        assert_eq!(r.method, Method::Get);
        assert_eq!(r.warnings.len(), 3, "{:?}", r.warnings);
    }

    #[test]
    fn form_bodies_are_redacted() {
        let r = p("curl http://h.test/login -d 'user=alex&password=pw'");
        assert_eq!(
            r.body,
            Some(CurlBody::Text("user=alex&password=[REDACTED]".into()))
        );
    }

    #[test]
    fn endpoint_draft() {
        let r = p(
            r#"curl -X PATCH "https://h.test/api/users/42/posts/0b6f3a52-6bd7-4d2e-9b1c-1f9e5c7a1b2c?fields=a" -H 'X-Tenant: t1' -H 'Authorization: Bearer x' -H 'Accept: */*' -d '{"title":"t"}'"#,
        );
        let ep = r.to_endpoint(Some("/api")).unwrap();
        assert_eq!(ep.path.to_string(), "/users/:id/posts/:id2");
        assert_eq!(ep.method, Method::Patch);
        assert_eq!(ep.auth, AuthSimulation::Bearer);
        assert_eq!(ep.request.headers.len(), 1);
        assert_eq!(ep.request.headers[0].name, "X-Tenant");
        assert_eq!(ep.request.query[0].name, "fields");
        assert_eq!(ep.request.body_example, Some(json!({ "title": "t" })));

        let ep = p("curl http://h.test/ -H 'x-api-key: k'")
            .to_endpoint(None)
            .unwrap();
        assert_eq!(ep.path.to_string(), "/");
        assert_eq!(
            ep.auth,
            AuthSimulation::ApiKey {
                header: "x-api-key".into()
            }
        );
    }

    #[test]
    fn base64_matches_rfc4648() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }
}
