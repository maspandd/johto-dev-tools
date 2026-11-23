//! Runtime configuration with conservative defaults.

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
    /// Largest request body accepted, in bytes. Larger bodies get `413`.
    #[serde(default = "default_max_body_bytes")]
    pub max_body_bytes: usize,
    /// Upper bound on any simulated delay, so one endpoint cannot hold a
    /// connection open indefinitely.
    #[serde(default = "default_max_delay_ms")]
    pub max_delay_ms: u64,
    #[serde(default)]
    pub history: HistoryConfig,
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
            max_body_bytes: default_max_body_bytes(),
            max_delay_ms: default_max_delay_ms(),
            history: HistoryConfig::default(),
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
