//! `/projects` (spec §8.2). Archive import and export live in `archive.rs`.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::Deserialize;
use serde_json::Value;
use workbench_core::{ErrorDetail, ProjectId, WorkspaceId};
use workbench_domain::Project;

use crate::extract::{ApiJson, ApiPath, ApiQuery};
use crate::util::{PageQuery, apply_patch, data, paginate};
use crate::{ApiError, ApiResult, AppState};

/// Fields clients cannot set directly.
const FIXED: &[&str] = &[
    "id",
    "workspace_id",
    "format_version",
    "archived_at",
    "created_at",
    "updated_at",
];

#[derive(Deserialize)]
pub struct ListQuery {
    workspace_id: Option<WorkspaceId>,
    #[serde(default)]
    include_archived: bool,
    page: Option<usize>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DuplicateBody {
    name: Option<String>,
}

pub async fn list(
    State(s): State<AppState>,
    ApiQuery(q): ApiQuery<ListQuery>,
) -> ApiResult<Json<Value>> {
    let Some(workspace_id) = q.workspace_id else {
        return Err(ApiError::validation(vec![ErrorDetail::new(
            "query.workspace_id",
            "is required",
        )]));
    };
    s.store.get_workspace(workspace_id).await?;
    let projects = s
        .store
        .list_projects(workspace_id, q.include_archived)
        .await?;
    Ok(Json(paginate(
        projects,
        PageQuery {
            page: q.page,
            limit: q.limit,
        },
    )?))
}

/// Body: `{ "workspace_id", "name", "description"?, "base_path"?, "default_headers"? }`.
pub async fn create(
    State(s): State<AppState>,
    ApiJson(body): ApiJson<Value>,
) -> ApiResult<impl IntoResponse> {
    let workspace_id: WorkspaceId = body
        .get("workspace_id")
        .cloned()
        .and_then(|v| serde_json::from_value(v).ok())
        .ok_or_else(|| {
            ApiError::validation(vec![ErrorDetail::new("body.workspace_id", "is required")])
        })?;
    let name = body.get("name").and_then(Value::as_str).unwrap_or_default();
    let project = apply_patch(&Project::new(workspace_id, name), &body, FIXED)?;
    s.store.insert_project(&project).await?;
    Ok((StatusCode::CREATED, Json(data(project))))
}

pub async fn get(
    State(s): State<AppState>,
    ApiPath(id): ApiPath<ProjectId>,
) -> ApiResult<Json<Value>> {
    Ok(Json(data(s.store.get_project(id).await?)))
}

/// JSON merge patch; also accepts `"archived": true | false`. Archiving
/// stops the project's mock servers. Base path and header changes apply
/// to servers started afterwards.
pub async fn update(
    State(s): State<AppState>,
    ApiPath(id): ApiPath<ProjectId>,
    ApiJson(mut body): ApiJson<Value>,
) -> ApiResult<Json<Value>> {
    let archived = body
        .as_object_mut()
        .and_then(|m| m.shift_remove("archived"))
        .map(|v| {
            v.as_bool().ok_or_else(|| {
                ApiError::validation(vec![ErrorDetail::new("body.archived", "must be a boolean")])
            })
        })
        .transpose()?;
    let current = s.store.get_project(id).await?;
    let mut project = s
        .store
        .update_project(&apply_patch(&current, &body, FIXED)?)
        .await?;
    if let Some(archived) = archived {
        if archived {
            s.runtime.stop_project(id).await;
        }
        project = s.store.set_project_archived(id, archived).await?;
    }
    Ok(Json(data(project)))
}

pub async fn delete(
    State(s): State<AppState>,
    ApiPath(id): ApiPath<ProjectId>,
) -> ApiResult<StatusCode> {
    s.store.get_project(id).await?;
    s.runtime.stop_project(id).await;
    s.store.delete_project(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Copies configuration and data; secrets and request logs are not copied.
pub async fn duplicate(
    State(s): State<AppState>,
    ApiPath(id): ApiPath<ProjectId>,
    body: axum::body::Bytes,
) -> ApiResult<impl IntoResponse> {
    let name = if body.iter().all(u8::is_ascii_whitespace) {
        None
    } else {
        serde_json::from_slice::<DuplicateBody>(&body)
            .map_err(|e| ApiError::validation(vec![ErrorDetail::new("body", e.to_string())]))?
            .name
    };
    let copy = s.store.duplicate_project(id, name.as_deref()).await?;
    Ok((StatusCode::CREATED, Json(data(copy))))
}
