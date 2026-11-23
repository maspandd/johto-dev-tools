//! Workspace repository.

use chrono::{DateTime, Utc};
use uuid::Uuid;
use workbench_core::{WorkspaceId, time};
use workbench_domain::Workspace;

use crate::store::with_pool;
use crate::{StorageError, StorageResult, Store};

const ENTITY: &str = "workspace";
const COLUMNS: &str = "id, name, archived_at, created_at, updated_at";

#[derive(sqlx::FromRow)]
struct WorkspaceRow {
    id: Uuid,
    name: String,
    archived_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<WorkspaceRow> for Workspace {
    fn from(r: WorkspaceRow) -> Self {
        Self {
            id: WorkspaceId::from_uuid(r.id),
            name: r.name,
            archived_at: r.archived_at,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

impl Store {
    /// Creates a workspace with the given name.
    pub async fn create_workspace(&self, name: &str) -> StorageResult<Workspace> {
        let workspace = Workspace::new(name);
        self.insert_workspace(&workspace).await?;
        Ok(workspace)
    }

    /// Inserts a fully specified workspace (used by import and sync).
    pub async fn insert_workspace(&self, w: &Workspace) -> StorageResult<()> {
        w.validate().map_err(|e| StorageError::invalid(ENTITY, e))?;
        let sql = format!("INSERT INTO workspaces ({COLUMNS}) VALUES ($1, $2, $3, $4, $5)");
        with_pool!(self, |pool| sqlx::query(&sql)
            .bind(*w.id.as_uuid())
            .bind(&w.name)
            .bind(w.archived_at.map(time::truncate))
            .bind(time::truncate(w.created_at))
            .bind(time::truncate(w.updated_at))
            .execute(pool)
            .await
            .map_err(|e| crate::error::map_constraint(
                e,
                || StorageError::Conflict(format!("workspace {} already exists", w.id)),
                || StorageError::Conflict("constraint violation".into()),
            ))?
            .rows_affected());
        Ok(())
    }

    pub async fn get_workspace(&self, id: WorkspaceId) -> StorageResult<Workspace> {
        let sql = format!("SELECT {COLUMNS} FROM workspaces WHERE id = $1");
        let row: Option<WorkspaceRow> = with_pool!(self, |pool| sqlx::query_as(&sql)
            .bind(*id.as_uuid())
            .fetch_optional(pool)
            .await?);
        row.map(Workspace::from)
            .ok_or_else(|| StorageError::not_found(ENTITY, id))
    }

    /// Oldest first. Archived workspaces are included only on request.
    pub async fn list_workspaces(&self, include_archived: bool) -> StorageResult<Vec<Workspace>> {
        let sql = format!(
            "SELECT {COLUMNS} FROM workspaces WHERE ($1 OR archived_at IS NULL) ORDER BY created_at, id"
        );
        let rows: Vec<WorkspaceRow> = with_pool!(self, |pool| sqlx::query_as(&sql)
            .bind(include_archived)
            .fetch_all(pool)
            .await?);
        Ok(rows.into_iter().map(Workspace::from).collect())
    }

    pub async fn rename_workspace(&self, id: WorkspaceId, name: &str) -> StorageResult<Workspace> {
        let mut w = self.get_workspace(id).await?;
        w.name = name.trim().to_owned();
        w.validate().map_err(|e| StorageError::invalid(ENTITY, e))?;
        let affected = with_pool!(self, |pool| sqlx::query(
            "UPDATE workspaces SET name = $2, updated_at = $3 WHERE id = $1"
        )
        .bind(*id.as_uuid())
        .bind(&w.name)
        .bind(time::now())
        .execute(pool)
        .await?
        .rows_affected());
        if affected == 0 {
            return Err(StorageError::not_found(ENTITY, id));
        }
        self.get_workspace(id).await
    }

    /// Archives (`true`) or restores (`false`) a workspace.
    pub async fn set_workspace_archived(
        &self,
        id: WorkspaceId,
        archived: bool,
    ) -> StorageResult<Workspace> {
        let now = time::now();
        let affected = with_pool!(self, |pool| sqlx::query(
            "UPDATE workspaces SET archived_at = $2, updated_at = $3 WHERE id = $1"
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
        self.get_workspace(id).await
    }

    /// Deletes a workspace and everything in it.
    pub async fn delete_workspace(&self, id: WorkspaceId) -> StorageResult<()> {
        let affected = with_pool!(self, |pool| sqlx::query(
            "DELETE FROM workspaces WHERE id = $1"
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
