//! Synthetic data definitions (spec §4.4).
//!
//! A [`DataSchema`] describes the shape of generated records; generation
//! itself lives in the `data-generator` crate. Generated data is always
//! synthetic: names, addresses and phone numbers come from built-in word
//! lists, never from real personal records.

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use workbench_core::{DatasetId, ProjectId, time};

use crate::validation::{Errors, FieldError};

/// Most records a single generation may produce.
pub const MAX_RECORDS: usize = 10_000;
/// Longest array a field may generate.
pub const MAX_ARRAY_ITEMS: usize = 1_000;

/// Word lists and formats used for names, phones and addresses.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Locale {
    #[default]
    En,
    /// Indonesian names, `+62` mobile numbers and `Jl.` street addresses.
    Id,
}

/// The fields of one generated record.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DataSchema {
    pub fields: Vec<FieldSpec>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldSpec {
    pub name: String,
    #[serde(flatten)]
    pub kind: FieldKind,
    /// Probability (0–1) that the value is `null`.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub null_rate: f64,
    /// No two records share a value for this field (nulls excepted).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unique: bool,
}

#[allow(clippy::trivially_copy_pass_by_ref)] // serde passes a reference
fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

impl FieldSpec {
    #[must_use]
    pub fn new(name: impl Into<String>, kind: FieldKind) -> Self {
        Self {
            name: name.into(),
            kind,
            null_rate: 0.0,
            unique: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FieldKind {
    Uuid,
    /// `start`, `start + step`, … by record index.
    Sequence {
        #[serde(default = "one")]
        start: i64,
        #[serde(default = "one")]
        step: i64,
    },
    Integer {
        min: i64,
        max: i64,
    },
    Decimal {
        min: f64,
        max: f64,
        /// Digits after the decimal point.
        #[serde(default = "two")]
        scale: u32,
    },
    Boolean,
    FirstName,
    LastName,
    FullName,
    Email,
    Phone,
    Address,
    City,
    Company,
    /// Random words, e.g. for titles and descriptions.
    Words {
        #[serde(default = "one_usize")]
        min: usize,
        #[serde(default = "three")]
        max: usize,
    },
    /// `YYYY-MM-DD` between two dates, inclusive.
    Date {
        from: NaiveDate,
        to: NaiveDate,
    },
    /// RFC 3339 timestamp between two instants, inclusive.
    Timestamp {
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    },
    /// One of the given values.
    Enum {
        values: Vec<Value>,
    },
    Constant {
        value: Value,
    },
    /// Text where `#` becomes a digit, `?` an uppercase letter, and
    /// `{{field}}` the value of a sibling field defined earlier.
    Template {
        template: String,
    },
    Object {
        fields: Vec<FieldSpec>,
    },
    Array {
        items: Box<FieldKind>,
        #[serde(default)]
        min_items: usize,
        #[serde(default = "three")]
        max_items: usize,
    },
    /// A value copied from a record of another resource, e.g. `users.id`.
    Reference {
        resource: String,
        #[serde(default = "id_field")]
        field: String,
    },
}

const fn one() -> i64 {
    1
}
const fn one_usize() -> usize {
    1
}
const fn two() -> u32 {
    2
}
const fn three() -> usize {
    3
}
fn id_field() -> String {
    "id".to_owned()
}

impl DataSchema {
    #[must_use]
    pub fn new(fields: Vec<FieldSpec>) -> Self {
        Self { fields }
    }

    /// Resources this schema references, in field order, without duplicates.
    #[must_use]
    pub fn referenced_resources(&self) -> Vec<&str> {
        fn walk<'a>(kind: &'a FieldKind, out: &mut Vec<&'a str>) {
            match kind {
                FieldKind::Reference { resource, .. } => {
                    if !out.contains(&resource.as_str()) {
                        out.push(resource);
                    }
                }
                FieldKind::Object { fields } => fields.iter().for_each(|f| walk(&f.kind, out)),
                FieldKind::Array { items, .. } => walk(items, out),
                _ => {}
            }
        }
        let mut out = Vec::new();
        for f in &self.fields {
            walk(&f.kind, &mut out);
        }
        out
    }

    pub fn validate(&self) -> Result<(), Vec<FieldError>> {
        let mut e = Errors::default();
        validate_fields(&self.fields, "fields", &mut e);
        e.finish()
    }
}

fn validate_fields(fields: &[FieldSpec], at: &str, e: &mut Errors) {
    let mut seen = std::collections::HashSet::new();
    for f in fields {
        let here = format!("{at}.{}", if f.name.is_empty() { "?" } else { &f.name });
        if f.name.trim().is_empty() {
            e.push(&here, "field name must not be empty");
        } else if !seen.insert(f.name.as_str()) {
            e.push(&here, "duplicate field name");
        }
        if !(0.0..=1.0).contains(&f.null_rate) {
            e.push(&here, "null_rate must be between 0 and 1");
        }
        validate_kind(&f.kind, &here, e);
    }
}

fn validate_kind(kind: &FieldKind, at: &str, e: &mut Errors) {
    match kind {
        FieldKind::Integer { min, max } if min > max => e.push(at, "min must not exceed max"),
        FieldKind::Decimal { min, max, scale } => {
            if !min.is_finite() || !max.is_finite() || min > max {
                e.push(at, "min and max must be finite with min <= max");
            }
            if *scale > 10 {
                e.push(at, "scale must be at most 10");
            }
        }
        FieldKind::Words { min, max } if *min == 0 || min > max || *max > MAX_ARRAY_ITEMS => {
            e.push(at, format!("need 1 <= min <= max <= {MAX_ARRAY_ITEMS}"));
        }
        FieldKind::Date { from, to } if from > to => e.push(at, "from must not be after to"),
        FieldKind::Timestamp { from, to } if from > to => e.push(at, "from must not be after to"),
        FieldKind::Enum { values } if values.is_empty() => {
            e.push(at, "enum needs at least one value")
        }
        FieldKind::Template { template } if template.is_empty() => {
            e.push(at, "template must not be empty");
        }
        FieldKind::Reference { resource, field } if resource.is_empty() || field.is_empty() => {
            e.push(at, "reference needs a resource and a field");
        }
        FieldKind::Object { fields } => validate_fields(fields, at, e),
        FieldKind::Array {
            items,
            min_items,
            max_items,
        } => {
            if min_items > max_items || *max_items > MAX_ARRAY_ITEMS {
                e.push(
                    at,
                    format!("need min_items <= max_items <= {MAX_ARRAY_ITEMS}"),
                );
            }
            validate_kind(items, &format!("{at}[]"), e);
        }
        _ => {}
    }
}

/// A saved, reusable generator configuration. Records are regenerated on
/// demand; the same schema, count, seed and locale always give the same data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Dataset {
    pub id: DatasetId,
    pub project_id: ProjectId,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub schema: DataSchema,
    pub count: usize,
    #[serde(default)]
    pub seed: u64,
    #[serde(default)]
    pub locale: Locale,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Dataset {
    #[must_use]
    pub fn new(
        project_id: ProjectId,
        name: impl Into<String>,
        schema: DataSchema,
        count: usize,
    ) -> Self {
        let now = time::now();
        Self {
            id: DatasetId::new(),
            project_id,
            name: name.into().trim().to_owned(),
            description: String::new(),
            schema,
            count,
            seed: 0,
            locale: Locale::En,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn validate(&self) -> Result<(), Vec<FieldError>> {
        let mut e = Errors::default();
        e.name("name", &self.name);
        if self.count == 0 || self.count > MAX_RECORDS {
            e.push("count", format!("must be between 1 and {MAX_RECORDS}"));
        }
        let mut errors = e.finish().err().unwrap_or_default();
        if let Err(more) = self.schema.validate() {
            errors.extend(more);
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn field_json_shape() {
        let schema: DataSchema = serde_json::from_value(json!({
            "fields": [
                { "name": "id", "type": "uuid" },
                { "name": "age", "type": "integer", "min": 18, "max": 90, "null_rate": 0.1 },
                { "name": "email", "type": "email", "unique": true },
                { "name": "tags", "type": "array", "items": { "type": "words", "min": 1, "max": 1 }, "max_items": 2 },
                { "name": "owner_id", "type": "reference", "resource": "users" },
                { "name": "address", "type": "object", "fields": [{ "name": "city", "type": "city" }] }
            ]
        }))
        .unwrap();
        assert_eq!(schema.fields.len(), 6);
        assert!(schema.fields[2].unique);
        assert_eq!(schema.referenced_resources(), ["users"]);
        assert!(schema.validate().is_ok());
        let back = serde_json::to_value(&schema).unwrap();
        assert_eq!(back["fields"][4]["field"], "id");
        assert!(
            back["fields"][0].get("unique").is_none(),
            "defaults are omitted"
        );
    }

    #[test]
    fn validation_reports_paths() {
        let schema = DataSchema::new(vec![
            FieldSpec::new("n", FieldKind::Integer { min: 5, max: 1 }),
            FieldSpec::new("n", FieldKind::Boolean),
            FieldSpec {
                null_rate: 2.0,
                ..FieldSpec::new("e", FieldKind::Enum { values: vec![] })
            },
            FieldSpec::new(
                "list",
                FieldKind::Array {
                    items: Box::new(FieldKind::Integer { min: 2, max: 1 }),
                    min_items: 0,
                    max_items: 3,
                },
            ),
        ]);
        let fields: Vec<_> = schema
            .validate()
            .unwrap_err()
            .into_iter()
            .map(|e| e.field)
            .collect();
        assert_eq!(
            fields,
            [
                "fields.n",
                "fields.n",
                "fields.e",
                "fields.e",
                "fields.list[]"
            ]
        );
    }

    #[test]
    fn dataset_validation() {
        let d = Dataset::new(ProjectId::new(), "Users", DataSchema::default(), 0);
        assert_eq!(d.validate().unwrap_err()[0].field, "count");
    }
}
