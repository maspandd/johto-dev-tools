use std::fmt::Display;

use thiserror::Error;
use workbench_domain::FieldError;

pub type StorageResult<T> = Result<T, StorageError>;

fn join<T: Display>(items: &[T]) -> String {
    items
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("{entity} {id} was not found")]
    NotFound { entity: &'static str, id: String },
    #[error("invalid {entity}: {}", join(.errors))]
    Invalid {
        entity: &'static str,
        errors: Vec<FieldError>,
    },
    #[error("{0}")]
    Conflict(String),
    #[error("stored {entity} {id} cannot be read: {reason}")]
    Corrupt {
        entity: &'static str,
        id: String,
        reason: String,
    },
    #[error("unsupported database URL; expected `sqlite:` or `postgres://`")]
    UnsupportedUrl,
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("migration failed: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),
}

impl StorageError {
    pub(crate) fn not_found(entity: &'static str, id: impl Display) -> Self {
        Self::NotFound {
            entity,
            id: id.to_string(),
        }
    }

    pub(crate) fn corrupt(entity: &'static str, id: impl Display, reason: impl Display) -> Self {
        Self::Corrupt {
            entity,
            id: id.to_string(),
            reason: reason.to_string(),
        }
    }

    pub(crate) fn invalid(entity: &'static str, errors: Vec<FieldError>) -> Self {
        Self::Invalid { entity, errors }
    }
}

/// Maps constraint violations to domain errors; other errors pass through.
pub(crate) fn map_constraint(
    err: sqlx::Error,
    on_unique: impl FnOnce() -> StorageError,
    on_foreign_key: impl FnOnce() -> StorageError,
) -> StorageError {
    if let sqlx::Error::Database(db) = &err {
        if db.is_unique_violation() {
            return on_unique();
        }
        if db.is_foreign_key_violation() {
            return on_foreign_key();
        }
    }
    err.into()
}
