//! API definition import and export (spec §4.7, §4.8).
//!
//! - [`analyze`] reads an OpenAPI 3.0, 3.1 or 3.2 document, reports every
//!   problem, and maps each operation to an editable [`Endpoint`] draft.
//! - [`export`] turns a project's endpoints and CRUD resources into an
//!   OpenAPI 3.1 document.
//! - [`curl`] parses a cURL command into a request preview and an
//!   endpoint draft. Nothing is ever executed.
//!
//! The crate is pure: no I/O, no network, no database. Callers decide what
//! to persist.
//!
//! [`Endpoint`]: workbench_domain::Endpoint

pub mod curl;
mod document;
mod export;
mod import;
mod issue;
mod schema;

pub use document::{MAX_DOCUMENT_BYTES, ParseError, parse_document, to_json, to_yaml};
pub use export::{ApiDefinition, export};
pub use import::{Analysis, Operation, analyze, operation_key};
pub use issue::{Issue, Severity};
