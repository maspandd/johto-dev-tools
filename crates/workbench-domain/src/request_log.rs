//! Captured mock requests (spec §4.6). Produced by the runtime, persisted
//! by storage, shown in the history panel.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use workbench_core::{EndpointId, MockServerId, RequestId, ResourceId};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapturedBody {
    pub content: String,
    /// Original size in bytes.
    pub size: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryRecord {
    pub id: RequestId,
    pub server_id: MockServerId,
    pub timestamp: DateTime<Utc>,
    pub method: String,
    pub path: String,
    pub query: Vec<(String, String)>,
    pub request_headers: Vec<(String, String)>,
    pub request_body: Option<CapturedBody>,
    pub matched_endpoint: Option<EndpointId>,
    pub matched_rule: Option<String>,
    /// The CRUD resource that handled the request, if any.
    #[serde(default)]
    pub matched_resource: Option<ResourceId>,
    pub status: u16,
    /// Total handling time including any simulated delay.
    pub duration_ms: u64,
    pub response_headers: Vec<(String, String)>,
    pub response_body: Option<CapturedBody>,
}

/// Filters for listing history. Every set field must match.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HistoryFilter {
    pub method: Option<String>,
    pub status: Option<u16>,
    pub endpoint: Option<EndpointId>,
    pub resource: Option<ResourceId>,
    pub server: Option<MockServerId>,
    pub since: Option<DateTime<Utc>>,
    pub until: Option<DateTime<Utc>>,
    pub limit: Option<usize>,
}

impl HistoryFilter {
    /// Whether `r` passes every set filter (`limit` is applied by the caller).
    #[must_use]
    pub fn matches(&self, r: &HistoryRecord) -> bool {
        self.method
            .as_ref()
            .is_none_or(|m| m.eq_ignore_ascii_case(&r.method))
            && self.status.is_none_or(|s| s == r.status)
            && self.endpoint.is_none_or(|e| Some(e) == r.matched_endpoint)
            && self.resource.is_none_or(|x| Some(x) == r.matched_resource)
            && self.server.is_none_or(|s| s == r.server_id)
            && self.since.is_none_or(|t| r.timestamp >= t)
            && self.until.is_none_or(|t| r.timestamp <= t)
    }
}
