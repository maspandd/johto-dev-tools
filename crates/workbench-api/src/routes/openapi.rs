//! OpenAPI import and export (spec §4.7, §8.5) and cURL parsing (§4.8).

use std::collections::HashSet;

use axum::Json;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use openapi_engine::curl::{CurlOptions, CurlRequest};
use openapi_engine::{ApiDefinition, Issue, analyze, parse_document};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use workbench_core::{ErrorDetail, ProjectId};
use workbench_domain::{Endpoint, Method, PathTemplate, Resource};

use crate::extract::{ApiJson, ApiPath, ApiQuery};
use crate::routes::endpoints::EndpointDto;
use crate::util::data;
use crate::{ApiError, ApiResult, AppState};

fn issue_details(issues: &[Issue]) -> Vec<ErrorDetail> {
    issues
        .iter()
        .map(|i| ErrorDetail::new(format!("document{}", i.pointer), i.message.clone()))
        .collect()
}

fn parse_text(bytes: &[u8], location: &str) -> ApiResult<Value> {
    let text = std::str::from_utf8(bytes).map_err(|_| {
        ApiError::validation(vec![ErrorDetail::new(location, "must be UTF-8 text")])
    })?;
    parse_document(text)
        .map_err(|e| ApiError::validation(vec![ErrorDetail::new(location, e.to_string())]))
}

/// `POST /openapi/validate` — body: the document as JSON or YAML text.
/// Always answers 200 with `valid` and every issue found.
pub async fn validate(body: Bytes) -> ApiResult<Json<Value>> {
    let invalid = |issues: Vec<Issue>| {
        Json(data(
            json!({ "valid": false, "operations": 0, "importable": 0, "issues": issues }),
        ))
    };
    let doc = match std::str::from_utf8(&body)
        .map_err(|_| "the document must be UTF-8 text".to_owned())
        .and_then(|t| parse_document(t).map_err(|e| e.to_string()))
    {
        Ok(doc) => doc,
        Err(message) => return Ok(invalid(vec![Issue::error("", message)])),
    };
    let analysis = match analyze(&doc) {
        Ok(a) => a,
        Err(issues) => return Ok(invalid(issues)),
    };
    let issues: Vec<&Issue> = analysis.all_issues().collect();
    Ok(Json(data(json!({
        "valid": !analysis.has_errors(),
        "openapi": analysis.openapi,
        "title": analysis.title,
        "version": analysis.version,
        "base_path": analysis.base_path,
        "operations": analysis.operations.len(),
        "importable": analysis.operations.iter().filter(|o| o.endpoint.is_some()).count(),
        "issues": issues,
    }))))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportBody {
    /// JSON or YAML text, or the document as a JSON object.
    document: Value,
    /// Operation keys (`GET /users/{id}`) or operation IDs. Default: all.
    operations: Option<Vec<String>>,
    /// Preview only: nothing is saved.
    #[serde(default)]
    dry_run: bool,
    /// Use the document's server path as the project base path when the
    /// project has none. Default: true.
    set_base_path: Option<bool>,
}

#[derive(Serialize)]
struct PreviewOperation<'a> {
    #[serde(flatten)]
    operation: &'a openapi_engine::Operation,
    importable: bool,
    /// Answers the same requests as an existing endpoint or resource; it
    /// would be imported disabled.
    conflicts_with_project: bool,
}

/// `(method, shape)` pairs already answered by the project.
async fn taken_routes(s: &AppState, project_id: ProjectId) -> ApiResult<HashSet<(Method, String)>> {
    let mut taken: HashSet<(Method, String)> = s
        .store
        .list_endpoints(project_id)
        .await?
        .into_iter()
        .filter(|e| e.enabled)
        .map(|e| (e.method, e.path.shape()))
        .collect();
    for r in s.store.list_resources(project_id).await? {
        if !r.enabled {
            continue;
        }
        taken.extend(resource_routes(&r));
    }
    Ok(taken)
}

fn resource_routes(r: &Resource) -> Vec<(Method, String)> {
    let collection = r.path.shape();
    let item = PathTemplate::parse(&format!("{}/:id", r.path))
        .map(|t| t.shape())
        .unwrap_or_default();
    vec![
        (Method::Get, collection.clone()),
        (Method::Post, collection),
        (Method::Get, item.clone()),
        (Method::Put, item.clone()),
        (Method::Patch, item.clone()),
        (Method::Delete, item),
    ]
}

/// `POST /projects/{id}/openapi/import`.
/// Body: `{ "document", "operations"?, "dry_run"?, "set_base_path"? }`.
/// Endpoints that answer the same requests as existing ones are imported
/// disabled. Everything is saved in one transaction.
pub async fn import(
    State(s): State<AppState>,
    ApiPath(project_id): ApiPath<ProjectId>,
    ApiJson(body): ApiJson<ImportBody>,
) -> ApiResult<Response> {
    let mut project = s.store.get_project(project_id).await?;
    let doc = match body.document {
        Value::String(text) => parse_text(text.as_bytes(), "body.document")?,
        doc @ Value::Object(_) => doc,
        _ => {
            return Err(ApiError::validation(vec![ErrorDetail::new(
                "body.document",
                "must be OpenAPI text (JSON or YAML) or a JSON object",
            )]));
        }
    };
    let analysis = analyze(&doc).map_err(|issues| ApiError::validation(issue_details(&issues)))?;
    let taken = taken_routes(&s, project_id).await?;
    let conflicts = |e: &Endpoint| taken.contains(&(e.method, e.path.shape()));

    if body.dry_run {
        let operations: Vec<PreviewOperation<'_>> = analysis
            .operations
            .iter()
            .map(|o| PreviewOperation {
                operation: o,
                importable: o.endpoint.is_some(),
                conflicts_with_project: o.endpoint.as_ref().is_some_and(conflicts),
            })
            .collect();
        return Ok(Json(data(json!({
            "openapi": analysis.openapi,
            "title": analysis.title,
            "description": analysis.description,
            "version": analysis.version,
            "base_path": analysis.base_path,
            "operations": operations,
            "issues": analysis.issues,
        })))
        .into_response());
    }

    let mut endpoints = analysis
        .endpoints(body.operations.as_deref())
        .map_err(|issues| ApiError::validation(issue_details(&issues)))?;
    if endpoints.is_empty() {
        return Err(ApiError::validation(vec![ErrorDetail::new(
            "body.document",
            "no operations can be imported",
        )]));
    }
    let mut disabled = Vec::new();
    for e in &mut endpoints {
        if e.enabled && conflicts(e) {
            e.enabled = false;
            disabled.push(e.name.clone());
        }
    }
    s.store.create_endpoints(project_id, &endpoints).await?;

    let mut base_path_set = None;
    if body.set_base_path.unwrap_or(true)
        && project.base_path.is_none()
        && let Some(base) = &analysis.base_path
    {
        project.base_path = Some(base.clone());
        s.store.update_project(&project).await?;
        base_path_set = Some(base.clone());
    }
    s.runtime.refresh_project(project_id).await;

    let mut imported = Vec::new();
    for e in &endpoints {
        imported.push(EndpointDto::from(s.store.get_endpoint(e.id).await?));
    }
    let issues: Vec<&Issue> = analysis.all_issues().collect();
    Ok((
        StatusCode::CREATED,
        Json(data(json!({
            "imported": imported,
            "disabled_due_to_conflicts": disabled,
            "base_path_set": base_path_set,
            "issues": issues,
        }))),
    )
        .into_response())
}

#[derive(Deserialize, Default, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DocFormat {
    #[default]
    Yaml,
    Json,
}

#[derive(Deserialize)]
pub struct ExportQuery {
    #[serde(default)]
    format: DocFormat,
    /// `info.version`; default `1.0.0`.
    version: Option<String>,
}

pub(crate) fn file_slug(name: &str) -> String {
    let slug: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "project".to_owned()
    } else {
        slug.to_owned()
    }
}

/// `GET /projects/{id}/openapi/export?format=yaml|json&version=`.
pub async fn export(
    State(s): State<AppState>,
    ApiPath(project_id): ApiPath<ProjectId>,
    ApiQuery(q): ApiQuery<ExportQuery>,
) -> ApiResult<Response> {
    let project = s.store.get_project(project_id).await?;
    let endpoints = s.store.list_endpoints(project_id).await?;
    let resources = s.store.list_resources(project_id).await?;
    let doc = openapi_engine::export(&ApiDefinition {
        title: &project.name,
        description: &project.description,
        version: q.version.as_deref().unwrap_or("1.0.0"),
        base_path: project.base_path.as_deref(),
        endpoints: &endpoints,
        resources: &resources,
    });
    let (body, content_type, ext) = match q.format {
        DocFormat::Yaml => (openapi_engine::to_yaml(&doc), "application/yaml", "yaml"),
        DocFormat::Json => (openapi_engine::to_json(&doc), "application/json", "json"),
    };
    Ok((
        [
            (header::CONTENT_TYPE, content_type.to_owned()),
            (
                header::CONTENT_DISPOSITION,
                format!(
                    "attachment; filename=\"{}-openapi.{ext}\"",
                    file_slug(&project.name)
                ),
            ),
        ],
        body,
    )
        .into_response())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CurlBody {
    command: String,
    /// Keep credential values instead of redacting them. Default: false.
    #[serde(default)]
    keep_credentials: bool,
    /// Strip this project's base path from the endpoint draft.
    project_id: Option<ProjectId>,
}

#[derive(Serialize)]
struct CurlPreview {
    request: CurlRequest,
    /// Ready to `POST /projects/{id}/endpoints` after editing.
    endpoint: Option<Endpoint>,
    #[serde(skip_serializing_if = "Option::is_none")]
    endpoint_error: Option<String>,
}

/// `POST /curl/parse` — body: `{ "command", "keep_credentials"?, "project_id"? }`.
/// The command is never executed and nothing is saved.
pub async fn parse_curl(
    State(s): State<AppState>,
    ApiJson(body): ApiJson<CurlBody>,
) -> ApiResult<Json<Value>> {
    let request = openapi_engine::curl::parse(
        &body.command,
        CurlOptions {
            keep_credentials: body.keep_credentials,
        },
    )
    .map_err(|e| ApiError::validation(vec![ErrorDetail::new("body.command", e.to_string())]))?;
    let base_path = match body.project_id {
        Some(id) => s.store.get_project(id).await?.base_path,
        None => None,
    };
    let (endpoint, endpoint_error) = match request.to_endpoint(base_path.as_deref()) {
        Ok(e) => (Some(e), None),
        Err(e) => (None, Some(e)),
    };
    Ok(Json(data(CurlPreview {
        request,
        endpoint,
        endpoint_error,
    })))
}
