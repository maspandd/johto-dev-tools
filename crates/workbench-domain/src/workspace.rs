//! Workspaces group projects (spec §4.1).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use workbench_core::{WorkspaceId, time};

use crate::validation::{Errors, FieldError};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    pub id: WorkspaceId,
    pub name: String,
    /// Set while the workspace is archived.
    pub archived_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Workspace {
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        let now = time::now();
        Self {
            id: WorkspaceId::new(),
            name: name.into().trim().to_owned(),
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
        e.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_and_validates_name() {
        assert_eq!(Workspace::new("  Team  ").name, "Team");
        assert!(Workspace::new(" ").validate().is_err());
        assert!(Workspace::new("x".repeat(201)).validate().is_err());
        assert!(Workspace::new("Team").validate().is_ok());
    }
}
