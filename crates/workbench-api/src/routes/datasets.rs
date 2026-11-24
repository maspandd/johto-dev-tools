//! `/datasets`: saved generator configurations, preview and export (spec §4.4).

use std::collections::HashMap;

use axum::Json;
use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::Value;
use workbench_core::{DatasetId, ErrorDetail, ProjectId};
use workbench_domain::{DataSchema, Dataset, Locale};

use crate::extract::{ApiJson, ApiPath, ApiQuery};
use crate::util::{PageQuery, apply_patch, apply_replace, data, paginate};
use crate::{ApiError, ApiResult, AppState};

const FIXED: &[&str] = &["id", "project_id", "created_at", "updated_at"];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewBody {
    schema: DataSchema,
    #[serde(default = "twenty")]
    count: usize,
    #[serde(default)]
    seed: u64,
    #[serde(default)]
    locale: Locale,
}

const fn twenty() -> usize {
    20
}

#[derive(Deserialize)]
pub struct ExportQuery {
    #[serde(default)]
    format: ExportFormat,
}

#[derive(Deserialize, Default, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    #[default]
    Json,
    Csv,
}

/// Current records of the project's resources that `schema` references.
async fn references(
    s: &AppState,
    project_id: ProjectId,
    schema: &DataSchema,
) -> ApiResult<HashMap<String, Vec<Value>>> {
    let wanted = schema.referenced_resources();
    let mut refs = HashMap::new();
    if wanted.is_empty() {
        return Ok(refs);
    }
    for r in s.store.list_resources(project_id).await? {
        if !wanted.contains(&r.name.as_str()) {
            continue;
        }
        let rows = match s.runtime.resource_records(project_id, r.id).await {
            Some(rows) => rows,
            None => s
                .store
                .load_resource_records(r.id)
                .await?
                .unwrap_or_default()
                .into_iter()
                .map(|rec| rec.data)
                .collect(),
        };
        refs.insert(r.name, rows);
    }
    Ok(refs)
}

async fn generate(
    s: &AppState,
    project_id: ProjectId,
    schema: &DataSchema,
    count: usize,
    seed: u64,
    locale: Locale,
    preview: bool,
) -> ApiResult<Vec<Value>> {
    let owned = references(s, project_id, schema).await?;
    let mut refs = data_generator::References::new();
    for (name, rows) in &owned {
        refs.insert(name, rows);
    }
    let result = if preview {
        data_generator::preview(schema, count, seed, locale, &refs)
    } else {
        data_generator::generate(schema, count, seed, locale, &refs)
    };
    result.map_err(|e| ApiError::validation(vec![ErrorDetail::new("schema", e.to_string())]))
}

pub async fn list(
    State(s): State<AppState>,
    ApiPath(project_id): ApiPath<ProjectId>,
    ApiQuery(page): ApiQuery<PageQuery>,
) -> ApiResult<Json<Value>> {
    s.store.get_project(project_id).await?;
    Ok(Json(paginate(
        s.store.list_datasets(project_id).await?,
        page,
    )?))
}

/// Body: `{ "name", "schema", "count", "seed"?, "locale"?, "description"? }`.
pub async fn create(
    State(s): State<AppState>,
    ApiPath(project_id): ApiPath<ProjectId>,
    ApiJson(body): ApiJson<Value>,
) -> ApiResult<impl IntoResponse> {
    let name = body.get("name").and_then(Value::as_str).unwrap_or_default();
    let dataset = apply_patch(
        &Dataset::new(project_id, name, DataSchema::default(), 0),
        &body,
        FIXED,
    )?;
    s.store.create_dataset(&dataset).await?;
    Ok((StatusCode::CREATED, Json(data(dataset))))
}

pub async fn get(
    State(s): State<AppState>,
    ApiPath(id): ApiPath<DatasetId>,
) -> ApiResult<Json<Value>> {
    Ok(Json(data(s.store.get_dataset(id).await?)))
}

pub async fn update(
    State(s): State<AppState>,
    ApiPath(id): ApiPath<DatasetId>,
    ApiJson(body): ApiJson<Value>,
) -> ApiResult<Json<Value>> {
    let current = s.store.get_dataset(id).await?;
    Ok(Json(data(
        s.store
            .update_dataset(&apply_patch(&current, &body, FIXED)?)
            .await?,
    )))
}

/// Replaces the whole definition (body as for create).
pub async fn replace(
    State(s): State<AppState>,
    ApiPath(id): ApiPath<DatasetId>,
    ApiJson(body): ApiJson<Value>,
) -> ApiResult<Json<Value>> {
    let current = s.store.get_dataset(id).await?;
    Ok(Json(data(
        s.store
            .update_dataset(&apply_replace(&current, &body, FIXED)?)
            .await?,
    )))
}

pub async fn delete(
    State(s): State<AppState>,
    ApiPath(id): ApiPath<DatasetId>,
) -> ApiResult<StatusCode> {
    s.store.delete_dataset(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// The first records of a saved dataset.
pub async fn preview(
    State(s): State<AppState>,
    ApiPath(id): ApiPath<DatasetId>,
) -> ApiResult<Json<Value>> {
    let d = s.store.get_dataset(id).await?;
    let rows = generate(&s, d.project_id, &d.schema, d.count, d.seed, d.locale, true).await?;
    Ok(Json(data(rows)))
}

/// Previews an unsaved configuration ("preview before saving").
pub async fn preview_unsaved(
    State(s): State<AppState>,
    ApiPath(project_id): ApiPath<ProjectId>,
    ApiJson(body): ApiJson<PreviewBody>,
) -> ApiResult<Json<Value>> {
    s.store.get_project(project_id).await?;
    let rows = generate(
        &s,
        project_id,
        &body.schema,
        body.count,
        body.seed,
        body.locale,
        true,
    )
    .await?;
    Ok(Json(data(rows)))
}

/// The full dataset as a JSON array or CSV file download.
pub async fn export(
    State(s): State<AppState>,
    ApiPath(id): ApiPath<DatasetId>,
    ApiQuery(q): ApiQuery<ExportQuery>,
) -> ApiResult<Response> {
    let d = s.store.get_dataset(id).await?;
    let rows = generate(
        &s,
        d.project_id,
        &d.schema,
        d.count,
        d.seed,
        d.locale,
        false,
    )
    .await?;
    let safe: String = d
        .name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let (body, content_type, ext) = match q.format {
        ExportFormat::Json => (
            serde_json::to_string_pretty(&rows).map_err(|_| ApiError::internal())?,
            "application/json",
            "json",
        ),
        ExportFormat::Csv => (
            data_generator::to_csv(&rows),
            "text/csv; charset=utf-8",
            "csv",
        ),
    };
    Ok((
        [
            (header::CONTENT_TYPE, content_type.to_owned()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{safe}.{ext}\""),
            ),
        ],
        body,
    )
        .into_response())
}
