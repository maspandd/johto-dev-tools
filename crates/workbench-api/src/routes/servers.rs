//! `/mock-servers` and request history (spec §4.6, §8.4).

use axum::Json;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use workbench_core::{EndpointId, ErrorDetail, MockServerId, ProjectId, ResourceId};
use workbench_domain::HistoryFilter;

use crate::extract::{ApiPath, ApiQuery};
use crate::runtime::StartRequest;
use crate::util::data;
use crate::{ApiError, ApiResult, AppState};

const DEFAULT_LOG_LIMIT: usize = 100;
const MAX_LOG_LIMIT: usize = 1_000;

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct LogQuery {
    method: Option<String>,
    status: Option<u16>,
    endpoint_id: Option<EndpointId>,
    resource_id: Option<ResourceId>,
    server_id: Option<MockServerId>,
    since: Option<DateTime<Utc>>,
    until: Option<DateTime<Utc>>,
    limit: Option<usize>,
}

impl LogQuery {
    fn filter(self) -> HistoryFilter {
        HistoryFilter {
            method: self.method,
            status: self.status,
            endpoint: self.endpoint_id,
            resource: self.resource_id,
            server: self.server_id,
            since: self.since,
            until: self.until,
            limit: Some(
                self.limit
                    .unwrap_or(DEFAULT_LOG_LIMIT)
                    .clamp(1, MAX_LOG_LIMIT),
            ),
        }
    }
}

/// Body (optional): `{ "environment_id"?, "host"?, "port"?,
/// "allow_network_exposure"?, "cors"?: { "enabled", "allowed_origins" } }`.
pub async fn start(
    State(s): State<AppState>,
    ApiPath(project_id): ApiPath<ProjectId>,
    body: Bytes,
) -> ApiResult<impl IntoResponse> {
    let req: StartRequest = if body.iter().all(u8::is_ascii_whitespace) {
        StartRequest::default()
    } else {
        serde_json::from_slice(&body)
            .map_err(|e| ApiError::validation(vec![ErrorDetail::new("body", e.to_string())]))?
    };
    let info = s.runtime.start(project_id, req).await?;
    Ok((StatusCode::CREATED, Json(data(info))))
}

pub async fn list_for_project(
    State(s): State<AppState>,
    ApiPath(project_id): ApiPath<ProjectId>,
) -> ApiResult<Json<Value>> {
    s.store.get_project(project_id).await?;
    Ok(Json(data(s.runtime.list(Some(project_id)).await)))
}

pub async fn list_all(State(s): State<AppState>) -> Json<Value> {
    Json(data(s.runtime.list(None).await))
}

pub async fn get(
    State(s): State<AppState>,
    ApiPath(id): ApiPath<MockServerId>,
) -> ApiResult<Json<Value>> {
    Ok(Json(data(s.runtime.get(id).await?)))
}

/// Re-reads the project's endpoints and resources into the running server.
/// Bind address, base path and default headers apply on the next start.
pub async fn reload(
    State(s): State<AppState>,
    ApiPath(id): ApiPath<MockServerId>,
) -> ApiResult<Json<Value>> {
    Ok(Json(data(s.runtime.reload(id).await?)))
}

pub async fn stop(
    State(s): State<AppState>,
    ApiPath(id): ApiPath<MockServerId>,
) -> ApiResult<StatusCode> {
    s.runtime.stop(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Request logs captured by one server, newest first.
pub async fn server_logs(
    State(s): State<AppState>,
    ApiPath(id): ApiPath<MockServerId>,
    ApiQuery(q): ApiQuery<LogQuery>,
) -> ApiResult<Json<Value>> {
    let info = s.runtime.get(id).await?;
    let mut filter = q.filter();
    filter.server = Some(id);
    Ok(Json(data(
        s.store.list_request_logs(info.project_id, &filter).await?,
    )))
}

/// Request logs of a project, newest first, filtered by method, status,
/// endpoint, resource, server and time.
pub async fn project_logs(
    State(s): State<AppState>,
    ApiPath(project_id): ApiPath<ProjectId>,
    ApiQuery(q): ApiQuery<LogQuery>,
) -> ApiResult<Json<Value>> {
    s.store.get_project(project_id).await?;
    Ok(Json(data(
        s.store.list_request_logs(project_id, &q.filter()).await?,
    )))
}

pub async fn clear_logs(
    State(s): State<AppState>,
    ApiPath(project_id): ApiPath<ProjectId>,
) -> ApiResult<Json<Value>> {
    s.store.get_project(project_id).await?;
    let deleted = s.store.clear_request_logs(project_id).await?;
    Ok(Json(data(json!({ "deleted": deleted }))))
}
