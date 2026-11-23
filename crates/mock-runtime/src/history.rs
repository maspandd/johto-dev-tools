//! Request history: capture, redaction, retention and filtering (spec §4.6).
//!
//! Records are built on the request path but handed to a background task
//! through a bounded channel with `try_send`, so a slow or full history
//! store never delays a response. When the channel is full the record is
//! dropped and counted.

use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use serde_json::Value;
use tokio::sync::mpsc;

use crate::HistoryConfig;

pub(crate) const REDACTED: &str = "[REDACTED]";

const DEFAULT_REDACTED_HEADERS: &[&str] = &[
    "authorization",
    "proxy-authorization",
    "cookie",
    "set-cookie",
    "x-api-key",
    "x-auth-token",
    "x-csrf-token",
];

const DEFAULT_REDACTED_FIELDS: &[&str] = &[
    "password",
    "passwd",
    "secret",
    "token",
    "access_token",
    "refresh_token",
    "id_token",
    "api_key",
    "apikey",
    "client_secret",
    "private_key",
    "card_number",
    "cvv",
];

pub use workbench_domain::{CapturedBody, HistoryFilter, HistoryRecord};

/// In-memory, retention-bounded history for one server.
/// Cheap to clone; clones share the same records.
#[derive(Debug, Clone)]
pub struct HistoryStore {
    records: Arc<Mutex<VecDeque<HistoryRecord>>>,
    retention: usize,
    dropped: Arc<AtomicU64>,
}

impl HistoryStore {
    #[must_use]
    pub fn new(retention: usize) -> Self {
        Self {
            records: Arc::new(Mutex::new(VecDeque::new())),
            retention: retention.max(1),
            dropped: Arc::new(AtomicU64::new(0)),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, VecDeque<HistoryRecord>> {
        // A panic while holding the lock cannot leave the deque inconsistent.
        self.records.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn push(&self, record: HistoryRecord) {
        let mut records = self.lock();
        while records.len() >= self.retention {
            records.pop_front();
        }
        records.push_back(record);
    }

    /// Matching records, newest first.
    #[must_use]
    pub fn list(&self, filter: &HistoryFilter) -> Vec<HistoryRecord> {
        let records = self.lock();
        records
            .iter()
            .rev()
            .filter(|r| filter.matches(r))
            .take(filter.limit.unwrap_or(usize::MAX))
            .cloned()
            .collect()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.lock().len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn clear(&self) {
        self.lock().clear();
    }

    /// Records discarded because the capture queue was full.
    #[must_use]
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

/// Sends records to the background writer without ever blocking.
#[derive(Debug, Clone)]
pub(crate) struct HistorySink {
    tx: mpsc::Sender<HistoryRecord>,
    store: HistoryStore,
}

impl HistorySink {
    /// Spawns the writer task and returns the sink feeding it.
    pub(crate) fn spawn(store: HistoryStore, capacity: usize) -> Self {
        let (tx, mut rx) = mpsc::channel::<HistoryRecord>(capacity.max(1));
        let writer = store.clone();
        tokio::spawn(async move {
            while let Some(record) = rx.recv().await {
                writer.push(record);
            }
        });
        Self { tx, store }
    }

    pub(crate) fn record(&self, record: HistoryRecord) {
        if self.tx.try_send(record).is_err() {
            self.store.dropped.fetch_add(1, Ordering::Relaxed);
            tracing::debug!("history queue full; record dropped");
        }
    }
}

/// Applies the configured redaction and size limits.
#[derive(Debug, Clone)]
pub(crate) struct Redactor {
    headers: HashSet<String>,
    fields: HashSet<String>,
    capture_bodies: bool,
    max_body: usize,
}

impl Redactor {
    pub(crate) fn new(config: &HistoryConfig) -> Self {
        let lower = |s: &str| s.to_ascii_lowercase();
        Self {
            headers: DEFAULT_REDACTED_HEADERS
                .iter()
                .copied()
                .chain(config.extra_redacted_headers.iter().map(String::as_str))
                .map(lower)
                .collect(),
            fields: DEFAULT_REDACTED_FIELDS
                .iter()
                .copied()
                .chain(config.extra_redacted_fields.iter().map(String::as_str))
                .map(lower)
                .collect(),
            capture_bodies: config.capture_bodies,
            max_body: config.max_captured_body_bytes,
        }
    }

    pub(crate) fn headers(&self, headers: &[(String, String)]) -> Vec<(String, String)> {
        headers
            .iter()
            .map(|(k, v)| {
                let v = if self.headers.contains(&k.to_ascii_lowercase()) {
                    REDACTED.to_owned()
                } else {
                    v.clone()
                };
                (k.clone(), v)
            })
            .collect()
    }

    pub(crate) fn query(&self, query: &[(String, String)]) -> Vec<(String, String)> {
        query
            .iter()
            .map(|(k, v)| {
                let v = if self.fields.contains(&k.to_ascii_lowercase()) {
                    REDACTED.to_owned()
                } else {
                    v.clone()
                };
                (k.clone(), v)
            })
            .collect()
    }

    fn redact_json(&self, value: &mut Value) {
        match value {
            Value::Object(map) => {
                for (k, v) in map.iter_mut() {
                    if self.fields.contains(&k.to_ascii_lowercase()) {
                        *v = Value::String(REDACTED.to_owned());
                    } else {
                        self.redact_json(v);
                    }
                }
            }
            Value::Array(items) => items.iter_mut().for_each(|v| self.redact_json(v)),
            _ => {}
        }
    }

    pub(crate) fn body(&self, body: &[u8]) -> Option<CapturedBody> {
        if !self.capture_bodies || body.is_empty() {
            return None;
        }
        let text = match serde_json::from_slice::<Value>(body) {
            Ok(mut json) => {
                self.redact_json(&mut json);
                json.to_string()
            }
            Err(_) => String::from_utf8_lossy(body).into_owned(),
        };
        let truncated = text.len() > self.max_body;
        let content = if truncated {
            let mut end = self.max_body;
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            text[..end].to_owned()
        } else {
            text
        };
        Some(CapturedBody {
            content,
            size: body.len(),
            truncated,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use serde_json::json;
    use workbench_core::{MockServerId, RequestId};

    fn record(method: &str, status: u16) -> HistoryRecord {
        HistoryRecord {
            id: RequestId::new(),
            server_id: MockServerId::new(),
            timestamp: Utc::now(),
            method: method.into(),
            path: "/".into(),
            query: vec![],
            request_headers: vec![],
            request_body: None,
            matched_endpoint: None,
            matched_rule: None,
            matched_resource: None,
            status,
            duration_ms: 0,
            response_headers: vec![],
            response_body: None,
        }
    }

    #[test]
    fn retention_keeps_newest() {
        let store = HistoryStore::new(2);
        store.push(record("GET", 200));
        store.push(record("POST", 201));
        store.push(record("DELETE", 204));
        let all = store.list(&HistoryFilter::default());
        assert_eq!(all.iter().map(|r| r.status).collect::<Vec<_>>(), [204, 201]);
    }

    #[test]
    fn filters_combine() {
        let store = HistoryStore::new(10);
        store.push(record("GET", 200));
        store.push(record("GET", 404));
        store.push(record("POST", 200));
        let f = HistoryFilter {
            method: Some("get".into()),
            status: Some(200),
            ..HistoryFilter::default()
        };
        assert_eq!(store.list(&f).len(), 1);
        assert_eq!(
            store
                .list(&HistoryFilter {
                    limit: Some(2),
                    ..HistoryFilter::default()
                })
                .len(),
            2
        );
        store.clear();
        assert!(store.is_empty());
    }

    #[test]
    fn redacts_headers_query_and_nested_body_fields() {
        let r = Redactor::new(&HistoryConfig {
            extra_redacted_headers: vec!["X-Session".into()],
            extra_redacted_fields: vec!["ssn".into()],
            ..HistoryConfig::default()
        });
        let h = r.headers(&[
            ("Authorization".into(), "Bearer abc".into()),
            ("x-session".into(), "s".into()),
            ("accept".into(), "*/*".into()),
        ]);
        assert_eq!(h[0].1, REDACTED);
        assert_eq!(h[1].1, REDACTED);
        assert_eq!(h[2].1, "*/*");

        let q = r.query(&[("token".into(), "t".into()), ("page".into(), "1".into())]);
        assert_eq!(q[0].1, REDACTED);
        assert_eq!(q[1].1, "1");

        let body = json!({ "user": { "Password": "x", "ssn": "1", "name": "a" }, "list": [{ "token": "t" }] });
        let captured = r.body(body.to_string().as_bytes()).unwrap();
        let back: Value = serde_json::from_str(&captured.content).unwrap();
        assert_eq!(back["user"]["Password"], REDACTED);
        assert_eq!(back["user"]["ssn"], REDACTED);
        assert_eq!(back["user"]["name"], "a");
        assert_eq!(back["list"][0]["token"], REDACTED);
    }

    #[test]
    fn truncates_on_char_boundary_and_respects_capture_flag() {
        let r = Redactor::new(&HistoryConfig {
            max_captured_body_bytes: 4,
            ..HistoryConfig::default()
        });
        let c = r.body("aéééé".as_bytes()).unwrap();
        assert!(c.truncated);
        assert_eq!(c.content, "aé");
        assert_eq!(c.size, 9);

        let off = Redactor::new(&HistoryConfig {
            capture_bodies: false,
            ..HistoryConfig::default()
        });
        assert!(off.body(b"hello").is_none());
    }
}
