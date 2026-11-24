//! Runtime configuration with conservative defaults.

use std::collections::BTreeMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use workbench_core::BindAddress;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MockServerConfig {
    #[serde(default)]
    pub bind: BindAddress,
    /// Optional prefix for every route, e.g. `/mock`.
    #[serde(default)]
    pub base_path: Option<String>,
    /// Headers added to every response unless the endpoint sets them.
    #[serde(default)]
    pub default_headers: BTreeMap<String, String>,
    /// Values for `{{ env.NAME }}` placeholders (non-secret variables only).
    #[serde(default)]
    pub variables: BTreeMap<String, String>,
    /// Largest request body accepted, in bytes. Larger bodies get `413`.
    #[serde(default = "default_max_body_bytes")]
    pub max_body_bytes: usize,
    /// Upper bound on any simulated delay, so one endpoint cannot hold a
    /// connection open indefinitely.
    #[serde(default = "default_max_delay_ms")]
    pub max_delay_ms: u64,
    #[serde(default)]
    pub history: HistoryConfig,
    #[serde(default)]
    pub cors: CorsConfig,
}

const fn default_max_body_bytes() -> usize {
    1024 * 1024
}

const fn default_max_delay_ms() -> u64 {
    30_000
}

impl Default for MockServerConfig {
    fn default() -> Self {
        Self {
            bind: BindAddress::default(),
            base_path: None,
            default_headers: BTreeMap::new(),
            variables: BTreeMap::new(),
            max_body_bytes: default_max_body_bytes(),
            max_delay_ms: default_max_delay_ms(),
            history: HistoryConfig::default(),
            cors: CorsConfig::default(),
        }
    }
}

impl MockServerConfig {
    /// Loopback on the given port (`0` picks a free port).
    #[must_use]
    pub fn loopback(port: u16) -> Self {
        Self {
            bind: BindAddress::loopback(port),
            ..Self::default()
        }
    }

    #[must_use]
    pub fn max_delay(&self) -> Duration {
        Duration::from_millis(self.max_delay_ms)
    }
}

/// What request history records and keeps (spec §4.6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryConfig {
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Store request and response bodies.
    #[serde(default = "yes")]
    pub capture_bodies: bool,
    /// Bodies longer than this are truncated in history.
    #[serde(default = "default_captured_body_bytes")]
    pub max_captured_body_bytes: usize,
    /// Newest records kept per server; older ones are discarded.
    #[serde(default = "default_retention")]
    pub retention: usize,
    /// Headers to redact in addition to the built-in list.
    #[serde(default)]
    pub extra_redacted_headers: Vec<String>,
    /// JSON body fields and query parameters to redact in addition to the built-in list.
    #[serde(default)]
    pub extra_redacted_fields: Vec<String>,
}

const fn yes() -> bool {
    true
}

const fn default_captured_body_bytes() -> usize {
    16 * 1024
}

const fn default_retention() -> usize {
    1000
}

impl Default for HistoryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            capture_bodies: true,
            max_captured_body_bytes: default_captured_body_bytes(),
            retention: default_retention(),
            extra_redacted_headers: Vec::new(),
            extra_redacted_fields: Vec::new(),
        }
    }
}

/// Browser access to the mock server (CORS).
///
/// Pages served from the same machine (`localhost`, `127.0.0.1`, `[::1]`
/// or `*.localhost`, any port) may call the mock server by default, so a
/// local front end and the built-in request client work out of the box.
/// Other sites may not unless listed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CorsConfig {
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Extra origins such as `https://app.example.test`. `"*"` allows any.
    #[serde(default)]
    pub allowed_origins: Vec<String>,
}

impl Default for CorsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            allowed_origins: Vec::new(),
        }
    }
}

impl CorsConfig {
    /// Whether a browser page from `origin` may read responses.
    #[must_use]
    pub fn allows(&self, origin: &str) -> bool {
        if !self.enabled {
            return false;
        }
        if self
            .allowed_origins
            .iter()
            .any(|o| o == "*" || o.trim_end_matches('/').eq_ignore_ascii_case(origin))
        {
            return true;
        }
        let Some(rest) = origin
            .strip_prefix("http://")
            .or_else(|| origin.strip_prefix("https://"))
        else {
            return false;
        };
        let host = if let Some(v6) = rest.strip_prefix('[') {
            v6.split(']').next().map(|h| format!("[{h}]"))
        } else {
            rest.split(':').next().map(str::to_owned)
        };
        host.is_some_and(|h| {
            let h = h.to_ascii_lowercase();
            h == "localhost" || h == "127.0.0.1" || h == "[::1]" || h.ends_with(".localhost")
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_origins_are_allowed_by_default() {
        let cors = CorsConfig::default();
        for ok in [
            "http://localhost:5173",
            "http://127.0.0.1:3000",
            "http://[::1]:8080",
            "https://app.localhost",
            "http://LOCALHOST",
        ] {
            assert!(cors.allows(ok), "{ok}");
        }
        for no in [
            "https://evil.test",
            "http://localhost.evil.test",
            "http://127.0.0.1.evil.test:80",
            "null",
            "file://",
        ] {
            assert!(!cors.allows(no), "{no}");
        }
    }

    #[test]
    fn listed_origins_and_switch() {
        let mut cors = CorsConfig {
            enabled: true,
            allowed_origins: vec!["https://app.example.test/".into()],
        };
        assert!(cors.allows("https://app.example.test"));
        assert!(!cors.allows("https://other.example.test"));
        cors.allowed_origins.push("*".into());
        assert!(cors.allows("https://other.example.test"));
        cors.enabled = false;
        assert!(!cors.allows("http://localhost:5173"));
    }
}
