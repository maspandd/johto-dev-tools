//! Pagination and merge-patch helpers shared by the route modules.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use workbench_core::ErrorDetail;

use crate::ApiError;

const DEFAULT_LIMIT: usize = 50;
const MAX_LIMIT: usize = 200;

/// `?page=&limit=` for collections. `page` starts at 1; `limit` is capped at 200.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
pub struct PageQuery {
    pub page: Option<usize>,
    pub limit: Option<usize>,
}

/// `{ "data": [...], "meta": { page, limit, total, total_pages } }`
pub fn paginate<T: Serialize>(items: Vec<T>, q: PageQuery) -> Result<Value, ApiError> {
    let page = q.page.unwrap_or(1);
    let limit = q.limit.unwrap_or(DEFAULT_LIMIT).min(MAX_LIMIT);
    if page == 0 || limit == 0 {
        return Err(ApiError::validation(vec![ErrorDetail::new(
            "query",
            "page and limit must be at least 1",
        )]));
    }
    let total = items.len();
    let data: Vec<T> = items
        .into_iter()
        .skip((page - 1).saturating_mul(limit))
        .take(limit)
        .collect();
    Ok(json!({
        "data": data,
        "meta": { "page": page, "limit": limit, "total": total, "total_pages": total.div_ceil(limit) }
    }))
}

/// `{ "data": value }`
pub fn data<T: Serialize>(value: T) -> Value {
    json!({ "data": value })
}

/// RFC 7386 JSON merge patch.
fn merge(target: &mut Value, patch: &Value) {
    let Value::Object(changes) = patch else {
        *target = patch.clone();
        return;
    };
    if !target.is_object() {
        *target = Value::Object(Map::new());
    }
    if let Value::Object(t) = target {
        for (k, v) in changes {
            if v.is_null() {
                t.shift_remove(k);
            } else {
                merge(t.entry(k.clone()).or_insert(Value::Null), v);
            }
        }
    }
}

/// Applies a JSON merge patch to `current`. Keys in `fixed` (IDs, owners,
/// timestamps) cannot be changed and are ignored if present.
pub fn apply_patch<T: Serialize + DeserializeOwned>(
    current: &T,
    patch: &Value,
    fixed: &[&str],
) -> Result<T, ApiError> {
    let Value::Object(changes) = patch else {
        return Err(ApiError::validation(vec![ErrorDetail::new(
            "body",
            "must be a JSON object",
        )]));
    };
    let mut changes = changes.clone();
    for key in fixed {
        changes.shift_remove(*key);
    }
    let mut value = serde_json::to_value(current).map_err(|_| ApiError::internal())?;
    merge(&mut value, &Value::Object(changes));
    serde_json::from_value(value)
        .map_err(|e| ApiError::validation(vec![ErrorDetail::new("body", e.to_string())]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct Thing {
        id: u32,
        name: String,
        #[serde(default)]
        tags: Vec<String>,
    }

    #[test]
    fn patch_ignores_fixed_fields_and_validates_shape() {
        let t = Thing {
            id: 1,
            name: "a".into(),
            tags: vec!["x".into()],
        };
        let out = apply_patch(&t, &json!({ "id": 9, "name": "b", "tags": null }), &["id"]).unwrap();
        assert_eq!(
            out,
            Thing {
                id: 1,
                name: "b".into(),
                tags: vec![]
            }
        );
        assert!(apply_patch(&t, &json!({ "name": 5 }), &[]).is_err());
        assert!(apply_patch(&t, &json!([1]), &[]).is_err());
    }

    #[test]
    fn paginates() {
        let v = paginate(
            (1..=7).collect::<Vec<_>>(),
            PageQuery {
                page: Some(2),
                limit: Some(3),
            },
        )
        .unwrap();
        assert_eq!(v["data"], json!([4, 5, 6]));
        assert_eq!(
            v["meta"],
            json!({ "page": 2, "limit": 3, "total": 7, "total_pages": 3 })
        );
        assert!(
            paginate(
                vec![1],
                PageQuery {
                    page: Some(0),
                    limit: None
                }
            )
            .is_err()
        );
    }
}
