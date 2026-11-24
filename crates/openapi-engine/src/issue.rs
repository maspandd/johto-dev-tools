//! Problems found while reading or mapping a document.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Something was not imported.
    Error,
    /// Something was imported only in part, or approximated.
    Warning,
    /// Worth knowing; nothing was lost.
    Info,
}

/// One problem, located by a JSON pointer into the source document
/// (for example `/paths/~1users/get/requestBody`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Issue {
    pub severity: Severity,
    pub pointer: String,
    pub message: String,
}

impl Issue {
    pub fn error(pointer: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(Severity::Error, pointer, message)
    }

    pub fn warning(pointer: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(Severity::Warning, pointer, message)
    }

    pub fn info(pointer: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(Severity::Info, pointer, message)
    }

    fn new(severity: Severity, pointer: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity,
            pointer: pointer.into(),
            message: message.into(),
        }
    }
}

/// Escapes one JSON pointer token (RFC 6901).
pub(crate) fn escape(token: &str) -> String {
    token.replace('~', "~0").replace('/', "~1")
}

/// Appends an escaped token to a pointer.
pub(crate) fn join(pointer: &str, token: &str) -> String {
    format!("{pointer}/{}", escape(token))
}
