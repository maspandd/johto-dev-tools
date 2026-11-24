//! Project archives (spec §4.10, §8.2): export a project as a `.zip`,
//! and import one as a new project.

use axum::Json;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use project_archive::{
    ArchiveError, DatasetFile, EnvironmentFile, Limits, ProjectBundle, ProjectFile, ResourceEntry,
};
use serde::Deserialize;
use serde_json::{Value, json};
use workbench_core::{ErrorDetail, ProjectId, WorkspaceId, time};
use workbench_domain::{Dataset, Environment, Project, StoredRecord, key_of};
use workbench_storage::ProjectTree;

use crate::extract::{ApiPath, ApiQuery};
use crate::routes::openapi::file_slug;
use crate::util::data;
use crate::{ApiError, ApiResult, AppState};

const GENERATOR: &str = concat!("developer-workbench ", env!("CARGO_PKG_VERSION"));

#[derive(Deserialize)]
pub struct ExportQuery {
    /// Include CRUD resource records. Default: false (resources are
    /// seeded again on import).
    #[serde(default)]
    records: bool,
}

/// `GET /projects/{id}/export?records=` — a `.zip` archive. Secret values
/// are never included; environments list secret names only.
pub async fn export(
    State(s): State<AppState>,
    ApiPath(project_id): ApiPath<ProjectId>,
    ApiQuery(q): ApiQuery<ExportQuery>,
) -> ApiResult<Response> {
    let project = s.store.get_project(project_id).await?;
    let mut environments = Vec::new();
    for env in s.store.list_environments(project_id).await? {
        let secret_names = s
            .store
            .list_secrets(env.id)
            .await?
            .into_iter()
            .map(|m| m.name)
            .collect();
        environments.push(EnvironmentFile {
            name: env.name,
            base_url: env.base_url,
            default_headers: env.default_headers,
            variables: env.variables,
            bind: env.bind,
            secret_names,
        });
    }
    let mut resources = Vec::new();
    for resource in s.store.list_resources(project_id).await? {
        let records = if q.records {
            match s.runtime.resource_records(project_id, resource.id).await {
                Some(live) => Some(live),
                None => s
                    .store
                    .load_resource_records(resource.id)
                    .await?
                    .map(|rows| rows.into_iter().map(|r| r.data).collect()),
            }
        } else {
            None
        };
        resources.push(ResourceEntry { resource, records });
    }
    let datasets = s
        .store
        .list_datasets(project_id)
        .await?
        .into_iter()
        .map(|d| DatasetFile {
            name: d.name,
            description: d.description,
            schema: d.schema,
            count: d.count,
            seed: d.seed,
            locale: d.locale,
        })
        .collect();
    let bundle = ProjectBundle {
        project: ProjectFile {
            name: project.name.clone(),
            description: project.description,
            base_path: project.base_path,
            default_headers: project.default_headers,
        },
        environments,
        endpoints: s.store.list_endpoints(project_id).await?,
        resources,
        datasets,
    };
    let bytes = project_archive::write(&bundle, time::now(), GENERATOR).map_err(|e| {
        tracing::error!(error = %e, "archive export failed");
        ApiError::internal()
    })?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/zip".to_owned()),
            (
                header::CONTENT_DISPOSITION,
                format!(
                    "attachment; filename=\"{}.workbench.zip\"",
                    file_slug(&project.name)
                ),
            ),
        ],
        bytes,
    )
        .into_response())
}

#[derive(Deserialize)]
pub struct ImportQuery {
    workspace_id: Option<WorkspaceId>,
    /// Validate and describe the archive without saving anything.
    #[serde(default)]
    preview: bool,
    /// Name for the new project; default: the archive's.
    name: Option<String>,
}

fn archive_error(e: ArchiveError) -> ApiError {
    let details = match e {
        ArchiveError::Invalid(problems) => problems
            .into_iter()
            .map(|p| ErrorDetail::new("archive", p))
            .collect(),
        other => vec![ErrorDetail::new("archive", other.to_string())],
    };
    ApiError::validation(details)
}

/// `POST /projects/import?workspace_id=&preview=&name=` — body: the
/// `.zip` bytes. Creates a new project with new IDs in one transaction.
pub async fn import(
    State(s): State<AppState>,
    ApiQuery(q): ApiQuery<ImportQuery>,
    body: Bytes,
) -> ApiResult<Response> {
    let Some(workspace_id) = q.workspace_id else {
        return Err(ApiError::validation(vec![ErrorDetail::new(
            "query.workspace_id",
            "is required",
        )]));
    };
    s.store.get_workspace(workspace_id).await?;
    if body.is_empty() {
        return Err(ApiError::validation(vec![ErrorDetail::new(
            "body",
            "send the archive (.zip) as the request body",
        )]));
    }
    let limits = Limits {
        max_archive_bytes: s.config.max_body_bytes,
        ..Limits::default()
    };
    let archive = project_archive::read(&body, &limits).map_err(archive_error)?;
    let bundle = archive.bundle;
    let secrets: Vec<Value> = bundle
        .environments
        .iter()
        .filter(|e| !e.secret_names.is_empty())
        .map(|e| json!({ "environment": e.name, "names": e.secret_names }))
        .collect();

    if q.preview {
        let endpoints: Vec<Value> = bundle
            .endpoints
            .iter()
            .map(|e| json!({ "name": e.name, "method": e.method, "path": e.path, "enabled": e.enabled }))
            .collect();
        return Ok(Json(data(json!({
            "manifest": archive.manifest,
            "project": bundle.project,
            "environments": bundle.environments.iter().map(|e| &e.name).collect::<Vec<_>>(),
            "endpoints": endpoints,
            "resources": bundle.resources.iter().map(|r| json!({
                "name": r.resource.name,
                "path": r.resource.path,
                "records": r.records.as_ref().map(Vec::len),
            })).collect::<Vec<_>>(),
            "datasets": bundle.datasets.iter().map(|d| &d.name).collect::<Vec<_>>(),
            "secrets_to_set": secrets,
            "warnings": archive.warnings,
        })))
        .into_response());
    }

    let mut project = Project::new(
        workspace_id,
        q.name.as_deref().unwrap_or(&bundle.project.name),
    );
    project.description = bundle.project.description;
    project.base_path = bundle.project.base_path;
    project.default_headers = bundle.project.default_headers;
    let environments = bundle
        .environments
        .into_iter()
        .map(|e| {
            let mut env = Environment::new(project.id, e.name);
            env.base_url = e.base_url;
            env.default_headers = e.default_headers;
            env.variables = e.variables;
            env.bind = e.bind;
            env
        })
        .collect();
    let resources = bundle
        .resources
        .into_iter()
        .map(|entry| {
            let pk = entry.resource.primary_key.clone();
            let records = entry.records.map(|rows| {
                rows.into_iter()
                    .enumerate()
                    .map(|(i, data)| StoredRecord {
                        key: data.get(&pk).and_then(key_of).unwrap_or_default(),
                        position: i64::try_from(i).unwrap_or(i64::MAX),
                        data,
                    })
                    .collect()
            });
            (entry.resource, records)
        })
        .collect();
    let datasets = bundle
        .datasets
        .into_iter()
        .map(|d| {
            let mut ds = Dataset::new(project.id, d.name, d.schema, d.count);
            ds.description = d.description;
            ds.seed = d.seed;
            ds.locale = d.locale;
            ds
        })
        .collect();
    s.store
        .insert_project_tree(&ProjectTree {
            project: project.clone(),
            environments,
            endpoints: bundle.endpoints,
            resources,
            datasets,
        })
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(data(json!({
            "project": project,
            "secrets_to_set": secrets,
            "warnings": archive.warnings,
        }))),
    )
        .into_response())
}
