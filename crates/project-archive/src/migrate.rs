//! Upgrading archives written by older versions.
//!
//! Migrations run on the raw JSON files, before they are read into typed
//! values, so they can rename or reshape anything. When
//! [`FORMAT_VERSION`](crate::FORMAT_VERSION) is bumped, add one function
//! here that upgrades the previous version.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::{ArchiveError, FORMAT_VERSION};

/// The archive's JSON files by path relative to the project folder.
pub(crate) type Files = BTreeMap<String, Value>;

type Migration = fn(&mut Files) -> Result<(), String>;

/// `MIGRATIONS[i]` upgrades format `i + 1` to format `i + 2`.
const MIGRATIONS: &[Migration] = &[];

/// Brings `files` from `version` up to the current format.
pub(crate) fn migrate(version: u32, files: &mut Files) -> Result<(), ArchiveError> {
    if version == 0 {
        return Err(ArchiveError::UnsupportedFormat(version));
    }
    if version > FORMAT_VERSION {
        return Err(ArchiveError::NewerFormat {
            found: version,
            supported: FORMAT_VERSION,
        });
    }
    for (step, migration) in MIGRATIONS
        .iter()
        .enumerate()
        .skip(usize::try_from(version - 1).unwrap_or(usize::MAX))
    {
        migration(files).map_err(|message| ArchiveError::InvalidFile {
            file: "manifest.json".to_owned(),
            message: format!("upgrading from format {}: {message}", step + 1),
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_migration_per_older_version() {
        assert_eq!(
            MIGRATIONS.len(),
            usize::try_from(FORMAT_VERSION - 1).unwrap()
        );
    }

    #[test]
    fn rejects_unknown_versions() {
        let mut files = Files::new();
        assert!(migrate(FORMAT_VERSION, &mut files).is_ok());
        assert!(matches!(
            migrate(FORMAT_VERSION + 1, &mut files),
            Err(ArchiveError::NewerFormat { .. })
        ));
        assert!(matches!(
            migrate(0, &mut files),
            Err(ArchiveError::UnsupportedFormat(0))
        ));
    }
}
