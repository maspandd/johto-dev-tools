//! Endpoint repository.
//!
//! Name, method, path, enabled state and position are columns; the request
//! spec, responses, rules and auth settings are one JSON `config` value.

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::types::Json;
use uuid::Uuid;
use workbench_core::{EndpointId, ProjectId, time};
use workbench_domain::{
    AuthSimulation, Endpoint, FieldError, Method, MockResponse, PathTemplate, RequestSpec,
    ResponseRule,
};

use crate::error::map_constraint;
use crate::store::with_pool;
use crate::{StorageError, StorageResult, Store};

const ENTITY: &str = "endpoint";
const COLUMNS: &str = "id, project_id, name, description, method, path, enabled, position, \
                       config, created_at, updated_at";

/// New endpoints go to the end of their project's list.
pub(crate) const INSERT_ENDPOINT: &str = "INSERT INTO endpoints (id, project_id, name, \
     description, method, path, enabled, config, created_at, updated_at, position) \
     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, \
     (SELECT COALESCE(MAX(position), -1) + 1 FROM endpoints WHERE project_id = $2))";

#[derive(Serialize)]
pub(crate) struct ConfigRef<'a> {
    pub request: &'a RequestSpec,
    pub response: &'a MockResponse,
    pub rules: &'a [ResponseRule],
    pub auth: &'a AuthSimulation,
}

impl<'a> ConfigRef<'a> {
    pub(crate) fn of(e: &'a Endpoint) -> Json<Self> {
        Json(Self {
            request: &e.request,
            response: &e.response,
            rules: &e.rules,
            auth: &e.auth,
        })
    }
}

#[derive(Deserialize)]
struct Config {
    #[serde(default)]
    request: RequestSpec,
    #[serde(default)]
    response: MockResponse,
    #[serde(default)]
    rules: Vec<ResponseRule>,
    #[serde(default)]
    auth: AuthSimulation,
}

/// `bind_endpoint!(sql, project_id, endpoint, timestamp)` binds an insert.
macro_rules! bind_endpoint {
    ($sql:expr, $project_id:expr, $e:expr, $now:expr) => {
        sqlx::query($sql)
            .bind(*$e.id.as_uuid())
            .bind(*$project_id.as_uuid())
            .bind(&$e.name)
            .bind(&$e.description)
            .bind($e.method.as_str())
            .bind($e.path.to_string())
            .bind($e.enabled)
            .bind($crate::endpoints::ConfigRef::of(&$e))
            .bind($now)
            .bind($now)
    };
}
pub(crate) use bind_endpoint;

/// An endpoint with its storage metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct EndpointRecord {
    pub project_id: ProjectId,
    /// Display order within the project, starting at 0.
    pub position: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub endpoint: Endpoint,
}

#[derive(sqlx::FromRow)]
struct EndpointRow {
    id: Uuid,
    project_id: Uuid,
    name: String,
    description: String,
    method: String,
    path: String,
    enabled: bool,
    position: i32,
    config: Json<Config>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<EndpointRow> for EndpointRecord {
    type Error = StorageError;

    fn try_from(r: EndpointRow) -> Result<Self, Self::Error> {
        let method: Method = r
            .method
            .parse()
            .map_err(|e| StorageError::corrupt(ENTITY, r.id, e))?;
        let path =
            PathTemplate::parse(&r.path).map_err(|e| StorageError::corrupt(ENTITY, r.id, e))?;
        let Config {
            request,
            response,
            rules,
            auth,
        } = r.config.0;
        Ok(Self {
            project_id: ProjectId::from_uuid(r.project_id),
            position: r.position,
            created_at: r.created_at,
            updated_at: r.updated_at,
            endpoint: Endpoint {
                id: EndpointId::from_uuid(r.id),
                name: r.name,
                description: r.description,
                method,
                path,
                enabled: r.enabled,
                request,
                response,
                rules,
                auth,
            },
        })
    }
}

fn validate(e: &Endpoint) -> StorageResult<()> {
    e.validate().map_err(|errors| {
        StorageError::invalid(
            ENTITY,
            errors
                .into_iter()
                .map(|err| FieldError::new("endpoint", err.to_string()))
                .collect(),
        )
    })
}

fn copy_of(e: &Endpoint) -> Endpoint {
    Endpoint {
        id: EndpointId::new(),
        name: format!("{} (copy)", e.name),
        // A copy answers the same route, so it starts disabled to avoid a conflict.
        enabled: false,
        ..e.clone()
    }
}

impl Store {
    /// Adds an endpoint at the end of the project's list.
    /// Route conflicts are not checked here; the mock engine reports them
    /// for enabled endpoints when a server starts or reloads.
    pub async fn create_endpoint(&self, project_id: ProjectId, e: &Endpoint) -> StorageResult<()> {
        validate(e)?;
        let now = time::now();
        with_pool!(self, |pool| bind_endpoint!(
            INSERT_ENDPOINT,
            project_id,
            e,
            now
        )
        .execute(pool)
        .await
        .map_err(|err| map_constraint(
            err,
            || StorageError::Conflict(format!("endpoint {} already exists", e.id)),
            || StorageError::not_found("project", project_id),
        ))?
        .rows_affected());
        Ok(())
    }

    /// Adds several endpoints at the end of the project's list, in order,
    /// in one transaction: all are saved or none are.
    pub async fn create_endpoints(
        &self,
        project_id: ProjectId,
        endpoints: &[Endpoint],
    ) -> StorageResult<()> {
        for e in endpoints {
            validate(e)?;
        }
        let now = time::now();
        let result: Result<(), sqlx::Error> = with_pool!(self, |pool| async {
            let mut tx = pool.begin().await?;
            for e in endpoints {
                bind_endpoint!(INSERT_ENDPOINT, project_id, e, now)
                    .execute(&mut *tx)
                    .await?;
            }
            tx.commit().await
        }
        .await);
        result.map_err(|err| {
            map_constraint(
                err,
                || StorageError::Conflict("an endpoint with the same ID already exists".into()),
                || StorageError::not_found("project", project_id),
            )
        })
    }

    pub async fn get_endpoint(&self, id: EndpointId) -> StorageResult<EndpointRecord> {
        let sql = format!("SELECT {COLUMNS} FROM endpoints WHERE id = $1");
        let row: Option<EndpointRow> = with_pool!(self, |pool| sqlx::query_as(&sql)
            .bind(*id.as_uuid())
            .fetch_optional(pool)
            .await?);
        row.ok_or_else(|| StorageError::not_found(ENTITY, id))?
            .try_into()
    }

    /// Endpoints of a project in display order, with storage metadata.
    pub async fn list_endpoint_records(
        &self,
        project_id: ProjectId,
    ) -> StorageResult<Vec<EndpointRecord>> {
        let sql = format!(
            "SELECT {COLUMNS} FROM endpoints WHERE project_id = $1 \
             ORDER BY position, created_at, id"
        );
        let rows: Vec<EndpointRow> = with_pool!(self, |pool| sqlx::query_as(&sql)
            .bind(*project_id.as_uuid())
            .fetch_all(pool)
            .await?);
        rows.into_iter().map(EndpointRecord::try_from).collect()
    }

    /// Endpoints of a project in display order, ready for the mock engine.
    pub async fn list_endpoints(&self, project_id: ProjectId) -> StorageResult<Vec<Endpoint>> {
        Ok(self
            .list_endpoint_records(project_id)
            .await?
            .into_iter()
            .map(|r| r.endpoint)
            .collect())
    }

    /// Saves every field of an existing endpoint. Its project and position are kept.
    pub async fn update_endpoint(&self, e: &Endpoint) -> StorageResult<EndpointRecord> {
        validate(e)?;
        let affected = with_pool!(self, |pool| sqlx::query(
            "UPDATE endpoints SET name = $2, description = $3, method = $4, path = $5, \
             enabled = $6, config = $7, updated_at = $8 WHERE id = $1"
        )
        .bind(*e.id.as_uuid())
        .bind(&e.name)
        .bind(&e.description)
        .bind(e.method.as_str())
        .bind(e.path.to_string())
        .bind(e.enabled)
        .bind(ConfigRef::of(e))
        .bind(time::now())
        .execute(pool)
        .await?
        .rows_affected());
        if affected == 0 {
            return Err(StorageError::not_found(ENTITY, e.id));
        }
        self.get_endpoint(e.id).await
    }

    pub async fn set_endpoint_enabled(
        &self,
        id: EndpointId,
        enabled: bool,
    ) -> StorageResult<EndpointRecord> {
        let affected = with_pool!(self, |pool| sqlx::query(
            "UPDATE endpoints SET enabled = $2, updated_at = $3 WHERE id = $1"
        )
        .bind(*id.as_uuid())
        .bind(enabled)
        .bind(time::now())
        .execute(pool)
        .await?
        .rows_affected());
        if affected == 0 {
            return Err(StorageError::not_found(ENTITY, id));
        }
        self.get_endpoint(id).await
    }

    /// Copies an endpoint into the same project, appended and disabled.
    pub async fn duplicate_endpoint(&self, id: EndpointId) -> StorageResult<EndpointRecord> {
        let source = self.get_endpoint(id).await?;
        let copy = copy_of(&source.endpoint);
        self.create_endpoint(source.project_id, &copy).await?;
        self.get_endpoint(copy.id).await
    }

    pub async fn delete_endpoint(&self, id: EndpointId) -> StorageResult<()> {
        let affected = with_pool!(self, |pool| sqlx::query(
            "DELETE FROM endpoints WHERE id = $1"
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

    /// Sets the display order. `order` must list every endpoint of the
    /// project exactly once.
    pub async fn reorder_endpoints(
        &self,
        project_id: ProjectId,
        order: &[EndpointId],
    ) -> StorageResult<()> {
        let current: HashSet<EndpointId> = self
            .list_endpoint_records(project_id)
            .await?
            .into_iter()
            .map(|r| r.endpoint.id)
            .collect();
        let requested: HashSet<EndpointId> = order.iter().copied().collect();
        if requested.len() != order.len() || requested != current {
            return Err(StorageError::invalid(
                ENTITY,
                vec![FieldError::new(
                    "order",
                    "must list every endpoint of the project exactly once",
                )],
            ));
        }
        with_pool!(self, |pool| {
            let mut tx = pool.begin().await?;
            for (position, id) in order.iter().enumerate() {
                sqlx::query("UPDATE endpoints SET position = $3 WHERE id = $1 AND project_id = $2")
                    .bind(*id.as_uuid())
                    .bind(*project_id.as_uuid())
                    .bind(i32::try_from(position).unwrap_or(i32::MAX))
                    .execute(&mut *tx)
                    .await?;
            }
            tx.commit().await?;
        });
        Ok(())
    }
}
