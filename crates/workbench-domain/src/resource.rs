//! CRUD resources (spec §4.5): a resource such as `users` at `/users`
//! answers list, get, create, replace, patch and delete with stateful data.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use workbench_core::ResourceId;

use crate::dataset::{DataSchema, Locale, MAX_RECORDS};
use crate::validation::{Errors, FieldError, is_identifier};
use crate::{PathTemplate, Segment};

/// Most records a list request may return.
pub const MAX_PAGE_LIMIT: usize = 1_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Resource {
    #[serde(default)]
    pub id: ResourceId,
    /// Identifier used by references from other resources, e.g. `users`.
    pub name: String,
    /// Collection path, e.g. `/users`; items live at `/users/:id`.
    pub path: PathTemplate,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Field holding each record's unique key.
    #[serde(default = "id_field")]
    pub primary_key: String,
    /// How keys are assigned to new records that do not provide one.
    #[serde(default)]
    pub key_strategy: KeyStrategy,
    /// Shape of seeded records. `reference` fields are checked on writes.
    #[serde(default)]
    pub schema: DataSchema,
    /// Initial data generated the first time the resource starts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<SeedSpec>,
    /// JSON Schema for create and replace bodies, and for patched records.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_schema: Option<Value>,
    #[serde(default)]
    pub pagination: Pagination,
    /// Top-level fields matched by the `_q` search parameter.
    #[serde(default)]
    pub searchable: Vec<String>,
    #[serde(default)]
    pub delete: DeleteBehavior,
}

const fn yes() -> bool {
    true
}
fn id_field() -> String {
    "id".to_owned()
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyStrategy {
    /// Random UUID strings.
    #[default]
    Uuid,
    /// 1, 2, 3, … (one more than the largest numeric key).
    Increment,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SeedSpec {
    pub count: usize,
    #[serde(default)]
    pub seed: u64,
    #[serde(default)]
    pub locale: Locale,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pagination {
    pub default_limit: usize,
    pub max_limit: usize,
}

impl Default for Pagination {
    fn default() -> Self {
        Self {
            default_limit: 20,
            max_limit: 100,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeleteBehavior {
    /// Remove the record.
    #[default]
    Hard,
    /// Keep the record with a `deleted_at` timestamp and hide it.
    Soft,
}

impl Resource {
    /// An enabled resource with UUID keys, no seed data and default paging.
    pub fn new(name: impl Into<String>, path: PathTemplate) -> Self {
        Self {
            id: ResourceId::new(),
            name: name.into(),
            path,
            enabled: true,
            primary_key: id_field(),
            key_strategy: KeyStrategy::Uuid,
            schema: DataSchema::default(),
            seed: None,
            body_schema: None,
            pagination: Pagination::default(),
            searchable: Vec::new(),
            delete: DeleteBehavior::Hard,
        }
    }

    pub fn validate(&self) -> Result<(), Vec<FieldError>> {
        let mut e = Errors::default();
        if !is_identifier(&self.name) {
            e.push(
                "name",
                "use letters, digits and `_`, not starting with a digit",
            );
        }
        if self
            .path
            .segments()
            .iter()
            .any(|s| matches!(s, Segment::Param(_)))
        {
            e.push("path", "must not contain path parameters");
        }
        if self.path.segments().is_empty() {
            e.push("path", "must not be `/`");
        }
        if self.primary_key.trim().is_empty() {
            e.push("primary_key", "must not be empty");
        }
        if let Some(seed) = &self.seed
            && seed.count > MAX_RECORDS
        {
            e.push("seed.count", format!("must be at most {MAX_RECORDS}"));
        }
        let p = self.pagination;
        if p.default_limit == 0 || p.default_limit > p.max_limit || p.max_limit > MAX_PAGE_LIMIT {
            e.push(
                "pagination",
                format!("need 1 <= default_limit <= max_limit <= {MAX_PAGE_LIMIT}"),
            );
        }
        if self.searchable.iter().any(|s| s.trim().is_empty()) {
            e.push("searchable", "field names must not be empty");
        }
        if let Some(schema) = &self.body_schema
            && !(schema.is_object() || schema.is_boolean())
        {
            e.push("body_schema", "must be a JSON object or boolean");
        }
        let mut errors = e.finish().err().unwrap_or_default();
        if let Err(more) = self.schema.validate() {
            errors.extend(
                more.into_iter()
                    .map(|f| FieldError::new(format!("schema.{}", f.field), f.message)),
            );
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

/// One persisted record of a resource.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredRecord {
    /// The primary key as text (`"42"` for the number 42).
    pub key: String,
    /// Display order; new records get the next number.
    pub position: i64,
    pub data: Value,
}

/// A change to a resource's records, emitted by the runtime for persistence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum RecordChange {
    /// Insert or overwrite one record.
    Upsert {
        record: StoredRecord,
    },
    Delete {
        key: String,
    },
    /// Replace every record (seeding and reset). Marks the data as initialized.
    Replace {
        records: Vec<StoredRecord>,
    },
}

/// Text form of a primary-key value; `None` for values that cannot be keys.
#[must_use]
pub fn key_of(value: &Value) -> Option<String> {
    match value {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn minimal_json() {
        let r: Resource =
            serde_json::from_value(json!({ "name": "users", "path": "/users" })).unwrap();
        assert!(r.enabled);
        assert_eq!(r.primary_key, "id");
        assert_eq!(r.key_strategy, KeyStrategy::Uuid);
        assert_eq!(r.pagination, Pagination::default());
        assert!(r.validate().is_ok());
    }

    #[test]
    fn rejects_bad_config() {
        let mut r = Resource::new("2users", PathTemplate::parse("/users/:id").unwrap());
        r.pagination.default_limit = 500;
        r.seed = Some(SeedSpec {
            count: MAX_RECORDS + 1,
            seed: 0,
            locale: Locale::En,
        });
        let fields: Vec<_> = r
            .validate()
            .unwrap_err()
            .into_iter()
            .map(|e| e.field)
            .collect();
        assert_eq!(fields, ["name", "path", "seed.count", "pagination"]);
    }

    #[test]
    fn keys() {
        assert_eq!(key_of(&json!("a")), Some("a".into()));
        assert_eq!(key_of(&json!(42)), Some("42".into()));
        assert_eq!(key_of(&json!("")), None);
        assert_eq!(key_of(&json!(null)), None);
        assert_eq!(key_of(&json!({})), None);
    }
}
