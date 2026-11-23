//! CRUD resource definitions and their records.
//!
//! The resource's name, path and enabled flag are columns; the rest of its
//! configuration (keys, schema, seed, validation, paging, search, delete
//! behaviour) is one JSON `config` value. Records live in
//! `resource_records`, one row per primary key.

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::types::Json;
use uuid::Uuid;
use workbench_core::{ProjectId, ResourceId, time};
use workbench_domain::{PathTemplate, RecordChange, Resource, StoredRecord};

use crate::error::map_constraint;
use crate::store::with_pool;
use crate::{StorageError, StorageResult, Store};

const ENTITY: &str = "resource";
const COLUMNS: &str = "id, project_id, name, path, enabled, config, data_initialized_at, \
                       created_at, updated_at";

pub(crate) const INSERT_RESOURCE: &str = "INSERT INTO resources (id, project_id, name, path, \
     enabled, config, data_initialized_at, created_at, updated_at) \
     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)";

pub(crate) const UPSERT_RECORD: &str = "INSERT INTO resource_records \
     (resource_id, record_key, position, data) VALUES ($1, $2, $3, $4) \
     ON CONFLICT (resource_id, record_key) \
     DO UPDATE SET position = excluded.position, data = excluded.data";

/// `bind_resource!(sql, project_id, resource, data_initialized_at, now)`
macro_rules! bind_resource {
    ($sql:expr, $project_id:expr, $r:expr, $initialized:expr, $now:expr) => {
        sqlx::query($sql)
            .bind(*$r.id.as_uuid())
            .bind(*$project_id.as_uuid())
            .bind(&$r.name)
            .bind($r.path.to_string())
            .bind($r.enabled)
            .bind(sqlx::types::Json(&$r))
            .bind($initialized)
            .bind($now)
            .bind($now)
    };
}
pub(crate) use bind_resource;

/// `bind_record!(resource_id, record)` for [`UPSERT_RECORD`].
macro_rules! bind_record {
    ($resource_id:expr, $rec:expr) => {
        sqlx::query($crate::resources::UPSERT_RECORD)
            .bind(*$resource_id.as_uuid())
            .bind(&$rec.key)
            .bind($rec.position)
            .bind(sqlx::types::Json(&$rec.data))
    };
}
pub(crate) use bind_record;

/// A resource with its storage metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct ResourceRecord {
    pub project_id: ProjectId,
    /// When records were first written; `None` means the runtime will seed.
    pub data_initialized_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub resource: Resource,
}

#[derive(sqlx::FromRow)]
struct ResourceRow {
    id: Uuid,
    project_id: Uuid,
    name: String,
    path: String,
    enabled: bool,
    config: Json<Resource>,
    data_initialized_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<ResourceRow> for ResourceRecord {
    type Error = StorageError;

    fn try_from(r: ResourceRow) -> Result<Self, Self::Error> {
        let path =
            PathTemplate::parse(&r.path).map_err(|e| StorageError::corrupt(ENTITY, r.id, e))?;
        let resource = Resource {
            id: ResourceId::from_uuid(r.id),
            name: r.name,
            path,
            enabled: r.enabled,
            ..r.config.0
        };
        Ok(Self {
            project_id: ProjectId::from_uuid(r.project_id),
            data_initialized_at: r.data_initialized_at,
            created_at: r.created_at,
            updated_at: r.updated_at,
            resource,
        })
    }
}

#[derive(sqlx::FromRow)]
struct RecordRow {
    record_key: String,
    position: i64,
    data: Json<Value>,
}

fn validate(r: &Resource) -> StorageResult<()> {
    r.validate().map_err(|e| StorageError::invalid(ENTITY, e))
}

fn duplicate_name(r: &Resource) -> StorageError {
    StorageError::Conflict(format!(
        "a resource named `{}` already exists in this project",
        r.name
    ))
}

impl Store {
    pub async fn create_resource(&self, project_id: ProjectId, r: &Resource) -> StorageResult<()> {
        validate(r)?;
        let now = time::now();
        let initialized: Option<DateTime<Utc>> = None;
        with_pool!(self, |pool| bind_resource!(
            INSERT_RESOURCE,
            project_id,
            r,
            initialized,
            now
        )
        .execute(pool)
        .await
        .map_err(|e| map_constraint(
            e,
            || duplicate_name(r),
            || StorageError::not_found("project", project_id),
        ))?
        .rows_affected());
        Ok(())
    }

    pub async fn get_resource(&self, id: ResourceId) -> StorageResult<ResourceRecord> {
        let sql = format!("SELECT {COLUMNS} FROM resources WHERE id = $1");
        let row: Option<ResourceRow> = with_pool!(self, |pool| sqlx::query_as(&sql)
            .bind(*id.as_uuid())
            .fetch_optional(pool)
            .await?);
        row.ok_or_else(|| StorageError::not_found(ENTITY, id))?
            .try_into()
    }

    /// Resources of a project, oldest first.
    pub async fn list_resources(&self, project_id: ProjectId) -> StorageResult<Vec<Resource>> {
        let sql = format!(
            "SELECT {COLUMNS} FROM resources WHERE project_id = $1 ORDER BY created_at, id"
        );
        let rows: Vec<ResourceRow> = with_pool!(self, |pool| sqlx::query_as(&sql)
            .bind(*project_id.as_uuid())
            .fetch_all(pool)
            .await?);
        rows.into_iter()
            .map(|r| ResourceRecord::try_from(r).map(|rec| rec.resource))
            .collect()
    }

    /// Saves the resource definition. Records are kept.
    pub async fn update_resource(&self, r: &Resource) -> StorageResult<ResourceRecord> {
        validate(r)?;
        let affected = with_pool!(self, |pool| sqlx::query(
            "UPDATE resources SET name = $2, path = $3, enabled = $4, config = $5, \
             updated_at = $6 WHERE id = $1"
        )
        .bind(*r.id.as_uuid())
        .bind(&r.name)
        .bind(r.path.to_string())
        .bind(r.enabled)
        .bind(Json(r))
        .bind(time::now())
        .execute(pool)
        .await
        .map_err(|e| map_constraint(
            e,
            || duplicate_name(r),
            || StorageError::not_found(ENTITY, r.id)
        ))?
        .rows_affected());
        if affected == 0 {
            return Err(StorageError::not_found(ENTITY, r.id));
        }
        self.get_resource(r.id).await
    }

    /// Deletes a resource and its records.
    pub async fn delete_resource(&self, id: ResourceId) -> StorageResult<()> {
        let affected = with_pool!(self, |pool| sqlx::query(
            "DELETE FROM resources WHERE id = $1"
        )
        .bind(*id.as_uuid())
        .execute(pool)
        .await?
        .rows_affected());
        if affected == 0 {
            return Err(StorageError::not_found(ENTITY, id));
        }
        Ok(())
    }

    /// The resource's records in display order, or `None` if they have
    /// never been written (the runtime then seeds the resource).
    pub async fn load_resource_records(
        &self,
        id: ResourceId,
    ) -> StorageResult<Option<Vec<StoredRecord>>> {
        let record = self.get_resource(id).await?;
        if record.data_initialized_at.is_none() {
            return Ok(None);
        }
        let rows: Vec<RecordRow> = with_pool!(self, |pool| sqlx::query_as(
            "SELECT record_key, position, data FROM resource_records \
             WHERE resource_id = $1 ORDER BY position, record_key"
        )
        .bind(*id.as_uuid())
        .fetch_all(pool)
        .await?);
        Ok(Some(
            rows.into_iter()
                .map(|r| StoredRecord {
                    key: r.record_key,
                    position: r.position,
                    data: r.data.0,
                })
                .collect(),
        ))
    }

    /// Applies one change emitted by the mock runtime.
    pub async fn apply_record_change(
        &self,
        id: ResourceId,
        change: &RecordChange,
    ) -> StorageResult<()> {
        let missing = || StorageError::not_found(ENTITY, id);
        match change {
            RecordChange::Upsert { record } => {
                with_pool!(self, |pool| bind_record!(id, record)
                    .execute(pool)
                    .await
                    .map_err(|e| map_constraint(e, missing, missing))?
                    .rows_affected());
            }
            RecordChange::Delete { key } => {
                with_pool!(self, |pool| sqlx::query(
                    "DELETE FROM resource_records WHERE resource_id = $1 AND record_key = $2"
                )
                .bind(*id.as_uuid())
                .bind(key)
                .execute(pool)
                .await?
                .rows_affected());
            }
            RecordChange::Replace { records } => {
                let affected = with_pool!(self, |pool| {
                    let mut tx = pool.begin().await?;
                    sqlx::query("DELETE FROM resource_records WHERE resource_id = $1")
                        .bind(*id.as_uuid())
                        .execute(&mut *tx)
                        .await?;
                    for record in records {
                        bind_record!(id, record).execute(&mut *tx).await?;
                    }
                    let affected =
                        sqlx::query("UPDATE resources SET data_initialized_at = $2 WHERE id = $1")
                            .bind(*id.as_uuid())
                            .bind(time::now())
                            .execute(&mut *tx)
                            .await?
                            .rows_affected();
                    tx.commit().await?;
                    affected
                });
                if affected == 0 {
                    return Err(missing());
                }
            }
        }
        Ok(())
    }

    /// Deletes every record and marks the resource uninitialized, so it is
    /// seeded again the next time a server starts.
    pub async fn reset_resource_records(&self, id: ResourceId) -> StorageResult<()> {
        let affected = with_pool!(self, |pool| {
            let mut tx = pool.begin().await?;
            sqlx::query("DELETE FROM resource_records WHERE resource_id = $1")
                .bind(*id.as_uuid())
                .execute(&mut *tx)
                .await?;
            let affected =
                sqlx::query("UPDATE resources SET data_initialized_at = NULL WHERE id = $1")
                    .bind(*id.as_uuid())
                    .execute(&mut *tx)
                    .await?
                    .rows_affected();
            tx.commit().await?;
            affected
        });
        if affected == 0 {
            return Err(StorageError::not_found(ENTITY, id));
        }
        Ok(())
    }
}
