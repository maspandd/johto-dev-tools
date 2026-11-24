//! Schema helpers: `$ref` resolution, OpenAPI → JSON Schema conversion,
//! sample values for mock responses, and schema inference from examples.

use std::collections::HashSet;

use serde_json::{Map, Value, json};

use crate::issue::{Issue, join};

const MAX_REF_HOPS: usize = 16;
const MAX_SAMPLE_DEPTH: usize = 8;
const COMPONENT_SCHEMAS: &str = "#/components/schemas/";
pub(crate) const DEFS: &str = "#/$defs/";

/// Keywords whose value is a schema.
const SCHEMA_KEYWORDS: &[&str] = &[
    "items",
    "additionalProperties",
    "not",
    "contains",
    "propertyNames",
    "if",
    "then",
    "else",
    "unevaluatedItems",
    "unevaluatedProperties",
    "additionalItems",
];
/// Keywords whose value is a list of schemas.
const SCHEMA_LIST_KEYWORDS: &[&str] = &["allOf", "anyOf", "oneOf", "prefixItems"];
/// Keywords whose value maps names to schemas.
const SCHEMA_MAP_KEYWORDS: &[&str] = &[
    "properties",
    "patternProperties",
    "dependentSchemas",
    "$defs",
    "definitions",
];

/// Reads a local JSON pointer such as `#/components/schemas/User`.
pub(crate) fn pointer<'a>(doc: &'a Value, reference: &str) -> Option<&'a Value> {
    let path = reference.strip_prefix('#')?;
    if path.is_empty() {
        return Some(doc);
    }
    let mut node = doc;
    for token in path.strip_prefix('/')?.split('/') {
        let token = percent_decode(token).replace("~1", "/").replace("~0", "~");
        node = match node {
            Value::Object(map) => map.get(&token)?,
            Value::Array(items) => items.get(token.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(node)
}

fn percent_decode(s: &str) -> String {
    if !s.contains('%') {
        return s.to_owned();
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(hex) = s.get(i + 1..i + 3)
            && let Ok(b) = u8::from_str_radix(hex, 16)
        {
            out.push(b);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Follows `$ref` chains (parameters, responses, request bodies, …).
/// On failure, records an issue at `at` and returns `None`.
pub(crate) fn resolve<'a>(
    doc: &'a Value,
    value: &'a Value,
    at: &str,
    issues: &mut Vec<Issue>,
) -> Option<&'a Value> {
    let mut node = value;
    for _ in 0..MAX_REF_HOPS {
        let Some(reference) = node.get("$ref").and_then(Value::as_str) else {
            return Some(node);
        };
        if !reference.starts_with('#') {
            issues.push(Issue::error(
                at,
                format!("external reference `{reference}` is not supported"),
            ));
            return None;
        }
        let Some(target) = pointer(doc, reference) else {
            issues.push(Issue::error(
                at,
                format!("reference `{reference}` does not exist"),
            ));
            return None;
        };
        node = target;
    }
    issues.push(Issue::error(at, "reference chain is too long or circular"));
    None
}

/// Converts an OpenAPI schema into a self-contained JSON Schema (2020-12).
///
/// `#/components/schemas/X` references become `#/$defs/X`, with every
/// referenced component copied into `$defs`, so recursive schemas keep
/// working. OpenAPI 3.0 keywords (`nullable`, boolean `exclusiveMinimum`)
/// are rewritten to their JSON Schema equivalents.
pub(crate) fn to_json_schema(
    doc: &Value,
    schema: &Value,
    openapi_30: bool,
    at: &str,
    issues: &mut Vec<Issue>,
) -> Value {
    let mut conv = Converter {
        doc,
        openapi_30,
        at,
        issues,
        wanted: Vec::new(),
        inline_depth: 0,
    };
    let mut root = conv.convert(schema);
    let mut defs = Map::new();
    let mut done = HashSet::new();
    let mut next = 0;
    while next < conv.wanted.len() {
        let name = conv.wanted[next].clone();
        next += 1;
        if !done.insert(name.clone()) {
            continue;
        }
        let def = match doc
            .pointer("/components/schemas")
            .and_then(|s| s.get(&name))
        {
            Some(s) => conv.convert(s),
            None => {
                conv.issues.push(Issue::error(
                    at,
                    format!("schema `{name}` is referenced but not defined"),
                ));
                json!({})
            }
        };
        defs.insert(name, def);
    }
    if !defs.is_empty() {
        if let Value::Object(map) = &mut root {
            map.insert("$defs".to_owned(), Value::Object(defs));
        } else {
            root = json!({ "allOf": [root], "$defs": defs });
        }
    }
    root
}

struct Converter<'a, 'i> {
    doc: &'a Value,
    openapi_30: bool,
    at: &'a str,
    issues: &'i mut Vec<Issue>,
    wanted: Vec<String>,
    inline_depth: usize,
}

impl Converter<'_, '_> {
    fn convert(&mut self, schema: &Value) -> Value {
        let Value::Object(src) = schema else {
            return schema.clone();
        };
        if let Some(reference) = src.get("$ref").and_then(Value::as_str) {
            let mut out = self.convert_ref(reference);
            if !self.openapi_30
                && let Value::Object(map) = &mut out
            {
                for (k, v) in src {
                    if k != "$ref" {
                        map.insert(k.clone(), self.convert_keyword(k, v));
                    }
                }
            }
            return out;
        }
        let mut out = Map::new();
        for (k, v) in src {
            out.insert(k.clone(), self.convert_keyword(k, v));
        }
        if self.openapi_30 {
            rewrite_30(&mut out);
        }
        Value::Object(out)
    }

    fn convert_keyword(&mut self, key: &str, value: &Value) -> Value {
        if SCHEMA_KEYWORDS.contains(&key) && !value.is_boolean() {
            if key == "items" && value.is_array() {
                return self.convert_list(value);
            }
            return self.convert(value);
        }
        if SCHEMA_LIST_KEYWORDS.contains(&key) {
            return self.convert_list(value);
        }
        if SCHEMA_MAP_KEYWORDS.contains(&key)
            && let Value::Object(map) = value
        {
            return Value::Object(
                map.iter()
                    .map(|(k, v)| (k.clone(), self.convert(v)))
                    .collect(),
            );
        }
        value.clone()
    }

    fn convert_list(&mut self, value: &Value) -> Value {
        match value {
            Value::Array(items) => Value::Array(items.iter().map(|s| self.convert(s)).collect()),
            other => other.clone(),
        }
    }

    fn convert_ref(&mut self, reference: &str) -> Value {
        if let Some(name) = reference.strip_prefix(COMPONENT_SCHEMAS)
            && !name.contains('/')
        {
            let name = percent_decode(name).replace("~1", "/").replace("~0", "~");
            let target = format!("{DEFS}{}", crate::issue::escape(&name));
            self.wanted.push(name);
            return json!({ "$ref": target });
        }
        if !reference.starts_with('#') {
            self.issues.push(Issue::warning(
                self.at,
                format!("external schema reference `{reference}` was replaced by an empty schema"),
            ));
            return json!({});
        }
        match pointer(self.doc, reference) {
            Some(target) if self.inline_depth < MAX_REF_HOPS => {
                self.inline_depth += 1;
                let out = self.convert(target);
                self.inline_depth -= 1;
                out
            }
            Some(_) => {
                self.issues.push(Issue::warning(
                    self.at,
                    format!("schema reference `{reference}` is nested too deeply"),
                ));
                json!({})
            }
            None => {
                self.issues.push(Issue::error(
                    self.at,
                    format!("schema reference `{reference}` does not exist"),
                ));
                json!({})
            }
        }
    }
}

/// OpenAPI 3.0 → JSON Schema 2020-12 for one schema object.
fn rewrite_30(map: &mut Map<String, Value>) {
    if map.shift_remove("nullable") == Some(Value::Bool(true)) {
        match map.get_mut("type") {
            Some(Value::String(t)) => {
                let t = t.clone();
                map.insert("type".to_owned(), json!([t, "null"]));
                if let Some(Value::Array(values)) = map.get_mut("enum")
                    && !values.contains(&Value::Null)
                {
                    values.push(Value::Null);
                }
            }
            Some(_) => {}
            None => {
                let rest = std::mem::take(map);
                map.insert(
                    "anyOf".to_owned(),
                    json!([Value::Object(rest), { "type": "null" }]),
                );
            }
        }
    }
    for (flag, bound) in [
        ("exclusiveMinimum", "minimum"),
        ("exclusiveMaximum", "maximum"),
    ] {
        if let Some(Value::Bool(exclusive)) = map.get(flag).cloned() {
            map.shift_remove(flag);
            if exclusive && let Some(limit) = map.shift_remove(bound) {
                map.insert(flag.to_owned(), limit);
            }
        }
    }
}

/// A sample value for a converted (self-contained) schema. Strings with
/// well-known formats become response templates such as `{{ uuid }}`.
pub(crate) fn sample(root: &Value) -> Value {
    sample_node(root, root, 0)
}

fn sample_node(root: &Value, node: &Value, depth: usize) -> Value {
    let Value::Object(s) = node else {
        return Value::Null;
    };
    for key in ["example", "default", "const"] {
        if let Some(v) = s.get(key) {
            return v.clone();
        }
    }
    if let Some(Value::Array(examples)) = s.get("examples")
        && let Some(first) = examples.first()
    {
        return first.clone();
    }
    if let Some(Value::Array(values)) = s.get("enum")
        && let Some(first) = values.iter().find(|v| !v.is_null()).or(values.first())
    {
        return first.clone();
    }
    if depth > MAX_SAMPLE_DEPTH {
        return Value::Null;
    }
    if let Some(reference) = s.get("$ref").and_then(Value::as_str) {
        return pointer(root, reference)
            .map_or(Value::Null, |target| sample_node(root, target, depth + 1));
    }
    if let Some(Value::Array(parts)) = s.get("allOf") {
        let mut merged = Map::new();
        let mut last = Value::Null;
        for part in parts {
            match sample_node(root, part, depth + 1) {
                Value::Object(m) => merged.extend(m),
                other => last = other,
            }
        }
        return if merged.is_empty() {
            last
        } else {
            Value::Object(merged)
        };
    }
    for key in ["oneOf", "anyOf"] {
        if let Some(Value::Array(options)) = s.get(key) {
            let option = options
                .iter()
                .find(|o| o.get("type").and_then(Value::as_str) != Some("null"))
                .or(options.first());
            if let Some(option) = option {
                return sample_node(root, option, depth + 1);
            }
        }
    }
    let ty = match s.get("type") {
        Some(Value::String(t)) => Some(t.as_str()),
        Some(Value::Array(types)) => types
            .iter()
            .filter_map(Value::as_str)
            .find(|t| *t != "null")
            .or(Some("null")),
        _ => None,
    };
    let ty = ty.or_else(|| {
        if s.contains_key("properties") {
            Some("object")
        } else if s.contains_key("items") {
            Some("array")
        } else {
            None
        }
    });
    match ty {
        Some("object") => {
            let mut out = Map::new();
            if let Some(Value::Object(props)) = s.get("properties") {
                for (name, prop) in props {
                    out.insert(name.clone(), sample_node(root, prop, depth + 1));
                }
            }
            Value::Object(out)
        }
        Some("array") => match s.get("items") {
            Some(items) if items.is_object() => {
                let item = sample_node(root, items, depth + 1);
                if item.is_null() && depth >= MAX_SAMPLE_DEPTH {
                    json!([])
                } else {
                    json!([item])
                }
            }
            _ => json!([]),
        },
        Some("string") => Value::String(sample_string(s)),
        Some("integer") => {
            let min = s.get("minimum").and_then(Value::as_i64).unwrap_or(0);
            json!(min.max(0))
        }
        Some("number") => {
            let min = s.get("minimum").and_then(Value::as_f64).unwrap_or(0.0);
            json!(min.max(0.0))
        }
        Some("boolean") => Value::Bool(true),
        _ => Value::Null,
    }
}

fn sample_string(s: &Map<String, Value>) -> String {
    let format = s.get("format").and_then(Value::as_str).unwrap_or_default();
    match format {
        "uuid" => "{{ uuid }}",
        "date-time" => "{{ now }}",
        "date" => "2026-01-15",
        "time" => "12:00:00",
        "email" => "user@example.test",
        "uri" | "url" | "iri" => "https://example.test",
        "hostname" => "example.test",
        "ipv4" => "192.0.2.1",
        "ipv6" => "2001:db8::1",
        _ => "string",
    }
    .to_owned()
}

/// A JSON Schema describing `example`, used when exporting endpoints that
/// have an example body but no schema.
pub(crate) fn infer(example: &Value) -> Value {
    match example {
        Value::Null => json!({}),
        Value::Bool(_) => json!({ "type": "boolean" }),
        Value::Number(n) if n.is_i64() || n.is_u64() => json!({ "type": "integer" }),
        Value::Number(_) => json!({ "type": "number" }),
        Value::String(s) => match s.trim() {
            "{{ uuid }}" | "{{uuid}}" => json!({ "type": "string", "format": "uuid" }),
            "{{ now }}" | "{{now}}" => json!({ "type": "string", "format": "date-time" }),
            _ => json!({ "type": "string" }),
        },
        Value::Array(items) => json!({
            "type": "array",
            "items": items.first().map_or_else(|| json!({}), infer),
        }),
        Value::Object(map) => {
            let props: Map<String, Value> =
                map.iter().map(|(k, v)| (k.clone(), infer(v))).collect();
            json!({ "type": "object", "properties": props })
        }
    }
}

/// Media types treated as JSON.
pub(crate) fn is_json_media(media: &str) -> bool {
    let media = media.split(';').next().unwrap_or_default().trim();
    media.eq_ignore_ascii_case("application/json")
        || media.to_ascii_lowercase().ends_with("+json")
        || media == "*/*"
}

/// The JSON media entry of a `content` map, if any.
pub(crate) fn json_content<'a>(
    content: &'a Map<String, Value>,
    at: &str,
) -> Option<(String, &'a Value)> {
    content
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("application/json"))
        .or_else(|| content.iter().find(|(k, _)| is_json_media(k)))
        .map(|(k, v)| (join(&join(at, "content"), k), v))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc() -> Value {
        json!({
            "components": { "schemas": {
                "User": {
                    "type": "object",
                    "required": ["id"],
                    "properties": {
                        "id": { "type": "string", "format": "uuid" },
                        "nickname": { "type": "string", "nullable": true },
                        "manager": { "$ref": "#/components/schemas/User" },
                        "age": { "type": "integer", "minimum": 0, "exclusiveMinimum": true },
                        "role": { "$ref": "#/components/schemas/Role" }
                    }
                },
                "Role": { "type": "string", "enum": ["admin", "member"] }
            }}
        })
    }

    #[test]
    fn converts_30_schema_with_recursive_refs() {
        let doc = doc();
        let mut issues = Vec::new();
        let out = to_json_schema(
            &doc,
            &json!({ "$ref": "#/components/schemas/User" }),
            true,
            "/x",
            &mut issues,
        );
        assert!(issues.is_empty(), "{issues:?}");
        assert_eq!(out["$ref"], "#/$defs/User");
        let user = &out["$defs"]["User"];
        assert_eq!(
            user["properties"]["nickname"]["type"],
            json!(["string", "null"])
        );
        assert_eq!(user["properties"]["manager"]["$ref"], "#/$defs/User");
        assert_eq!(user["properties"]["age"]["exclusiveMinimum"], 0);
        assert!(user["properties"]["age"].get("minimum").is_none());
        assert_eq!(out["$defs"]["Role"]["enum"], json!(["admin", "member"]));

        let validator = jsonschema::validator_for(&out).unwrap();
        assert!(validator.is_valid(&json!({ "id": "x", "nickname": null, "role": "admin" })));
        assert!(validator.is_valid(&json!({ "id": "x", "manager": { "id": "y" } })));
        assert!(!validator.is_valid(&json!({ "id": "x", "manager": {} })));
        assert!(!validator.is_valid(&json!({ "id": "x", "age": 0 })));
        assert!(!validator.is_valid(&json!({ "id": "x", "role": "boss" })));
    }

    #[test]
    fn property_names_are_not_keywords() {
        let doc = json!({});
        let mut issues = Vec::new();
        let out = to_json_schema(
            &doc,
            &json!({ "type": "object", "properties": { "nullable": { "type": "boolean" } } }),
            true,
            "/x",
            &mut issues,
        );
        assert_eq!(out["properties"]["nullable"]["type"], "boolean");
    }

    #[test]
    fn reports_missing_and_external_refs() {
        let doc = json!({});
        let mut issues = Vec::new();
        to_json_schema(
            &doc,
            &json!({ "properties": {
                "a": { "$ref": "#/components/schemas/Nope" },
                "b": { "$ref": "other.yaml#/X" }
            }}),
            false,
            "/x",
            &mut issues,
        );
        assert_eq!(issues.len(), 2, "{issues:?}");
    }

    #[test]
    fn samples_use_examples_formats_and_templates() {
        let doc = doc();
        let mut issues = Vec::new();
        let schema = to_json_schema(
            &doc,
            &json!({ "$ref": "#/components/schemas/User" }),
            true,
            "/x",
            &mut issues,
        );
        let s = sample(&schema);
        assert_eq!(s["id"], "{{ uuid }}");
        assert_eq!(s["role"], "admin");
        assert_eq!(s["nickname"], "string");
        assert!(
            s["manager"].is_object(),
            "recursion is bounded, not cut at once"
        );
        assert_eq!(
            sample(&json!({ "type": "array", "items": { "type": "integer", "example": 7 } })),
            json!([7])
        );
    }

    #[test]
    fn infers_schema_from_example() {
        assert_eq!(
            infer(&json!({ "id": "{{ uuid }}", "n": 1, "tags": ["a"] })),
            json!({ "type": "object", "properties": {
                "id": { "type": "string", "format": "uuid" },
                "n": { "type": "integer" },
                "tags": { "type": "array", "items": { "type": "string" } }
            }})
        );
    }

    #[test]
    fn pointer_unescapes() {
        let doc = json!({ "paths": { "/a/b": { "get": 1 } } });
        assert_eq!(pointer(&doc, "#/paths/~1a~1b/get"), Some(&json!(1)));
        assert_eq!(pointer(&doc, "#/paths/%7E1a~1b/get"), Some(&json!(1)));
        assert_eq!(pointer(&doc, "#/paths/missing"), None);
    }
}
