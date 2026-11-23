//! Field-level validation shared by workspaces, projects and environments.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::{PathTemplate, Segment};

/// Longest accepted display name.
pub const MAX_NAME_LEN: usize = 200;

/// One invalid field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldError {
    pub field: String,
    pub message: String,
}

impl FieldError {
    #[must_use]
    pub fn new(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for FieldError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.field, self.message)
    }
}

/// Collects errors and turns them into a `Result`.
#[derive(Debug, Default)]
pub(crate) struct Errors(Vec<FieldError>);

impl Errors {
    pub(crate) fn push(&mut self, field: impl Into<String>, message: impl Into<String>) {
        self.0.push(FieldError::new(field, message));
    }

    pub(crate) fn name(&mut self, field: &str, value: &str) {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            self.push(field, "must not be empty");
        } else if trimmed.chars().count() > MAX_NAME_LEN {
            self.push(field, format!("must be at most {MAX_NAME_LEN} characters"));
        }
    }

    pub(crate) fn headers(&mut self, field: &str, headers: &BTreeMap<String, String>) {
        for name in headers.keys() {
            if !is_valid_header_name(name) {
                self.push(format!("{field}.{name}"), "is not a valid header name");
            }
        }
    }

    /// A static path prefix such as `/api`; parameters are not allowed.
    pub(crate) fn base_path(&mut self, field: &str, value: Option<&str>) {
        let Some(value) = value else { return };
        match PathTemplate::parse(value) {
            Ok(t) if t.segments().iter().any(|s| matches!(s, Segment::Param(_))) => {
                self.push(field, "must not contain path parameters");
            }
            Ok(_) => {}
            Err(e) => self.push(field, e.to_string()),
        }
    }

    pub(crate) fn finish(self) -> Result<(), Vec<FieldError>> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(self.0)
        }
    }
}

/// RFC 9110 token characters.
pub(crate) fn is_valid_header_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
}

/// Letters, digits and `_`, not starting with a digit.
pub(crate) fn is_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}
