//! CRUD simulation for resources (spec §4.5).
//!
//! | Method | Path          | Behaviour                                   |
//! |--------|---------------|---------------------------------------------|
//! | GET    | `/users`      | list with filters, `_q`, `_sort`, paging    |
//! | POST   | `/users`      | create (key assigned if missing)            |
//! | GET    | `/users/:id`  | read one                                    |
//! | PUT    | `/users/:id`  | replace (key cannot change)                 |
//! | PATCH  | `/users/:id`  | JSON merge patch (RFC 7386)                 |
//! | DELETE | `/users/:id`  | hard delete, or soft delete via `deleted_at`|
//!
//! List query parameters: any top-level field for exact matches (repeat a
//! field to match any of several values), `_q` for case-insensitive search,
//! `_sort=name,-age` (or `_sort=name&_order=desc`), `_page` and `_limit`.
//! Responses are `{ "data": ... }`; lists add `"meta"` and `X-Total-Count`.

use std::cmp::Ordering;
use std::collections::HashMap;

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use serde_json::{Map, Value, json};
use thiserror::Error;
use workbench_core::{ErrorCode, ErrorDetail, ResourceId};
use workbench_domain::{
    DeleteBehavior, FieldKind, FieldSpec, KeyStrategy, RecordChange, Resource, StoredRecord, key_of,
};

use crate::data::{Collection, DataStore};
use crate::engine::{error_outcome, json_outcome, parse_body};
use crate::{Entropy, MockOutcome, MockRequest};

/// Path parameter name for item routes; never visible to users.
pub(crate) const KEY_PARAM: &str = "__key";
const DELETED_AT: &str = "deleted_at";
const SEGMENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CrudOp {
    List,
    Create,
    Get,
    Replace,
    Patch,
    Delete,
}

/// A top-level `reference` field checked on every write.
pub(crate) struct Reference {
    pub field: String,
    pub target: ResourceId,
    pub target_name: String,
    pub target_field: String,
    pub target_soft_delete: bool,
}

pub(crate) struct CompiledResource {
    pub resource: Resource,
    pub validator: Option<jsonschema::Validator>,
    /// Absolute collection path including any base path, e.g. `/api/users`.
    pub collection_url: String,
    pub references: Vec<Reference>,
}

/// Why seed data could not be generated.
#[derive(Debug, Error)]
pub enum SeedError {
    #[error("resource {0} is not part of this engine")]
    UnknownResource(ResourceId),
    #[error("resource `{resource}`: {source}")]
    Generate {
        resource: String,
        #[source]
        source: data_generator::GenerateError,
    },
    #[error("resource `{resource}`: generated record {index} has no usable `{primary_key}`")]
    InvalidKey {
        resource: String,
        primary_key: String,
        index: usize,
    },
    #[error("resource `{resource}`: generated key `{key}` is not unique; mark the field unique")]
    DuplicateKey { resource: String, key: String },
}

fn text_of(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn is_deleted(soft: bool, record: &Value) -> bool {
    soft && record.get(DELETED_AT).is_some_and(|v| !v.is_null())
}

fn rank(v: Option<&Value>) -> u8 {
    match v {
        None | Some(Value::Null) => 0,
        Some(Value::Bool(_)) => 1,
        Some(Value::Number(_)) => 2,
        Some(Value::String(_)) => 3,
        Some(_) => 4,
    }
}

fn compare(a: Option<&Value>, b: Option<&Value>) -> Ordering {
    match (a, b) {
        (Some(Value::Bool(x)), Some(Value::Bool(y))) => x.cmp(y),
        (Some(Value::Number(x)), Some(Value::Number(y))) => x
            .as_f64()
            .partial_cmp(&y.as_f64())
            .unwrap_or(Ordering::Equal),
        (Some(Value::String(x)), Some(Value::String(y))) => x.cmp(y),
        _ => {
            // Missing and `null` sort together, first.
            let text = |v: Option<&Value>| v.filter(|v| !v.is_null()).map(Value::to_string);
            rank(a).cmp(&rank(b)).then_with(|| text(a).cmp(&text(b)))
        }
    }
}

/// RFC 7386 JSON merge patch. Removing a key keeps the order of the others.
fn merge_patch(target: &mut Value, patch: &Value) {
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
                merge_patch(t.entry(k.clone()).or_insert(Value::Null), v);
            }
        }
    }
}

/// Puts the primary key first, followed by the other fields in order.
fn with_key_first(pk: &str, key: Value, mut rest: Map<String, Value>) -> Value {
    rest.shift_remove(pk);
    let mut out = Map::with_capacity(rest.len() + 1);
    out.insert(pk.to_owned(), key);
    out.extend(rest);
    Value::Object(out)
}

fn not_found(req: &MockRequest, r: &Resource, key: &str) -> MockOutcome {
    error_outcome(
        req,
        ErrorCode::RecordNotFound,
        format!("No `{}` record has {} `{key}`.", r.name, r.primary_key),
        Vec::new(),
    )
}

fn invalid(req: &MockRequest, details: Vec<ErrorDetail>) -> MockOutcome {
    error_outcome(
        req,
        ErrorCode::RequestValidationFailed,
        "The request did not pass validation.",
        details,
    )
}

impl CompiledResource {
    fn soft(&self) -> bool {
        self.resource.delete == DeleteBehavior::Soft
    }

    /// Parses a write body that must be a JSON object.
    fn object_body(&self, req: &MockRequest) -> Result<Map<String, Value>, Box<MockOutcome>> {
        match parse_body(req, true) {
            Err(message) => Err(Box::new(error_outcome(
                req,
                ErrorCode::InvalidJsonBody,
                "The request body is not valid JSON.",
                vec![ErrorDetail::new("body", message)],
            ))),
            Ok(None) => Err(Box::new(invalid(
                req,
                vec![ErrorDetail::new("body", "request body is required")],
            ))),
            Ok(Some(Value::Object(map))) => Ok(map),
            Ok(Some(_)) => Err(Box::new(invalid(
                req,
                vec![ErrorDetail::new("body", "must be a JSON object")],
            ))),
        }
    }

    /// Schema and reference checks on the record that would be stored.
    fn check(&self, record: &Value, all: &HashMap<ResourceId, Collection>) -> Vec<ErrorDetail> {
        let mut details: Vec<ErrorDetail> = self
            .validator
            .as_ref()
            .map(|v| {
                v.iter_errors(record)
                    .map(|e| ErrorDetail::new(format!("body{}", e.instance_path()), e.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        for r in &self.references {
            let Some(value) = record.get(&r.field).filter(|v| !v.is_null()) else {
                continue;
            };
            let wanted = text_of(value);
            let exists = all.get(&r.target).is_some_and(|c| {
                c.iter().any(|(_, rec)| {
                    !is_deleted(r.target_soft_delete, rec)
                        && rec
                            .get(&r.target_field)
                            .is_some_and(|v| text_of(v) == wanted)
                })
            });
            if !exists {
                details.push(ErrorDetail::new(
                    format!("body/{}", r.field),
                    format!(
                        "no `{}` record has {} `{wanted}`",
                        r.target_name, r.target_field
                    ),
                ));
            }
        }
        details
    }

    fn location(&self, key: &str) -> String {
        format!(
            "{}/{}",
            self.collection_url,
            utf8_percent_encode(key, SEGMENT)
        )
    }

    fn live<'c>(&self, coll: &'c Collection, key: &str) -> Option<&'c Value> {
        coll.get(key).filter(|r| !is_deleted(self.soft(), r))
    }

    pub(crate) fn execute(
        &self,
        op: CrudOp,
        key: Option<&str>,
        req: &MockRequest,
        data: &DataStore,
        entropy: &dyn Entropy,
    ) -> MockOutcome {
        let mut all = data.lock();
        let id = self.resource.id;
        let key = key.unwrap_or_default();
        let mut out = if all.contains_key(&id) {
            match op {
                CrudOp::List => self.list(req, &all[&id]),
                CrudOp::Get => match self.live(&all[&id], key) {
                    Some(r) => json_outcome(200, &json!({ "data": r })),
                    None => not_found(req, &self.resource, key),
                },
                CrudOp::Create => self.create(req, &mut all, entropy),
                CrudOp::Replace | CrudOp::Patch => self.update(op, key, req, &mut all),
                CrudOp::Delete => self.delete(key, req, &mut all, entropy),
            }
        } else {
            error_outcome(
                req,
                ErrorCode::InternalError,
                format!("Data for `{}` is not loaded.", self.resource.name),
                Vec::new(),
            )
        };
        // Still holding the lock: listeners see changes in applied order.
        data.notify(&out.changes);
        drop(all);
        out.matched_resource = Some(id);
        out
    }

    fn list(&self, req: &MockRequest, coll: &Collection) -> MockOutcome {
        let r = &self.resource;
        let mut details = Vec::new();
        let mut number = |name: &str, default: usize| match req.query_param(name) {
            None => default,
            Some(raw) => match raw.parse::<usize>() {
                Ok(n) if n >= 1 => n,
                _ => {
                    details.push(ErrorDetail::new(
                        format!("query.{name}"),
                        "must be a positive integer",
                    ));
                    default
                }
            },
        };
        let page = number("_page", 1);
        let limit = number("_limit", r.pagination.default_limit).min(r.pagination.max_limit);
        if !details.is_empty() {
            return invalid(req, details);
        }

        let mut filters: Vec<(&str, Vec<&str>)> = Vec::new();
        for (k, v) in req.query.iter().filter(|(k, _)| !k.starts_with('_')) {
            match filters.iter_mut().find(|(f, _)| f == k) {
                Some((_, values)) => values.push(v),
                None => filters.push((k, vec![v])),
            }
        }
        let search = req
            .query_param("_q")
            .map(str::to_lowercase)
            .filter(|q| !q.is_empty());

        let mut rows: Vec<&Value> = coll
            .iter()
            .map(|(_, v)| v)
            .filter(|v| !is_deleted(self.soft(), v))
            .filter(|v| {
                filters.iter().all(|(field, values)| {
                    v.get(*field)
                        .is_some_and(|actual| values.contains(&text_of(actual).as_str()))
                })
            })
            .filter(|v| {
                let Some(q) = &search else { return true };
                let matches =
                    |val: &Value| val.as_str().is_some_and(|s| s.to_lowercase().contains(q));
                if r.searchable.is_empty() {
                    v.as_object().is_some_and(|m| m.values().any(matches))
                } else {
                    r.searchable.iter().any(|f| v.get(f).is_some_and(matches))
                }
            })
            .collect();

        if let Some(sort) = req.query_param("_sort") {
            let desc_all = req
                .query_param("_order")
                .is_some_and(|o| o.eq_ignore_ascii_case("desc"));
            let keys: Vec<(&str, bool)> = sort
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| match s.strip_prefix('-') {
                    Some(field) => (field, true),
                    None => (s, desc_all),
                })
                .collect();
            rows.sort_by(|a, b| {
                keys.iter()
                    .map(|(f, desc)| {
                        let o = compare(a.get(*f), b.get(*f));
                        if *desc { o.reverse() } else { o }
                    })
                    .find(|o| o.is_ne())
                    .unwrap_or(Ordering::Equal)
            });
        }

        let total = rows.len();
        let items: Vec<&Value> = rows
            .into_iter()
            .skip((page - 1).saturating_mul(limit))
            .take(limit)
            .collect();
        let mut out = json_outcome(
            200,
            &json!({
                "data": items,
                "meta": { "page": page, "limit": limit, "total": total, "total_pages": total.div_ceil(limit) }
            }),
        );
        out.headers
            .push(("x-total-count".to_owned(), total.to_string()));
        out
    }

    fn create(
        &self,
        req: &MockRequest,
        all: &mut HashMap<ResourceId, Collection>,
        entropy: &dyn Entropy,
    ) -> MockOutcome {
        let r = &self.resource;
        let pk = r.primary_key.as_str();
        let body = match self.object_body(req) {
            Ok(b) => b,
            Err(out) => return *out,
        };
        let coll = &all[&r.id];
        let (key, key_value) = match body.get(pk) {
            Some(v) => match key_of(v) {
                Some(k) => (k, v.clone()),
                None => {
                    return invalid(
                        req,
                        vec![ErrorDetail::new(
                            format!("body/{pk}"),
                            "must be a non-empty string or a number",
                        )],
                    );
                }
            },
            None => {
                let v = match r.key_strategy {
                    KeyStrategy::Uuid => Value::String(entropy.uuid()),
                    KeyStrategy::Increment => {
                        let max = coll.iter().filter_map(|(k, _)| k.parse::<i64>().ok()).max();
                        Value::from(max.map_or(1, |m| m.saturating_add(1)))
                    }
                };
                (key_of(&v).unwrap_or_default(), v)
            }
        };
        if coll.get(&key).is_some() {
            return error_outcome(
                req,
                ErrorCode::RecordConflict,
                format!("A `{}` record with {pk} `{key}` already exists.", r.name),
                Vec::new(),
            );
        }
        let record = with_key_first(pk, key_value, body);
        let details = self.check(&record, all);
        if !details.is_empty() {
            return invalid(req, details);
        }
        let Some(coll) = all.get_mut(&r.id) else {
            return not_found(req, r, &key);
        };
        let position = coll.next_position();
        coll.upsert(key.clone(), position, record.clone());

        let mut out = json_outcome(201, &json!({ "data": record }));
        out.headers
            .push(("location".to_owned(), self.location(&key)));
        out.changes.push((
            r.id,
            RecordChange::Upsert {
                record: StoredRecord {
                    key,
                    position,
                    data: record,
                },
            },
        ));
        out
    }

    fn update(
        &self,
        op: CrudOp,
        key: &str,
        req: &MockRequest,
        all: &mut HashMap<ResourceId, Collection>,
    ) -> MockOutcome {
        let r = &self.resource;
        let pk = r.primary_key.as_str();
        let Some(existing) = self.live(&all[&r.id], key).cloned() else {
            return not_found(req, r, key);
        };
        let body = match self.object_body(req) {
            Ok(b) => b,
            Err(out) => return *out,
        };
        if let Some(v) = body.get(pk)
            && key_of(v).as_deref() != Some(key)
        {
            return invalid(
                req,
                vec![ErrorDetail::new(
                    format!("body/{pk}"),
                    "the primary key cannot be changed",
                )],
            );
        }
        let key_value = existing
            .get(pk)
            .cloned()
            .unwrap_or_else(|| Value::from(key));
        let record = if op == CrudOp::Patch {
            let mut merged = existing;
            merge_patch(&mut merged, &Value::Object(body));
            match merged {
                Value::Object(m) => with_key_first(pk, key_value, m),
                _ => unreachable!("merge patch of an object with an object is an object"),
            }
        } else {
            with_key_first(pk, key_value, body)
        };
        let details = self.check(&record, all);
        if !details.is_empty() {
            return invalid(req, details);
        }
        let Some(coll) = all.get_mut(&r.id) else {
            return not_found(req, r, key);
        };
        let position = coll.position(key).unwrap_or_else(|| coll.next_position());
        coll.upsert(key.to_owned(), position, record.clone());

        let mut out = json_outcome(200, &json!({ "data": record }));
        out.changes.push((
            r.id,
            RecordChange::Upsert {
                record: StoredRecord {
                    key: key.to_owned(),
                    position,
                    data: record,
                },
            },
        ));
        out
    }

    fn delete(
        &self,
        key: &str,
        req: &MockRequest,
        all: &mut HashMap<ResourceId, Collection>,
        entropy: &dyn Entropy,
    ) -> MockOutcome {
        let r = &self.resource;
        let Some(coll) = all.get_mut(&r.id) else {
            return not_found(req, r, key);
        };
        let Some(existing) = self.live(coll, key).cloned() else {
            return not_found(req, r, key);
        };
        let change = if self.soft() {
            let mut record = existing;
            if let Value::Object(m) = &mut record {
                m.insert(
                    DELETED_AT.to_owned(),
                    Value::String(entropy.now().to_rfc3339()),
                );
            }
            let position = coll.position(key).unwrap_or_default();
            coll.upsert(key.to_owned(), position, record.clone());
            RecordChange::Upsert {
                record: StoredRecord {
                    key: key.to_owned(),
                    position,
                    data: record,
                },
            }
        } else {
            coll.remove(key);
            RecordChange::Delete {
                key: key.to_owned(),
            }
        };
        let mut out = MockOutcome::new(204);
        out.changes.push((r.id, change));
        out
    }

    /// Generates seed records. `refs` holds current records of other
    /// resources by name, for `reference` fields.
    pub(crate) fn seed(
        &self,
        refs: &HashMap<String, Vec<Value>>,
    ) -> Result<Vec<StoredRecord>, SeedError> {
        let r = &self.resource;
        let Some(spec) = &r.seed else {
            return Ok(Vec::new());
        };
        let mut schema = r.schema.clone();
        if !schema.fields.iter().any(|f| f.name == r.primary_key) {
            let kind = match r.key_strategy {
                KeyStrategy::Uuid => FieldKind::Uuid,
                KeyStrategy::Increment => FieldKind::Sequence { start: 1, step: 1 },
            };
            schema
                .fields
                .insert(0, FieldSpec::new(r.primary_key.clone(), kind));
        }
        let mut references = data_generator::References::new();
        for (name, records) in refs {
            references.insert(name, records);
        }
        let generated =
            data_generator::generate(&schema, spec.count, spec.seed, spec.locale, &references)
                .map_err(|source| SeedError::Generate {
                    resource: r.name.clone(),
                    source,
                })?;
        let mut seen = std::collections::HashSet::new();
        generated
            .into_iter()
            .enumerate()
            .map(|(index, data)| {
                let key = data.get(&r.primary_key).and_then(key_of).ok_or_else(|| {
                    SeedError::InvalidKey {
                        resource: r.name.clone(),
                        primary_key: r.primary_key.clone(),
                        index,
                    }
                })?;
                if !seen.insert(key.clone()) {
                    return Err(SeedError::DuplicateKey {
                        resource: r.name.clone(),
                        key,
                    });
                }
                Ok(StoredRecord {
                    key,
                    position: i64::try_from(index).unwrap_or(i64::MAX),
                    data,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_patch_follows_rfc_7386() {
        let mut doc = json!({ "a": "b", "c": { "d": "e", "f": "g" }, "z": 1 });
        merge_patch(&mut doc, &json!({ "a": "z", "c": { "f": null }, "n": [1] }));
        assert_eq!(
            doc,
            json!({ "a": "z", "c": { "d": "e" }, "z": 1, "n": [1] })
        );
        let keys: Vec<_> = doc.as_object().unwrap().keys().cloned().collect();
        assert_eq!(keys, ["a", "c", "z", "n"], "order is kept");
    }

    #[test]
    fn compare_orders_mixed_values() {
        let (n, s, null) = (json!(2), json!("a"), json!(null));
        assert_eq!(compare(Some(&n), Some(&json!(10))), Ordering::Less);
        assert_eq!(compare(Some(&null), Some(&n)), Ordering::Less);
        assert_eq!(compare(None, Some(&null)), Ordering::Equal);
        assert_eq!(compare(Some(&n), Some(&s)), Ordering::Less);
    }
}
