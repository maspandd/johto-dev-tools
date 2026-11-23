//! Project repository, including deep duplication.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use sqlx::types::Json;
use uuid::Uuid;
use workbench_core::{
    DatasetId, EndpointId, EnvironmentId, ProjectId, ResourceId, WorkspaceId, time,
};
use workbench_domain::Project;

use crate::datasets::{INSERT_DATASET, bind_dataset};
use crate::endpoints::{INSERT_ENDPOINT, bind_endpoint};
use crate::environments::{INSERT_ENVIRONMENT, bind_environment};
use crate::error::map_constraint;
use crate::resources::{INSERT_RESOURCE, bind_record, bind_resource};
use crate::store::with_pool;
use crate::{StorageError, StorageResult, Store};

const ENTITY: &str = "project";
const COLUMNS: &str = "id, workspace_id, name, description, base_path, default_headers, \
                       format_version, archived_at, created_at, updated_at";
const INSERT_PROJECT: &str = "INSERT INTO projects (id, workspace_id, name, description, base_path, \
     default_headers, format_version, archived_at, created_at, updated_at) \
     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)";

macro_rules! bind_project {
    ($sql:expr, $p:expr) => {
        sqlx::query($sql)
            .bind(*$p.id.as_uuid())
            .bind(*$p.workspace_id.as_uuid())
            .bind(&$p.name)
            .bind(&$p.description)
            .bind($p.base_path.as_deref())
            .bind(Json(&$p.default_headers))
            .bind(i32::try_from($p.format_version).unwrap_or(i32::MAX))
            .bind($p.archived_at.map(time::truncate))
            .bind(time::truncate($p.created_at))
            .bind(time::truncate($p.updated_at))
    };
}

#[derive(sqlx::FromRow)]
struct ProjectRow {
    id: Uuid,
    workspace_id: Uuid,
    name: String,
    description: String,
    base_path: Option<String>,
    default_headers: Json<BTreeMap<String, String>>,
    format_version: i32,
    archived_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<ProjectRow> for Project {
    type Error = StorageError;

    fn try_from(r: ProjectRow) -> Result<Self, Self::Error> {
        let format_version = u32::try_from(r.format_version)
            .map_err(|_| StorageError::corrupt(ENTITY, r.id, "negative format_version"))?;
        Ok(Self {
            id: ProjectId::from_uuid(r.id),
            workspace_id: WorkspaceId::from_uuid(r.workspace_id),
            name: r.name,
            description: r.description,
            base_path: r.base_path,
            default_headers: r.default_headers.0,
            format_version,
            archived_at: r.archived_at,
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
    }
}

fn copy_name(name: &str) -> String {
    format!("{name} (copy)")
}

impl Store {
    /// Creates an empty project in a workspace.
    pub async fn create_project(
        &self,
        workspace_id: WorkspaceId,
        name: &str,
    ) -> StorageResult<Project> {
        let project = Project::new(workspace_id, name);
        self.insert_project(&project).await?;
        Ok(project)
    }

    /// Inserts a fully specified project (used by import and sync).
    pub async fn insert_project(&self, p: &Project) -> StorageResult<()> {
        p.validate().map_err(|e| StorageError::invalid(ENTITY, e))?;
        with_pool!(self, |pool| bind_project!(INSERT_PROJECT, p)
            .execute(pool)
            .await
            .map_err(|e| map_constraint(
                e,
                || StorageError::Conflict(format!("project {} already exists", p.id)),
                || StorageError::not_found("workspace", p.workspace_id),
            ))?
            .rows_affected());
        Ok(())
    }

    pub async fn get_project(&self, id: ProjectId) -> StorageResult<Project> {
        let sql = format!("SELECT {COLUMNS} FROM projects WHERE id = $1");
        let row: Option<ProjectRow> = with_pool!(self, |pool| sqlx::query_as(&sql)
            .bind(*id.as_uuid())
            .fetch_optional(pool)
            .await?);
        row.ok_or_else(|| StorageError::not_found(ENTITY, id))?
            .try_into()
    }

    /// Projects in a workspace, oldest first.
    pub async fn list_projects(
        &self,
        workspace_id: WorkspaceId,
        include_archived: bool,
    ) -> StorageResult<Vec<Project>> {
        let sql = format!(
            "SELECT {COLUMNS} FROM projects WHERE workspace_id = $1 \
             AND ($2 OR archived_at IS NULL) ORDER BY created_at, id"
        );
        let rows: Vec<ProjectRow> = with_pool!(self, |pool| sqlx::query_as(&sql)
            .bind(*workspace_id.as_uuid())
            .bind(include_archived)
            .fetch_all(pool)
            .await?);
        rows.into_iter().map(Project::try_from).collect()
    }

    /// Saves name, description, base path and default headers.
    /// The workspace, format version and timestamps are not changed by callers.
    pub async fn update_project(&self, p: &Project) -> StorageResult<Project> {
        p.validate().map_err(|e| StorageError::invalid(ENTITY, e))?;
        let affected = with_pool!(self, |pool| sqlx::query(
            "UPDATE projects SET name = $2, description = $3, base_path = $4, \
             default_headers = $5, updated_at = $6 WHERE id = $1"
        )
        .bind(*p.id.as_uuid())
        .bind(&p.name)
        .bind(&p.description)
        .bind(p.base_path.as_deref())
        .bind(Json(&p.default_headers))
        .bind(time::now())
        .execute(pool)
        .await?
        .rows_affected());
        if affected == 0 {
            return Err(StorageError::not_found(ENTITY, p.id));
        }
        self.get_project(p.id).await
    }

    /// Archives (`true`) or restores (`false`) a project.
    pub async fn set_project_archived(
        &self,
        id: ProjectId,
        archived: bool,
    ) -> StorageResult<Project> {
        let now = time::now();
        let affected = with_pool!(self, |pool| sqlx::query(
            "UPDATE projects SET archived_at = $2, updated_at = $3 WHERE id = $1"
        )
        .bind(*id.as_uuid())
        .bind(archived.then_some(now))
        .bind(now)
        .execute(pool)
        .await?
        .rows_affected());
        if affected == 0 {
            return Err(StorageError::not_found(ENTITY, id));
        }
        self.get_project(id).await
    }

    /// Deletes a project with its environments, endpoints and request logs.
    pub async fn delete_project(&self, id: ProjectId) -> StorageResult<()> {
        let affected = with_pool!(self, |pool| sqlx::query(
            "DELETE FROM projects WHERE id = $1"
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

    /// Copies a project with its environments, endpoints, resources (with
    /// their records) and datasets under new IDs, in one transaction.
    /// Request logs are not copied. The copy is
    /// independent: changing it never affects the original.
    pub async fn duplicate_project(
        &self,
        id: ProjectId,
        new_name: Option<&str>,
    ) -> StorageResult<Project> {
        let source = self.get_project(id).await?;
        let environments = self.list_environments(id).await?;
        let endpoints = self.list_endpoints(id).await?;
        let datasets = self.list_datasets(id).await?;
        let mut resources = Vec::new();
        for r in self.list_resources(id).await? {
            let record = self.get_resource(r.id).await?;
            let rows = self.load_resource_records(r.id).await?.unwrap_or_default();
            resources.push((record, rows));
        }

        let now = time::now();
        let copy = Project {
            id: ProjectId::new(),
            name: new_name.map_or_else(|| copy_name(&source.name), |n| n.trim().to_owned()),
            archived_at: None,
            created_at: now,
            updated_at: now,
            ..source
        };
        copy.validate()
            .map_err(|e| StorageError::invalid(ENTITY, e))?;

        with_pool!(self, |pool| {
            let mut tx = pool.begin().await?;
            bind_project!(INSERT_PROJECT, copy)
                .execute(&mut *tx)
                .await?;
            for env in &environments {
                let env = workbench_domain::Environment {
                    id: EnvironmentId::new(),
                    project_id: copy.id,
                    created_at: now,
                    updated_at: now,
                    ..env.clone()
                };
                bind_environment!(INSERT_ENVIRONMENT, env)
                    .execute(&mut *tx)
                    .await?;
            }
            for ep in &endpoints {
                let ep = workbench_domain::Endpoint {
                    id: EndpointId::new(),
                    ..ep.clone()
                };
                bind_endpoint!(INSERT_ENDPOINT, copy.id, ep, now)
                    .execute(&mut *tx)
                    .await?;
            }
            for (record, rows) in &resources {
                let r = workbench_domain::Resource {
                    id: ResourceId::new(),
                    ..record.resource.clone()
                };
                bind_resource!(INSERT_RESOURCE, copy.id, r, record.data_initialized_at, now)
                    .execute(&mut *tx)
                    .await?;
                for row in rows {
                    bind_record!(r.id, row).execute(&mut *tx).await?;
                }
            }
            for d in &datasets {
                let d = workbench_domain::Dataset {
                    id: DatasetId::new(),
                    project_id: copy.id,
                    created_at: now,
                    updated_at: now,
                    ..d.clone()
                };
                bind_dataset!(INSERT_DATASET, d).execute(&mut *tx).await?;
            }
            tx.commit().await?;
        });
        Ok(copy)
    }
}
