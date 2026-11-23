//! Saved dataset definitions. Records are not stored: they are regenerated
//! deterministically from the schema, count, seed and locale.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::types::Json;
use uuid::Uuid;
use workbench_core::{DatasetId, ProjectId, time};
use workbench_domain::{DataSchema, Dataset, Locale};

use crate::error::map_constraint;
use crate::store::with_pool;
use crate::{StorageError, StorageResult, Store};

const ENTITY: &str = "dataset";
const COLUMNS: &str = "id, project_id, name, description, config, created_at, updated_at";
pub(crate) const INSERT_DATASET: &str = "INSERT INTO datasets (id, project_id, name, \
     description, config, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7)";

#[derive(Serialize, Deserialize)]
pub(crate) struct Config {
    pub schema: DataSchema,
    pub count: usize,
    pub seed: u64,
    pub locale: Locale,
}

impl Config {
    pub(crate) fn of(d: &Dataset) -> Json<Self> {
        Json(Self {
            schema: d.schema.clone(),
            count: d.count,
            seed: d.seed,
            locale: d.locale,
        })
    }
}

macro_rules! bind_dataset {
    ($sql:expr, $d:expr) => {
        sqlx::query($sql)
            .bind(*$d.id.as_uuid())
            .bind(*$d.project_id.as_uuid())
            .bind(&$d.name)
            .bind(&$d.description)
            .bind($crate::datasets::Config::of(&$d))
            .bind(workbench_core::time::truncate($d.created_at))
            .bind(workbench_core::time::truncate($d.updated_at))
    };
}
pub(crate) use bind_dataset;

#[derive(sqlx::FromRow)]
struct DatasetRow {
    id: Uuid,
    project_id: Uuid,
    name: String,
    description: String,
    config: Json<Config>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<DatasetRow> for Dataset {
    fn from(r: DatasetRow) -> Self {
        let c = r.config.0;
        Self {
            id: DatasetId::from_uuid(r.id),
            project_id: ProjectId::from_uuid(r.project_id),
            name: r.name,
            description: r.description,
            schema: c.schema,
            count: c.count,
            seed: c.seed,
            locale: c.locale,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

fn duplicate_name(d: &Dataset) -> StorageError {
    StorageError::Conflict(format!(
        "a dataset named `{}` already exists in this project",
        d.name
    ))
}

impl Store {
    pub async fn create_dataset(&self, d: &Dataset) -> StorageResult<()> {
        d.validate().map_err(|e| StorageError::invalid(ENTITY, e))?;
        with_pool!(self, |pool| bind_dataset!(INSERT_DATASET, d)
            .execute(pool)
            .await
            .map_err(|e| map_constraint(
                e,
                || duplicate_name(d),
                || StorageError::not_found("project", d.project_id),
            ))?
            .rows_affected());
        Ok(())
    }

    pub async fn get_dataset(&self, id: DatasetId) -> StorageResult<Dataset> {
        let sql = format!("SELECT {COLUMNS} FROM datasets WHERE id = $1");
        let row: Option<DatasetRow> = with_pool!(self, |pool| sqlx::query_as(&sql)
            .bind(*id.as_uuid())
            .fetch_optional(pool)
            .await?);
        row.map(Dataset::from)
            .ok_or_else(|| StorageError::not_found(ENTITY, id))
    }

    /// Datasets of a project, oldest first.
    pub async fn list_datasets(&self, project_id: ProjectId) -> StorageResult<Vec<Dataset>> {
        let sql =
            format!("SELECT {COLUMNS} FROM datasets WHERE project_id = $1 ORDER BY created_at, id");
        let rows: Vec<DatasetRow> = with_pool!(self, |pool| sqlx::query_as(&sql)
            .bind(*project_id.as_uuid())
            .fetch_all(pool)
            .await?);
        Ok(rows.into_iter().map(Dataset::from).collect())
    }

    pub async fn update_dataset(&self, d: &Dataset) -> StorageResult<Dataset> {
        d.validate().map_err(|e| StorageError::invalid(ENTITY, e))?;
        let affected = with_pool!(self, |pool| sqlx::query(
            "UPDATE datasets SET name = $2, description = $3, config = $4, updated_at = $5 \
             WHERE id = $1"
        )
        .bind(*d.id.as_uuid())
        .bind(&d.name)
        .bind(&d.description)
        .bind(Config::of(d))
        .bind(time::now())
        .execute(pool)
        .await
        .map_err(|e| map_constraint(
            e,
            || duplicate_name(d),
            || StorageError::not_found(ENTITY, d.id)
        ))?
        .rows_affected());
        if affected == 0 {
            return Err(StorageError::not_found(ENTITY, d.id));
        }
        self.get_dataset(d.id).await
    }

    pub async fn delete_dataset(&self, id: DatasetId) -> StorageResult<()> {
        let affected = with_pool!(self, |pool| sqlx::query(
            "DELETE FROM datasets WHERE id = $1"
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
}
