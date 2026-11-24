//! `{{ ... }}` placeholders in response bodies and headers.
//!
//! Supported expressions:
//!
//! | Expression            | Value                                        |
//! |-----------------------|----------------------------------------------|
//! | `path.<name>`         | path parameter                               |
//! | `query.<name>`        | first query value                            |
//! | `header.<name>`       | first header value (case-insensitive)        |
//! | `body` / `body.a.b`   | request JSON body or a nested field          |
//! | `request.method`      | HTTP method                                  |
//! | `request.path`        | raw request path                             |
//! | `request.id`          | request ID                                   |
//! | `uuid`                | fresh random UUID                            |
//! | `now`                 | current time, RFC 3339                       |
//! | `timestamp`           | current time, Unix milliseconds              |
//! | `env.<name>`          | environment variable (non-secret)            |
//!
//! A JSON string that is exactly one placeholder is replaced by the typed
//! value (so `"{{ body.age }}"` stays a number). Placeholders embedded in a
//! longer string are interpolated as text. Missing values become `null`
//! (whole-value) or an empty string (interpolated). Unknown expressions are
//! left untouched so typos stay visible.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::{Entropy, MockRequest};

pub(crate) struct TemplateContext<'a> {
    pub request: &'a MockRequest,
    pub params: &'a BTreeMap<String, String>,
    pub body: Option<&'a Value>,
    pub entropy: &'a dyn Entropy,
    pub variables: &'a BTreeMap<String, String>,
}

enum Resolved {
    Value(Value),
    Missing,
    Unknown,
}

impl TemplateContext<'_> {
    fn resolve(&self, expr: &str) -> Resolved {
        let string = |v: Option<&str>| match v {
            Some(s) => Resolved::Value(Value::String(s.to_owned())),
            None => Resolved::Missing,
        };
        match expr {
            "uuid" => return Resolved::Value(Value::String(self.entropy.uuid())),
            "now" => return Resolved::Value(Value::String(self.entropy.now().to_rfc3339())),
            "timestamp" => {
                return Resolved::Value(Value::from(self.entropy.now().timestamp_millis()));
            }
            "request.method" => return string(Some(&self.request.method)),
            "request.path" => return string(Some(&self.request.path)),
            "request.id" => {
                return Resolved::Value(Value::String(self.request.request_id.to_string()));
            }
            "body" => {
                return self
                    .body
                    .cloned()
                    .map_or(Resolved::Missing, Resolved::Value);
            }
            _ => {}
        }
        let Some((root, rest)) = expr.split_once('.') else {
            return Resolved::Unknown;
        };
        match root {
            "path" => string(self.params.get(rest).map(String::as_str)),
            "query" => string(self.request.query_param(rest)),
            "header" | "headers" => string(self.request.header(rest)),
            "env" => string(self.variables.get(rest).map(String::as_str)),
            "body" => {
                let pointer = format!("/{}", rest.replace('.', "/"));
                self.body
                    .and_then(|b| b.pointer(&pointer))
                    .cloned()
                    .map_or(Resolved::Missing, Resolved::Value)
            }
            _ => Resolved::Unknown,
        }
    }

    /// Interpolates every placeholder in `input` as text.
    pub(crate) fn render_str(&self, input: &str) -> String {
        let mut out = String::with_capacity(input.len());
        let mut rest = input;
        while let Some(start) = rest.find("{{") {
            let Some(len) = rest[start + 2..].find("}}") else {
                break;
            };
            let end = start + 2 + len;
            out.push_str(&rest[..start]);
            match self.resolve(rest[start + 2..end].trim()) {
                Resolved::Value(Value::String(s)) => out.push_str(&s),
                Resolved::Value(v) => out.push_str(&v.to_string()),
                Resolved::Missing => {}
                Resolved::Unknown => out.push_str(&rest[start..end + 2]),
            }
            rest = &rest[end + 2..];
        }
        out.push_str(rest);
        out
    }

    /// Renders a JSON value recursively.
    pub(crate) fn render_json(&self, value: &Value) -> Value {
        match value {
            Value::String(s) => {
                if let Some(expr) = whole_placeholder(s) {
                    match self.resolve(expr) {
                        Resolved::Value(v) => return v,
                        Resolved::Missing => return Value::Null,
                        Resolved::Unknown => return value.clone(),
                    }
                }
                Value::String(self.render_str(s))
            }
            Value::Array(items) => {
                Value::Array(items.iter().map(|v| self.render_json(v)).collect())
            }
            Value::Object(map) => Value::Object(
                map.iter()
                    .map(|(k, v)| (k.clone(), self.render_json(v)))
                    .collect(),
            ),
            other => other.clone(),
        }
    }
}

/// Returns the expression if `s` is exactly `{{ expr }}`.
fn whole_placeholder(s: &str) -> Option<&str> {
    let inner = s.trim().strip_prefix("{{")?.strip_suffix("}}")?;
    (!inner.contains("{{") && !inner.contains("}}")).then(|| inner.trim())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, TimeZone, Utc};
    use serde_json::json;

    struct Fixed;
    impl Entropy for Fixed {
        fn uuid(&self) -> String {
            "00000000-0000-4000-8000-000000000000".into()
        }
        fn now(&self) -> DateTime<Utc> {
            Utc.with_ymd_and_hms(2026, 1, 2, 3, 4, 5).unwrap()
        }
    }

    fn render(value: Value, body: Option<Value>) -> Value {
        let req = MockRequest::new("POST", "/users/42?page=3").with_header("X-Tenant", "acme");
        let params = BTreeMap::from([("id".to_owned(), "42".to_owned())]);
        let ctx = TemplateContext {
            request: &req,
            params: &params,
            body: body.as_ref(),
            entropy: &Fixed,
            variables: &BTreeMap::from([("REGION".to_owned(), "id-jkt".to_owned())]),
        };
        ctx.render_json(&value)
    }

    #[test]
    fn whole_value_placeholders_keep_types() {
        let out = render(
            json!({ "id": "{{ path.id }}", "age": "{{body.age}}", "tags": "{{ body.tags }}", "missing": "{{ body.nope }}" }),
            Some(json!({ "age": 30, "tags": ["a"] })),
        );
        assert_eq!(
            out,
            json!({ "id": "42", "age": 30, "tags": ["a"], "missing": null })
        );
    }

    #[test]
    fn interpolates_inside_strings() {
        let out = render(
            json!(
                "user {{path.id}} page {{ query.page }} tenant {{header.x-tenant}} age {{body.age}} {{query.none}}!"
            ),
            Some(json!({ "age": 30 })),
        );
        assert_eq!(out, json!("user 42 page 3 tenant acme age 30 !"));
    }

    #[test]
    fn builtins() {
        let out = render(
            json!({ "id": "{{uuid}}", "at": "{{ now }}", "ms": "{{timestamp}}", "m": "{{request.method}}", "region": "{{ env.REGION }}", "none": "{{ env.MISSING }}" }),
            None,
        );
        assert_eq!(out["region"], "id-jkt");
        assert_eq!(out["none"], Value::Null);
        assert_eq!(out["id"], "00000000-0000-4000-8000-000000000000");
        assert_eq!(out["at"], "2026-01-02T03:04:05+00:00");
        assert_eq!(out["ms"], json!(1_767_323_045_000_i64));
        assert_eq!(out["m"], "POST");
    }

    #[test]
    fn unknown_expressions_are_left_alone() {
        let out = render(
            json!({ "a": "{{ nope }}", "b": "x {{ foo.bar }} y", "c": "{{ unclosed" }),
            None,
        );
        assert_eq!(
            out,
            json!({ "a": "{{ nope }}", "b": "x {{ foo.bar }} y", "c": "{{ unclosed" })
        );
    }

    #[test]
    fn nested_arrays_and_objects() {
        let out = render(json!({ "data": [{ "id": "{{path.id}}" }] }), None);
        assert_eq!(out, json!({ "data": [{ "id": "42" }] }));
    }
}
