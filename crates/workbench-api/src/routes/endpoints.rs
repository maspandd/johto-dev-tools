//! `/endpoints` (spec §8.3). Every change is applied to the project's
//! running mock servers immediately.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use workbench_core::{EndpointId, ErrorDetail, ProjectId};
use workbench_domain::Endpoint;
use workbench_storage::EndpointRecord;

use crate::extract::{ApiJson, ApiPath, ApiQuery};
use crate::util::{PageQuery, apply_patch, apply_replace, data, paginate};
use crate::{ApiError, ApiResult, AppState};

/// An endpoint with its project and ordering metadata.
#[derive(Serialize)]
pub struct EndpointDto {
    #[serde(flatten)]
    endpoint: Endpoint,
    project_id: ProjectId,
    position: i32,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<EndpointRecord> for EndpointDto {
    fn from(r: EndpointRecord) -> Self {
        Self {
            endpoint: r.endpoint,
            project_id: r.project_id,
            position: r.position,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReorderBody {
    order: Vec<EndpointId>,
}

fn parse_new(mut body: Value) -> ApiResult<Endpoint> {
    if let Some(map) = body.as_object_mut() {
        // The server assigns IDs.
        map.shift_remove("id");
    }
    serde_json::from_value(body)
        .map_err(|e| ApiError::validation(vec![ErrorDetail::new("body", e.to_string())]))
}

pub async fn list(
    State(s): State<AppState>,
    ApiPath(project_id): ApiPath<ProjectId>,
    ApiQuery(page): ApiQuery<PageQuery>,
) -> ApiResult<Json<Value>> {
    s.store.get_project(project_id).await?;
    let records: Vec<EndpointDto> = s
        .store
        .list_endpoint_records(project_id)
        .await?
        .into_iter()
        .map(EndpointDto::from)
        .collect();
    Ok(Json(paginate(records, page)?))
}

/// Body: an endpoint definition (`name`, `method`, `path`, ...).
pub async fn create(
    State(s): State<AppState>,
    ApiPath(project_id): ApiPath<ProjectId>,
    ApiJson(body): ApiJson<Value>,
) -> ApiResult<impl IntoResponse> {
    let endpoint = parse_new(body)?;
    s.store.create_endpoint(project_id, &endpoint).await?;
    s.runtime.refresh_project(project_id).await;
    let record = s.store.get_endpoint(endpoint.id).await?;
    Ok((StatusCode::CREATED, Json(data(EndpointDto::from(record)))))
}

pub async fn get(
    State(s): State<AppState>,
    ApiPath(id): ApiPath<EndpointId>,
) -> ApiResult<Json<Value>> {
    Ok(Json(data(EndpointDto::from(
        s.store.get_endpoint(id).await?,
    ))))
}

pub async fn update(
    State(s): State<AppState>,
    ApiPath(id): ApiPath<EndpointId>,
    ApiJson(body): ApiJson<Value>,
) -> ApiResult<Json<Value>> {
    let current = s.store.get_endpoint(id).await?;
    let endpoint = apply_patch(&current.endpoint, &body, &["id"])?;
    let record = s.store.update_endpoint(&endpoint).await?;
    s.runtime.refresh_project(record.project_id).await;
    Ok(Json(data(EndpointDto::from(record))))
}

/// Replaces the whole definition (body as for create). Use this rather
/// than `PATCH` when bodies or schemas contain `null` values.
pub async fn replace(
    State(s): State<AppState>,
    ApiPath(id): ApiPath<EndpointId>,
    ApiJson(body): ApiJson<Value>,
) -> ApiResult<Json<Value>> {
    let current = s.store.get_endpoint(id).await?;
    let endpoint = apply_replace(&current.endpoint, &body, &["id"])?;
    let record = s.store.update_endpoint(&endpoint).await?;
    s.runtime.refresh_project(record.project_id).await;
    Ok(Json(data(EndpointDto::from(record))))
}

pub async fn delete(
    State(s): State<AppState>,
    ApiPath(id): ApiPath<EndpointId>,
) -> ApiResult<StatusCode> {
    let record = s.store.get_endpoint(id).await?;
    s.store.delete_endpoint(id).await?;
    s.runtime.refresh_project(record.project_id).await;
    Ok(StatusCode::NO_CONTENT)
}

/// Copies an endpoint into the same project. The copy starts disabled.
pub async fn duplicate(
    State(s): State<AppState>,
    ApiPath(id): ApiPath<EndpointId>,
) -> ApiResult<impl IntoResponse> {
    let record = s.store.duplicate_endpoint(id).await?;
    Ok((StatusCode::CREATED, Json(data(EndpointDto::from(record)))))
}

async fn set_enabled(s: AppState, id: EndpointId, enabled: bool) -> ApiResult<Json<Value>> {
    let record = s.store.set_endpoint_enabled(id, enabled).await?;
    s.runtime.refresh_project(record.project_id).await;
    Ok(Json(data(EndpointDto::from(record))))
}

pub async fn enable(
    State(s): State<AppState>,
    ApiPath(id): ApiPath<EndpointId>,
) -> ApiResult<Json<Value>> {
    set_enabled(s, id, true).await
}

pub async fn disable(
    State(s): State<AppState>,
    ApiPath(id): ApiPath<EndpointId>,
) -> ApiResult<Json<Value>> {
    set_enabled(s, id, false).await
}

/// Body: `{ "order": [endpoint ids...] }` listing every endpoint once.
pub async fn reorder(
    State(s): State<AppState>,
    ApiPath(project_id): ApiPath<ProjectId>,
    ApiJson(body): ApiJson<ReorderBody>,
) -> ApiResult<StatusCode> {
    s.store.get_project(project_id).await?;
    s.store.reorder_endpoints(project_id, &body.order).await?;
    Ok(StatusCode::NO_CONTENT)
}
