//! `/workspaces` (spec §8.1).

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::Deserialize;
use serde_json::Value;
use workbench_core::WorkspaceId;

use crate::extract::{ApiJson, ApiPath, ApiQuery};
use crate::util::{PageQuery, data, paginate};
use crate::{ApiResult, AppState};

#[derive(Deserialize)]
pub struct ListQuery {
    #[serde(default)]
    include_archived: bool,
    page: Option<usize>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateWorkspace {
    name: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateWorkspace {
    name: Option<String>,
    archived: Option<bool>,
}

pub async fn list(
    State(s): State<AppState>,
    ApiQuery(q): ApiQuery<ListQuery>,
) -> ApiResult<Json<Value>> {
    let page = PageQuery {
        page: q.page,
        limit: q.limit,
    };
    Ok(Json(paginate(
        s.store.list_workspaces(q.include_archived).await?,
        page,
    )?))
}

pub async fn create(
    State(s): State<AppState>,
    ApiJson(body): ApiJson<CreateWorkspace>,
) -> ApiResult<impl IntoResponse> {
    let ws = s.store.create_workspace(&body.name).await?;
    Ok((StatusCode::CREATED, Json(data(ws))))
}

pub async fn get(
    State(s): State<AppState>,
    ApiPath(id): ApiPath<WorkspaceId>,
) -> ApiResult<Json<Value>> {
    Ok(Json(data(s.store.get_workspace(id).await?)))
}

pub async fn update(
    State(s): State<AppState>,
    ApiPath(id): ApiPath<WorkspaceId>,
    ApiJson(body): ApiJson<UpdateWorkspace>,
) -> ApiResult<Json<Value>> {
    let mut ws = s.store.get_workspace(id).await?;
    if let Some(name) = body.name {
        ws = s.store.rename_workspace(id, &name).await?;
    }
    if let Some(archived) = body.archived {
        ws = s.store.set_workspace_archived(id, archived).await?;
    }
    Ok(Json(data(ws)))
}

/// Deletes the workspace and everything in it, stopping its mock servers first.
pub async fn delete(
    State(s): State<AppState>,
    ApiPath(id): ApiPath<WorkspaceId>,
) -> ApiResult<StatusCode> {
    for project in s.store.list_projects(id, true).await? {
        s.runtime.stop_project(project.id).await;
    }
    s.store.delete_workspace(id).await?;
    Ok(StatusCode::NO_CONTENT)
}
