//! Deterministic synthetic data generation (spec §4.4).
//!
//! The same schema, count, seed, locale and references always produce the
//! same records (with the same crate version), so datasets can be stored as
//! configuration and regenerated on demand. Records are generated in order
//! from one ChaCha stream, so a preview of the first N records equals the
//! first N records of the full dataset.

mod csv;
mod generator;
mod words;

use std::collections::HashMap;

use serde_json::Value;
use thiserror::Error;
use workbench_domain::{DataSchema, FieldError, Locale, MAX_RECORDS};

pub use csv::to_csv;

/// Records shown by [`preview`].
pub const PREVIEW_LIMIT: usize = 20;

/// Existing records of other resources, used by `reference` fields.
#[derive(Debug, Default, Clone)]
pub struct References<'a> {
    by_resource: HashMap<&'a str, &'a [Value]>,
}

impl<'a> References<'a> {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with(mut self, resource: &'a str, records: &'a [Value]) -> Self {
        self.by_resource.insert(resource, records);
        self
    }

    pub fn insert(&mut self, resource: &'a str, records: &'a [Value]) {
        self.by_resource.insert(resource, records);
    }

    pub(crate) fn get(&self, resource: &str) -> Option<&'a [Value]> {
        self.by_resource.get(resource).copied()
    }
}

#[derive(Debug, Error, PartialEq)]
pub enum GenerateError {
    #[error("invalid schema: {}", .0.iter().map(ToString::to_string).collect::<Vec<_>>().join("; "))]
    InvalidSchema(Vec<FieldError>),
    #[error("cannot generate {requested} records; the limit is {max}")]
    TooMany { requested: usize, max: usize },
    #[error("field `{field}` is unique but ran out of distinct values")]
    UniqueExhausted { field: String },
    #[error("field `{field}` references unknown resource `{resource}`")]
    UnknownReference { field: String, resource: String },
    #[error("field `{field}` references `{resource}`, which has no records")]
    EmptyReference { field: String, resource: String },
}

/// Generates `count` records.
pub fn generate(
    schema: &DataSchema,
    count: usize,
    seed: u64,
    locale: Locale,
    refs: &References<'_>,
) -> Result<Vec<Value>, GenerateError> {
    schema.validate().map_err(GenerateError::InvalidSchema)?;
    if count > MAX_RECORDS {
        return Err(GenerateError::TooMany {
            requested: count,
            max: MAX_RECORDS,
        });
    }
    let mut g = generator::Generator::new(seed, locale, refs);
    (0..count).map(|i| g.record(schema, i)).collect()
}

/// The first [`PREVIEW_LIMIT`] records of what [`generate`] would produce.
pub fn preview(
    schema: &DataSchema,
    count: usize,
    seed: u64,
    locale: Locale,
    refs: &References<'_>,
) -> Result<Vec<Value>, GenerateError> {
    generate(schema, count.min(PREVIEW_LIMIT), seed, locale, refs)
}
