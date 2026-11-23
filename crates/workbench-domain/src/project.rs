//! API projects (spec §4.1).

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use workbench_core::{ProjectId, WorkspaceId, time};

use crate::validation::{Errors, FieldError};

/// Version of the project data format. Bump it when stored or exported
/// project data changes shape, and add a migration.
pub const PROJECT_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub id: ProjectId,
    pub workspace_id: WorkspaceId,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Static prefix for every mock route, e.g. `/api`.
    #[serde(default)]
    pub base_path: Option<String>,
    /// Headers added to every mock response.
    #[serde(default)]
    pub default_headers: BTreeMap<String, String>,
    pub format_version: u32,
    pub archived_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Project {
    #[must_use]
    pub fn new(workspace_id: WorkspaceId, name: impl Into<String>) -> Self {
        let now = time::now();
        Self {
            id: ProjectId::new(),
            workspace_id,
            name: name.into().trim().to_owned(),
            description: String::new(),
            base_path: None,
            default_headers: BTreeMap::new(),
            format_version: PROJECT_FORMAT_VERSION,
            archived_at: None,
            created_at: now,
            updated_at: now,
        }
    }

    #[must_use]
    pub const fn is_archived(&self) -> bool {
        self.archived_at.is_some()
    }

    pub fn validate(&self) -> Result<(), Vec<FieldError>> {
        let mut e = Errors::default();
        e.name("name", &self.name);
        e.base_path("base_path", self.base_path.as_deref());
        e.headers("default_headers", &self.default_headers);
        if self.format_version == 0 || self.format_version > PROJECT_FORMAT_VERSION {
            e.push(
                "format_version",
                format!(
                    "unsupported version {} (this build supports up to {PROJECT_FORMAT_VERSION})",
                    self.format_version
                ),
            );
        }
        e.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_project_is_valid() {
        let p = Project::new(WorkspaceId::new(), "Shop API");
        assert_eq!(p.format_version, PROJECT_FORMAT_VERSION);
        assert!(p.validate().is_ok());
    }

    #[test]
    fn reports_every_invalid_field() {
        let mut p = Project::new(WorkspaceId::new(), "");
        p.base_path = Some("/api/:v".into());
        p.default_headers.insert("bad header".into(), "x".into());
        p.format_version = 99;
        let fields: Vec<_> = p
            .validate()
            .unwrap_err()
            .into_iter()
            .map(|e| e.field)
            .collect();
        assert_eq!(
            fields,
            [
                "name",
                "base_path",
                "default_headers.bad header",
                "format_version"
            ]
        );
    }
}
