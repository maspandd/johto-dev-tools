//! Encrypted secret variables of an environment.
//!
//! Storage never sees plaintext: callers encrypt before `put_secret` and
//! decrypt after `get_secret`. Secrets live in their own table, so they are
//! never part of an environment's normal JSON or exports, and they are not
//! copied when a project is duplicated (ciphertext is bound to the
//! environment it was written for).

use chrono::{DateTime, Utc};
use workbench_core::{EnvironmentId, time};
use workbench_domain::FieldError;

use crate::error::map_constraint;
use crate::store::with_pool;
use crate::{StorageError, StorageResult, Store};

/// Name and timestamps of a secret; never its value.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SecretMeta {
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// An encrypted secret value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncryptedSecret {
    pub nonce: Vec<u8>,
    pub ciphertext: Vec<u8>,
}

#[derive(sqlx::FromRow)]
struct MetaRow {
    name: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct ValueRow {
    nonce: Vec<u8>,
    ciphertext: Vec<u8>,
}

fn check_name(name: &str) -> StorageResult<()> {
    let mut chars = name.chars();
    let ok = matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        && name.len() <= 200;
    if ok {
        Ok(())
    } else {
        Err(StorageError::invalid(
            "secret",
            vec![FieldError::new(
                "name",
                "use letters, digits and `_`, not starting with a digit",
            )],
        ))
    }
}

impl Store {
    /// Creates or replaces a secret.
    pub async fn put_secret(
        &self,
        environment_id: EnvironmentId,
        name: &str,
        secret: &EncryptedSecret,
    ) -> StorageResult<()> {
        check_name(name)?;
        let now = time::now();
        with_pool!(self, |pool| sqlx::query(
            "INSERT INTO environment_secrets (environment_id, name, nonce, ciphertext, \
             created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $5) \
             ON CONFLICT (environment_id, name) DO UPDATE SET nonce = excluded.nonce, \
             ciphertext = excluded.ciphertext, updated_at = excluded.updated_at"
        )
        .bind(*environment_id.as_uuid())
        .bind(name)
        .bind(&secret.nonce)
        .bind(&secret.ciphertext)
        .bind(now)
        .execute(pool)
        .await
        .map_err(|e| {
            let missing = || StorageError::not_found("environment", environment_id);
            map_constraint(e, missing, missing)
        })?
        .rows_affected());
        Ok(())
    }

    /// Secret names of an environment, alphabetically.
    pub async fn list_secrets(
        &self,
        environment_id: EnvironmentId,
    ) -> StorageResult<Vec<SecretMeta>> {
        let rows: Vec<MetaRow> = with_pool!(self, |pool| sqlx::query_as(
            "SELECT name, created_at, updated_at FROM environment_secrets \
             WHERE environment_id = $1 ORDER BY name"
        )
        .bind(*environment_id.as_uuid())
        .fetch_all(pool)
        .await?);
        Ok(rows
            .into_iter()
            .map(|r| SecretMeta {
                name: r.name,
                created_at: r.created_at,
                updated_at: r.updated_at,
            })
            .collect())
    }

    pub async fn get_secret(
        &self,
        environment_id: EnvironmentId,
        name: &str,
    ) -> StorageResult<EncryptedSecret> {
        let row: Option<ValueRow> = with_pool!(self, |pool| sqlx::query_as(
            "SELECT nonce, ciphertext FROM environment_secrets \
             WHERE environment_id = $1 AND name = $2"
        )
        .bind(*environment_id.as_uuid())
        .bind(name)
        .fetch_optional(pool)
        .await?);
        row.map(|r| EncryptedSecret {
            nonce: r.nonce,
            ciphertext: r.ciphertext,
        })
        .ok_or_else(|| StorageError::not_found("secret", name))
    }

    pub async fn delete_secret(
        &self,
        environment_id: EnvironmentId,
        name: &str,
    ) -> StorageResult<()> {
        let affected = with_pool!(self, |pool| sqlx::query(
            "DELETE FROM environment_secrets WHERE environment_id = $1 AND name = $2"
        )
        .bind(*environment_id.as_uuid())
        .bind(name)
        .execute(pool)
        .await?
        .rows_affected());
        if affected == 0 {
            return Err(StorageError::not_found("secret", name));
        }
        Ok(())
    }
}
