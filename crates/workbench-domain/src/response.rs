//! Mock responses and the declarative rules that select one (spec §4.2).
//!
//! Rules are data only: no scripts or user code are ever executed (spec §9.1).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A response returned by the mock server.
///
/// String values inside `body` may contain `{{ ... }}` placeholders that the
/// engine fills from the request (see `mock-engine`'s template module).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MockResponse {
    #[serde(default = "default_status")]
    pub status: u16,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub body: ResponseBody,
    /// Artificial latency before the response is sent.
    #[serde(default)]
    pub delay_ms: u64,
}

const fn default_status() -> u16 {
    200
}

impl Default for MockResponse {
    fn default() -> Self {
        Self {
            status: default_status(),
            headers: BTreeMap::new(),
            body: ResponseBody::Empty,
            delay_ms: 0,
        }
    }
}

impl MockResponse {
    #[must_use]
    pub fn json(status: u16, body: Value) -> Self {
        Self {
            status,
            body: ResponseBody::Json(body),
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "content", rename_all = "snake_case")]
pub enum ResponseBody {
    #[default]
    Empty,
    Json(Value),
    Text(String),
}

/// Returns `response` when every condition in `when` holds.
/// Rules are evaluated in order; the first match wins.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponseRule {
    pub name: String,
    pub when: Vec<Condition>,
    pub response: MockResponse,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Condition {
    pub source: ValueSource,
    pub operator: Operator,
    /// Required for `equals`, `not_equals` and `contains`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
}

/// Where a condition reads its value from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "from", rename_all = "snake_case")]
pub enum ValueSource {
    /// Request header (case-insensitive name).
    Header { name: String },
    /// Query-string parameter.
    Query { name: String },
    /// Path parameter from the route template.
    Path { name: String },
    /// JSON pointer into the request body, e.g. `/user/role`.
    Body { pointer: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operator {
    Equals,
    NotEquals,
    Contains,
    Exists,
    Absent,
}

impl Operator {
    #[must_use]
    pub const fn requires_value(self) -> bool {
        matches!(self, Self::Equals | Self::NotEquals | Self::Contains)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn response_defaults() {
        let r: MockResponse = serde_json::from_value(json!({})).unwrap();
        assert_eq!(r, MockResponse::default());
        assert_eq!(r.status, 200);
    }

    #[test]
    fn rule_json_shape() {
        let rule: ResponseRule = serde_json::from_value(json!({
            "name": "admin",
            "when": [
                { "source": { "from": "header", "name": "x-role" }, "operator": "equals", "value": "admin" },
                { "source": { "from": "body", "pointer": "/force" }, "operator": "exists" }
            ],
            "response": { "status": 403, "body": { "type": "json", "content": { "denied": true } } }
        }))
        .unwrap();
        assert_eq!(rule.when.len(), 2);
        assert_eq!(rule.response.status, 403);
        assert_eq!(
            rule.response.body,
            ResponseBody::Json(json!({ "denied": true }))
        );
    }
}
