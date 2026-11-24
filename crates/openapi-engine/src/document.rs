//! Reading and writing documents as JSON or YAML.

use serde_json::{Map, Number, Value};
use thiserror::Error;

/// Largest document [`parse_document`] accepts.
pub const MAX_DOCUMENT_BYTES: usize = 5 * 1024 * 1024;

/// Deepest nesting accepted, to keep recursive processing bounded.
const MAX_DEPTH: usize = 128;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ParseError {
    #[error("document is larger than {max} bytes")]
    TooLarge { max: usize },
    #[error("document is empty")]
    Empty,
    #[error("not valid JSON or YAML: {0}")]
    Syntax(String),
    #[error("document is nested more than {MAX_DEPTH} levels deep")]
    TooDeep,
    #[error("document must be a JSON or YAML object")]
    NotAnObject,
}

/// Parses JSON or YAML text into a JSON value.
///
/// YAML keys that are numbers or booleans (such as unquoted `200:` response
/// codes) become strings, as OpenAPI expects.
pub fn parse_document(text: &str) -> Result<Value, ParseError> {
    if text.len() > MAX_DOCUMENT_BYTES {
        return Err(ParseError::TooLarge {
            max: MAX_DOCUMENT_BYTES,
        });
    }
    let text = text.trim_start_matches('\u{feff}');
    if text.trim().is_empty() {
        return Err(ParseError::Empty);
    }
    let value = if text.trim_start().starts_with('{') {
        serde_json::from_str(text).map_err(|e| ParseError::Syntax(e.to_string()))?
    } else {
        let mut yaml: serde_yaml_ng::Value =
            serde_yaml_ng::from_str(text).map_err(|e| ParseError::Syntax(e.to_string()))?;
        yaml.apply_merge()
            .map_err(|e| ParseError::Syntax(e.to_string()))?;
        from_yaml(yaml, 0)?
    };
    if !value.is_object() {
        return Err(ParseError::NotAnObject);
    }
    if depth(&value) > MAX_DEPTH {
        return Err(ParseError::TooDeep);
    }
    Ok(value)
}

fn from_yaml(value: serde_yaml_ng::Value, level: usize) -> Result<Value, ParseError> {
    use serde_yaml_ng::Value as Y;
    if level > MAX_DEPTH {
        return Err(ParseError::TooDeep);
    }
    Ok(match value {
        Y::Null => Value::Null,
        Y::Bool(b) => Value::Bool(b),
        Y::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::from(i)
            } else if let Some(u) = n.as_u64() {
                Value::from(u)
            } else {
                n.as_f64()
                    .and_then(Number::from_f64)
                    .map_or_else(|| Value::String(n.to_string()), Value::Number)
            }
        }
        Y::String(s) => Value::String(s),
        Y::Sequence(items) => Value::Array(
            items
                .into_iter()
                .map(|v| from_yaml(v, level + 1))
                .collect::<Result<_, _>>()?,
        ),
        Y::Mapping(map) => {
            let mut out = Map::new();
            for (k, v) in map {
                let key = match k {
                    Y::String(s) => s,
                    Y::Number(n) => n.to_string(),
                    Y::Bool(b) => b.to_string(),
                    Y::Null => "null".to_owned(),
                    _ => {
                        return Err(ParseError::Syntax(
                            "mapping keys must be strings, numbers or booleans".to_owned(),
                        ));
                    }
                };
                out.insert(key, from_yaml(v, level + 1)?);
            }
            Value::Object(out)
        }
        Y::Tagged(tagged) => from_yaml(tagged.value, level)?,
    })
}

fn depth(value: &Value) -> usize {
    match value {
        Value::Array(items) => 1 + items.iter().map(depth).max().unwrap_or(0),
        Value::Object(map) => 1 + map.values().map(depth).max().unwrap_or(0),
        _ => 0,
    }
}

/// Pretty JSON.
#[must_use]
pub fn to_json(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default()
}

/// YAML, keeping key order.
#[must_use]
pub fn to_yaml(value: &Value) -> String {
    serde_yaml_ng::to_string(value).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_json_and_yaml_alike() {
        let from_json = parse_document(r#"{"openapi":"3.1.0","paths":{}}"#).unwrap();
        let from_yaml = parse_document("openapi: 3.1.0\npaths: {}\n").unwrap();
        assert_eq!(from_json, from_yaml);
    }

    #[test]
    fn yaml_number_keys_become_strings() {
        let v = parse_document("responses:\n  200:\n    description: ok\n  404: {}\n").unwrap();
        assert_eq!(v["responses"]["200"]["description"], "ok");
        assert!(v["responses"]["404"].is_object());
    }

    #[test]
    fn yaml_merge_keys_and_anchors() {
        let v = parse_document("base: &b {a: 1}\nother:\n  <<: *b\n  c: 2\n").unwrap();
        assert_eq!(v["other"], json!({ "a": 1, "c": 2 }));
    }

    #[test]
    fn rejects_bad_input() {
        assert_eq!(parse_document("  "), Err(ParseError::Empty));
        assert_eq!(parse_document("- a\n- b\n"), Err(ParseError::NotAnObject));
        assert!(matches!(
            parse_document("{ nope"),
            Err(ParseError::Syntax(_))
        ));
        let deep = "[".repeat(200) + &"]".repeat(200);
        assert!(parse_document(&format!("{{\"a\": {deep}}}")).is_err());
        let big = "a".repeat(MAX_DOCUMENT_BYTES + 1);
        assert_eq!(
            parse_document(&big),
            Err(ParseError::TooLarge {
                max: MAX_DOCUMENT_BYTES
            })
        );
    }

    #[test]
    fn yaml_round_trip_keeps_order() {
        let v =
            json!({ "openapi": "3.1.0", "info": { "title": "T", "version": "1" }, "paths": {} });
        let text = to_yaml(&v);
        assert!(text.find("openapi").unwrap() < text.find("info").unwrap());
        assert_eq!(parse_document(&text).unwrap(), v);
    }
}
