//! Portable project archives (spec §4.10).
//!
//! A `.zip` holding one folder of plain JSON files, so archives diff well
//! and can be inspected by hand:
//!
//! ```text
//! workbench-project/
//! ├── manifest.json            format, version, generator, contents
//! ├── project.json             name, description, base path, headers
//! ├── environments/01-development.json
//! ├── endpoints/001-list-users.json      one per endpoint, in order
//! ├── resources/users.json               CRUD resource definition
//! ├── resources/users.records.json       records (only when exported)
//! ├── datasets/01-customers.json
//! └── openapi/openapi.yaml               generated; ignored on import
//! ```
//!
//! Secret values are never written; environments list secret *names* so
//! they can be filled in after import. IDs are not kept: an import always
//! creates new ones.
//!
//! Reading is defensive: entry names are validated, sizes and entry counts
//! are capped (also against decompression bombs), unknown files are
//! reported, every item is validated, and older format versions are
//! migrated forward.

mod migrate;
mod read;
mod write;

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use workbench_core::BindAddress;
use workbench_domain::{DataSchema, Endpoint, Locale, Resource};

pub use read::{Limits, ReadArchive, read};
pub use workbench_domain::PROJECT_FORMAT_VERSION as FORMAT_VERSION;
pub use write::write;

/// `manifest.json`'s `format` value.
pub const FORMAT: &str = "workbench-project";
/// Name of the folder inside the zip.
pub const ROOT_DIR: &str = "workbench-project";

#[derive(Debug, Error)]
pub enum ArchiveError {
    #[error("not a zip archive: {0}")]
    NotZip(String),
    #[error("the archive has no manifest.json; it is not a Developer Workbench project")]
    MissingManifest,
    #[error("not a Developer Workbench project archive (format `{0}`)")]
    WrongFormat(String),
    #[error(
        "the archive was made by a newer version (format {found}); this build reads up to {supported}"
    )]
    NewerFormat { found: u32, supported: u32 },
    #[error("format version {0} is not supported")]
    UnsupportedFormat(u32),
    #[error("unsafe entry name `{0}`")]
    UnsafePath(String),
    #[error("entry `{0}` is encrypted; encrypted archives are not supported")]
    Encrypted(String),
    #[error("entry `{name}` uses an unsupported compression method")]
    UnsupportedCompression { name: String },
    #[error("the archive has more than {max} entries")]
    TooManyEntries { max: usize },
    #[error("entry `{name}` is larger than {max} bytes")]
    EntryTooLarge { name: String, max: u64 },
    #[error("the archive expands to more than {max} bytes")]
    TooLarge { max: u64 },
    #[error("`{file}`: {message}")]
    InvalidFile { file: String, message: String },
    #[error("the archive contents are invalid")]
    Invalid(Vec<String>),
    #[error("cannot write archive: {0}")]
    Write(String),
}

/// `manifest.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub format: String,
    pub format_version: u32,
    /// The app that wrote the archive, e.g. `developer-workbench 0.1.0`.
    pub generator: String,
    pub exported_at: DateTime<Utc>,
    pub project_name: String,
    pub contents: Contents,
    /// Always `false`: secret values are never exported.
    #[serde(default)]
    pub secrets_included: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contents {
    pub environments: usize,
    pub endpoints: usize,
    pub resources: usize,
    pub datasets: usize,
    /// Resource records across all resources.
    pub records: usize,
}

/// `project.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectFile {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub base_path: Option<String>,
    #[serde(default)]
    pub default_headers: BTreeMap<String, String>,
}

/// One file in `environments/`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvironmentFile {
    pub name: String,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub default_headers: BTreeMap<String, String>,
    #[serde(default)]
    pub variables: BTreeMap<String, String>,
    #[serde(default)]
    pub bind: BindAddress,
    /// Secrets the environment had. Their values are not exported.
    #[serde(default)]
    pub secret_names: Vec<String>,
}

/// One file in `datasets/`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DatasetFile {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub schema: DataSchema,
    pub count: usize,
    #[serde(default)]
    pub seed: u64,
    #[serde(default)]
    pub locale: Locale,
}

/// A resource and, optionally, its records in display order.
#[derive(Debug, Clone, PartialEq)]
pub struct ResourceEntry {
    pub resource: Resource,
    /// `None`: the resource is seeded again when a server starts.
    pub records: Option<Vec<Value>>,
}

/// Everything an archive holds, without IDs.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectBundle {
    pub project: ProjectFile,
    pub environments: Vec<EnvironmentFile>,
    /// In display order.
    pub endpoints: Vec<Endpoint>,
    pub resources: Vec<ResourceEntry>,
    pub datasets: Vec<DatasetFile>,
}

impl ProjectBundle {
    #[must_use]
    pub fn contents(&self) -> Contents {
        Contents {
            environments: self.environments.len(),
            endpoints: self.endpoints.len(),
            resources: self.resources.len(),
            datasets: self.datasets.len(),
            records: self
                .resources
                .iter()
                .filter_map(|r| r.records.as_ref())
                .map(Vec::len)
                .sum(),
        }
    }
}

/// Lowercase ASCII letters, digits and `-`, at most 48 characters.
pub(crate) fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
        if out.len() >= 48 {
            break;
        }
    }
    let out = out.trim_end_matches('-');
    if out.is_empty() {
        "item".to_owned()
    } else {
        out.to_owned()
    }
}
