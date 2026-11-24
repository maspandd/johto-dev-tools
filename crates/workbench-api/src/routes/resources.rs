//! `/resources`: CRUD resource definitions and their data.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use workbench_core::{ErrorDetail, ProjectId, ResourceId};
use workbench_domain::Resource;
use workbench_storage::ResourceRecord;

use crate::extract::{ApiJson, ApiPath, ApiQuery};
use crate::util::{PageQuery, apply_patch, data, paginate};
use crate::{ApiError, ApiResult, AppState};

#[derive(Serialize)]
pub struct ResourceDto {
    #[serde(flatten)]
    resource: Resource,
    project_id: ProjectId,
    /// `false` until data is first generated; the next server start seeds it.
    data_initialized: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<ResourceRecord> for ResourceDto {
    fn from(r: ResourceRecord) -> Self {
        Self {
            resource: r.resource,
            project_id: r.project_id,
            data_initialized: r.data_initialized_at.is_some(),
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

pub async fn list(
    State(s): State<AppState>,
    ApiPath(project_id): ApiPath<ProjectId>,
    ApiQuery(page): ApiQuery<PageQuery>,
) -> ApiResult<Json<Value>> {
    s.store.get_project(project_id).await?;
    let mut list = Vec::new();
    for r in s.store.list_resources(project_id).await? {
        list.push(ResourceDto::from(s.store.get_resource(r.id).await?));
    }
    Ok(Json(paginate(list, page)?))
}

/// Body: a resource definition (`name`, `path`, `schema`, `seed`, ...).
pub async fn create(
    State(s): State<AppState>,
    ApiPath(project_id): ApiPath<ProjectId>,
    ApiJson(mut body): ApiJson<Value>,
) -> ApiResult<impl IntoResponse> {
    if let Some(map) = body.as_object_mut() {
        map.shift_remove("id");
    }
    let resource: Resource = serde_json::from_value(body)
        .map_err(|e| ApiError::validation(vec![ErrorDetail::new("body", e.to_string())]))?;
    s.store.create_resource(project_id, &resource).await?;
    s.runtime.refresh_project(project_id).await;
    let record = s.store.get_resource(resource.id).await?;
    Ok((StatusCode::CREATED, Json(data(ResourceDto::from(record)))))
}

pub async fn get(
    State(s): State<AppState>,
    ApiPath(id): ApiPath<ResourceId>,
) -> ApiResult<Json<Value>> {
    Ok(Json(data(ResourceDto::from(
        s.store.get_resource(id).await?,
    ))))
}

pub async fn update(
    State(s): State<AppState>,
    ApiPath(id): ApiPath<ResourceId>,
    ApiJson(body): ApiJson<Value>,
) -> ApiResult<Json<Value>> {
    let current = s.store.get_resource(id).await?;
    let resource = apply_patch(&current.resource, &body, &["id"])?;
    let record = s.store.update_resource(&resource).await?;
    s.runtime.refresh_project(record.project_id).await;
    Ok(Json(data(ResourceDto::from(record))))
}

pub async fn delete(
    State(s): State<AppState>,
    ApiPath(id): ApiPath<ResourceId>,
) -> ApiResult<StatusCode> {
    let record = s.store.get_resource(id).await?;
    s.store.delete_resource(id).await?;
    s.runtime.refresh_project(record.project_id).await;
    Ok(StatusCode::NO_CONTENT)
}

/// Current records (live from a running server when there is one).
pub async fn records(
    State(s): State<AppState>,
    ApiPath(id): ApiPath<ResourceId>,
    ApiQuery(page): ApiQuery<PageQuery>,
) -> ApiResult<Json<Value>> {
    let record = s.store.get_resource(id).await?;
    let rows = match s.runtime.resource_records(record.project_id, id).await {
        Some(rows) => rows,
        None => s
            .store
            .load_resource_records(id)
            .await?
            .unwrap_or_default()
            .into_iter()
            .map(|r| r.data)
            .collect(),
    };
    Ok(Json(paginate(rows, page)?))
}

/// Replaces the data with fresh seed records. Without a running server the
/// data is cleared and regenerated on the next start.
pub async fn reset(
    State(s): State<AppState>,
    ApiPath(id): ApiPath<ResourceId>,
) -> ApiResult<StatusCode> {
    let record = s.store.get_resource(id).await?;
    if !s.runtime.reset_resource(record.project_id, id).await? {
        s.store.reset_resource_records(id).await?;
    }
    Ok(StatusCode::NO_CONTENT)
}
