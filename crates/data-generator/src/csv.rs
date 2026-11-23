//! CSV export. Nested objects become dotted columns (`address.city`);
//! arrays are written as JSON text; `null` is an empty cell.

use serde_json::{Map, Value};

fn flatten(prefix: &str, map: &Map<String, Value>, out: &mut Vec<(String, Value)>) {
    for (k, v) in map {
        let key = if prefix.is_empty() {
            k.clone()
        } else {
            format!("{prefix}.{k}")
        };
        match v {
            Value::Object(inner) => flatten(&key, inner, out),
            other => out.push((key, other.clone())),
        }
    }
}

fn cell(value: Option<&Value>) -> String {
    let text = match value {
        None | Some(Value::Null) => return String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    };
    if text.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text
    }
}

/// Renders records as CSV with a header row. Columns appear in first-seen order.
#[must_use]
pub fn to_csv(records: &[Value]) -> String {
    let rows: Vec<Vec<(String, Value)>> = records
        .iter()
        .map(|r| {
            let mut flat = Vec::new();
            match r {
                Value::Object(map) => flatten("", map, &mut flat),
                other => flat.push(("value".to_owned(), other.clone())),
            }
            flat
        })
        .collect();

    let mut columns: Vec<String> = Vec::new();
    for row in &rows {
        for (k, _) in row {
            if !columns.contains(k) {
                columns.push(k.clone());
            }
        }
    }

    let mut out = columns
        .iter()
        .map(|c| cell(Some(&Value::String(c.clone()))))
        .collect::<Vec<_>>()
        .join(",");
    out.push('\n');
    for row in &rows {
        let line = columns
            .iter()
            .map(|c| cell(row.iter().find(|(k, _)| k == c).map(|(_, v)| v)))
            .collect::<Vec<_>>()
            .join(",");
        out.push_str(&line);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn flattens_quotes_and_unions_columns() {
        let csv = to_csv(&[
            json!({ "id": 1, "name": "Alex, Jr.", "address": { "city": "Fairview" }, "tags": ["a", "b"] }),
            json!({ "id": 2, "name": "Say \"hi\"", "extra": null, "note": "x" }),
        ]);
        assert_eq!(
            csv,
            "id,name,address.city,tags,extra,note\n\
             1,\"Alex, Jr.\",Fairview,\"[\"\"a\"\",\"\"b\"\"]\",,\n\
             2,\"Say \"\"hi\"\"\",,,,x\n"
        );
    }

    #[test]
    fn empty_input() {
        assert_eq!(to_csv(&[]), "\n");
    }
}
