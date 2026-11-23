//! Environment repository.

use std::collections::BTreeMap;
use std::net::IpAddr;

use chrono::{DateTime, Utc};
use sqlx::types::Json;
use uuid::Uuid;
use workbench_core::{BindAddress, EnvironmentId, ProjectId, time};
use workbench_domain::Environment;

use crate::error::map_constraint;
use crate::store::with_pool;
use crate::{StorageError, StorageResult, Store};

const ENTITY: &str = "environment";
const COLUMNS: &str = "id, project_id, name, base_url, default_headers, variables, \
                       bind_host, bind_port, allow_network_exposure, created_at, updated_at";
pub(crate) const INSERT_ENVIRONMENT: &str = "INSERT INTO environments (id, project_id, name, \
     base_url, default_headers, variables, bind_host, bind_port, allow_network_exposure, \
     created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)";

macro_rules! bind_environment {
    ($sql:expr, $e:expr) => {
        sqlx::query($sql)
            .bind(*$e.id.as_uuid())
            .bind(*$e.project_id.as_uuid())
            .bind(&$e.name)
            .bind($e.base_url.as_deref())
            .bind(sqlx::types::Json(&$e.default_headers))
            .bind(sqlx::types::Json(&$e.variables))
            .bind($e.bind.host.to_string())
            .bind(i32::from($e.bind.port))
            .bind($e.bind.allow_network_exposure)
            .bind(workbench_core::time::truncate($e.created_at))
            .bind(workbench_core::time::truncate($e.updated_at))
    };
}
pub(crate) use bind_environment;

#[derive(sqlx::FromRow)]
struct EnvironmentRow {
    id: Uuid,
    project_id: Uuid,
    name: String,
    base_url: Option<String>,
    default_headers: Json<BTreeMap<String, String>>,
    variables: Json<BTreeMap<String, String>>,
    bind_host: String,
    bind_port: i32,
    allow_network_exposure: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<EnvironmentRow> for Environment {
    type Error = StorageError;

    fn try_from(r: EnvironmentRow) -> Result<Self, Self::Error> {
        let host: IpAddr = r
            .bind_host
            .parse()
            .map_err(|_| StorageError::corrupt(ENTITY, r.id, "invalid bind_host"))?;
        let port = u16::try_from(r.bind_port)
            .map_err(|_| StorageError::corrupt(ENTITY, r.id, "bind_port out of range"))?;
        Ok(Self {
            id: EnvironmentId::from_uuid(r.id),
            project_id: ProjectId::from_uuid(r.project_id),
            name: r.name,
            base_url: r.base_url,
            default_headers: r.default_headers.0,
            variables: r.variables.0,
            bind: BindAddress {
                host,
                port,
                allow_network_exposure: r.allow_network_exposure,
            },
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
    }
}

fn duplicate_name(env: &Environment) -> StorageError {
    StorageError::Conflict(format!(
        "an environment named `{}` already exists in this project",
        env.name
    ))
}

impl Store {
    /// Adds an environment. Names are unique within a project.
    pub async fn create_environment(&self, env: &Environment) -> StorageResult<()> {
        env.validate()
            .map_err(|e| StorageError::invalid(ENTITY, e))?;
        with_pool!(self, |pool| bind_environment!(INSERT_ENVIRONMENT, env)
            .execute(pool)
            .await
            .map_err(|e| map_constraint(
                e,
                || duplicate_name(env),
                || StorageError::not_found("project", env.project_id),
            ))?
            .rows_affected());
        Ok(())
    }

    pub async fn get_environment(&self, id: EnvironmentId) -> StorageResult<Environment> {
        let sql = format!("SELECT {COLUMNS} FROM environments WHERE id = $1");
        let row: Option<EnvironmentRow> = with_pool!(self, |pool| sqlx::query_as(&sql)
            .bind(*id.as_uuid())
            .fetch_optional(pool)
            .await?);
        row.ok_or_else(|| StorageError::not_found(ENTITY, id))?
            .try_into()
    }

    /// Environments of a project, oldest first.
    pub async fn list_environments(
        &self,
        project_id: ProjectId,
    ) -> StorageResult<Vec<Environment>> {
        let sql = format!(
            "SELECT {COLUMNS} FROM environments WHERE project_id = $1 ORDER BY created_at, id"
        );
        let rows: Vec<EnvironmentRow> = with_pool!(self, |pool| sqlx::query_as(&sql)
            .bind(*project_id.as_uuid())
            .fetch_all(pool)
            .await?);
        rows.into_iter().map(Environment::try_from).collect()
    }

    /// Saves every editable field. The project and creation time are kept.
    pub async fn update_environment(&self, env: &Environment) -> StorageResult<Environment> {
        env.validate()
            .map_err(|e| StorageError::invalid(ENTITY, e))?;
        let affected = with_pool!(self, |pool| sqlx::query(
            "UPDATE environments SET name = $2, base_url = $3, default_headers = $4, \
             variables = $5, bind_host = $6, bind_port = $7, allow_network_exposure = $8, \
             updated_at = $9 WHERE id = $1"
        )
        .bind(*env.id.as_uuid())
        .bind(&env.name)
        .bind(env.base_url.as_deref())
        .bind(Json(&env.default_headers))
        .bind(Json(&env.variables))
        .bind(env.bind.host.to_string())
        .bind(i32::from(env.bind.port))
        .bind(env.bind.allow_network_exposure)
        .bind(time::now())
        .execute(pool)
        .await
        .map_err(|e| map_constraint(
            e,
            || duplicate_name(env),
            || StorageError::not_found(ENTITY, env.id)
        ))?
        .rows_affected());
        if affected == 0 {
            return Err(StorageError::not_found(ENTITY, env.id));
        }
        self.get_environment(env.id).await
    }

    pub async fn delete_environment(&self, id: EnvironmentId) -> StorageResult<()> {
        let affected = with_pool!(self, |pool| sqlx::query(
            "DELETE FROM environments WHERE id = $1"
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
