//! `/environments` and their secrets (spec §4.9, §8.6).

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::Deserialize;
use serde_json::Value;
use workbench_core::{EnvironmentId, ErrorDetail, ProjectId};
use workbench_domain::Environment;

use crate::extract::{ApiJson, ApiPath, ApiQuery};
use crate::util::{PageQuery, apply_patch, data, paginate};
use crate::{ApiError, ApiResult, AppState};

const FIXED: &[&str] = &["id", "project_id", "created_at", "updated_at"];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecretBody {
    value: String,
}

pub async fn list(
    State(s): State<AppState>,
    ApiPath(project_id): ApiPath<ProjectId>,
    ApiQuery(page): ApiQuery<PageQuery>,
) -> ApiResult<Json<Value>> {
    s.store.get_project(project_id).await?;
    Ok(Json(paginate(
        s.store.list_environments(project_id).await?,
        page,
    )?))
}

/// Body: `{ "name", "base_url"?, "default_headers"?, "variables"?, "bind"? }`.
pub async fn create(
    State(s): State<AppState>,
    ApiPath(project_id): ApiPath<ProjectId>,
    ApiJson(body): ApiJson<Value>,
) -> ApiResult<impl IntoResponse> {
    let name = body.get("name").and_then(Value::as_str).unwrap_or_default();
    let env = apply_patch(&Environment::new(project_id, name), &body, FIXED)?;
    s.store.create_environment(&env).await?;
    Ok((StatusCode::CREATED, Json(data(env))))
}

pub async fn get(
    State(s): State<AppState>,
    ApiPath(id): ApiPath<EnvironmentId>,
) -> ApiResult<Json<Value>> {
    Ok(Json(data(s.store.get_environment(id).await?)))
}

pub async fn update(
    State(s): State<AppState>,
    ApiPath(id): ApiPath<EnvironmentId>,
    ApiJson(body): ApiJson<Value>,
) -> ApiResult<Json<Value>> {
    let current = s.store.get_environment(id).await?;
    let env = s
        .store
        .update_environment(&apply_patch(&current, &body, FIXED)?)
        .await?;
    Ok(Json(data(env)))
}

pub async fn delete(
    State(s): State<AppState>,
    ApiPath(id): ApiPath<EnvironmentId>,
) -> ApiResult<StatusCode> {
    s.store.delete_environment(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Secret names and timestamps. Values are never returned.
pub async fn list_secrets(
    State(s): State<AppState>,
    ApiPath(id): ApiPath<EnvironmentId>,
) -> ApiResult<Json<Value>> {
    s.store.get_environment(id).await?;
    Ok(Json(data(s.store.list_secrets(id).await?)))
}

/// Stores a secret encrypted at rest. Write-only.
pub async fn put_secret(
    State(s): State<AppState>,
    ApiPath((id, name)): ApiPath<(EnvironmentId, String)>,
    ApiJson(body): ApiJson<SecretBody>,
) -> ApiResult<StatusCode> {
    s.store.get_environment(id).await?;
    if body.value.len() > 64 * 1024 {
        return Err(ApiError::validation(vec![ErrorDetail::new(
            "body.value",
            "must be at most 64 KiB",
        )]));
    }
    let sealed = s.secret_key.encrypt(id, &name, &body.value).map_err(|e| {
        tracing::error!(error = %e, "secret encryption failed");
        ApiError::internal()
    })?;
    s.store.put_secret(id, &name, &sealed).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn delete_secret(
    State(s): State<AppState>,
    ApiPath((id, name)): ApiPath<(EnvironmentId, String)>,
) -> ApiResult<StatusCode> {
    s.store.delete_secret(id, &name).await?;
    Ok(StatusCode::NO_CONTENT)
}
