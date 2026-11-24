//! Persisted request history (spec §4.6).
//!
//! Records arrive already redacted by the runtime. Summary fields are
//! columns for filtering; headers, query and bodies are one JSON value.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::types::Json;
use uuid::Uuid;
use workbench_core::{EndpointId, MockServerId, ProjectId, RequestId, ResourceId, time};
use workbench_domain::{CapturedBody, HistoryFilter, HistoryRecord};

use crate::error::map_constraint;
use crate::store::with_pool;
use crate::{StorageError, StorageResult, Store};

const ENTITY: &str = "request log";

#[derive(Serialize, Deserialize)]
struct Details {
    query: Vec<(String, String)>,
    request_headers: Vec<(String, String)>,
    request_body: Option<CapturedBody>,
    response_headers: Vec<(String, String)>,
    response_body: Option<CapturedBody>,
    #[serde(default)]
    matched_resource: Option<ResourceId>,
}

#[derive(sqlx::FromRow)]
struct RequestLogRow {
    id: Uuid,
    server_id: Uuid,
    endpoint_id: Option<Uuid>,
    timestamp: DateTime<Utc>,
    method: String,
    path: String,
    status: i32,
    duration_ms: i64,
    matched_rule: Option<String>,
    details: Json<Details>,
}

impl TryFrom<RequestLogRow> for HistoryRecord {
    type Error = StorageError;

    fn try_from(r: RequestLogRow) -> Result<Self, Self::Error> {
        let status = u16::try_from(r.status)
            .map_err(|_| StorageError::corrupt(ENTITY, r.id, "status out of range"))?;
        let duration_ms = u64::try_from(r.duration_ms)
            .map_err(|_| StorageError::corrupt(ENTITY, r.id, "negative duration"))?;
        let d = r.details.0;
        Ok(Self {
            id: RequestId::from_uuid(r.id),
            server_id: MockServerId::from_uuid(r.server_id),
            timestamp: r.timestamp,
            method: r.method,
            path: r.path,
            query: d.query,
            request_headers: d.request_headers,
            request_body: d.request_body,
            matched_endpoint: r.endpoint_id.map(EndpointId::from_uuid),
            matched_rule: r.matched_rule,
            matched_resource: d.matched_resource,
            status,
            duration_ms,
            response_headers: d.response_headers,
            response_body: d.response_body,
        })
    }
}

fn clamp_i64(n: usize) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

impl Store {
    /// Stores one captured request for a project.
    pub async fn insert_request_log(
        &self,
        project_id: ProjectId,
        r: &HistoryRecord,
    ) -> StorageResult<()> {
        let details = Json(Details {
            query: r.query.clone(),
            request_headers: r.request_headers.clone(),
            request_body: r.request_body.clone(),
            response_headers: r.response_headers.clone(),
            response_body: r.response_body.clone(),
            matched_resource: r.matched_resource,
        });
        with_pool!(self, |pool| sqlx::query(
            "INSERT INTO request_logs (id, project_id, server_id, endpoint_id, timestamp, method, \
             path, status, duration_ms, matched_rule, details, resource_id) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)"
        )
        .bind(*r.id.as_uuid())
        .bind(*project_id.as_uuid())
        .bind(*r.server_id.as_uuid())
        .bind(r.matched_endpoint.map(|e| *e.as_uuid()))
        .bind(time::truncate(r.timestamp))
        .bind(&r.method)
        .bind(&r.path)
        .bind(i32::from(r.status))
        .bind(i64::try_from(r.duration_ms).unwrap_or(i64::MAX))
        .bind(r.matched_rule.as_deref())
        .bind(&details)
        .bind(r.matched_resource.map(|x| *x.as_uuid()))
        .execute(pool)
        .await
        .map_err(|e| map_constraint(
            e,
            || StorageError::Conflict(format!("request log {} already exists", r.id)),
            || StorageError::not_found("project", project_id),
        ))?
        .rows_affected());
        Ok(())
    }

    /// Matching logs for a project, newest first.
    pub async fn list_request_logs(
        &self,
        project_id: ProjectId,
        filter: &HistoryFilter,
    ) -> StorageResult<Vec<HistoryRecord>> {
        let rows: Vec<RequestLogRow> = with_pool!(self, |pool| sqlx::query_as(
            "SELECT id, server_id, endpoint_id, timestamp, method, path, status, duration_ms, \
             matched_rule, details FROM request_logs \
             WHERE project_id = $1 \
             AND ($2 IS NULL OR method = $2) \
             AND ($3 IS NULL OR status = $3) \
             AND ($4 IS NULL OR endpoint_id = $4) \
             AND ($5 IS NULL OR timestamp >= $5) \
             AND ($6 IS NULL OR timestamp <= $6) \
             AND ($8 IS NULL OR resource_id = $8) \
             AND ($9 IS NULL OR server_id = $9) \
             ORDER BY timestamp DESC, id DESC LIMIT $7"
        )
        .bind(*project_id.as_uuid())
        .bind(filter.method.as_deref().map(str::to_ascii_uppercase))
        .bind(filter.status.map(i32::from))
        .bind(filter.endpoint.map(|e| *e.as_uuid()))
        .bind(filter.since.map(time::truncate))
        .bind(filter.until.map(time::truncate))
        .bind(filter.limit.map_or(i64::MAX, clamp_i64))
        .bind(filter.resource.map(|x| *x.as_uuid()))
        .bind(filter.server.map(|s| *s.as_uuid()))
        .fetch_all(pool)
        .await?);
        rows.into_iter().map(HistoryRecord::try_from).collect()
    }

    /// Deletes every log of a project; returns how many were removed.
    pub async fn clear_request_logs(&self, project_id: ProjectId) -> StorageResult<u64> {
        Ok(with_pool!(self, |pool| sqlx::query(
            "DELETE FROM request_logs WHERE project_id = $1"
        )
        .bind(*project_id.as_uuid())
        .execute(pool)
        .await?
        .rows_affected()))
    }

    /// Keeps only the newest `keep` logs of a project; returns how many were removed.
    pub async fn prune_request_logs(
        &self,
        project_id: ProjectId,
        keep: usize,
    ) -> StorageResult<u64> {
        Ok(with_pool!(self, |pool| sqlx::query(
            "DELETE FROM request_logs WHERE project_id = $1 AND id NOT IN \
             (SELECT id FROM request_logs WHERE project_id = $1 \
              ORDER BY timestamp DESC, id DESC LIMIT $2)"
        )
        .bind(*project_id.as_uuid())
        .bind(clamp_i64(keep))
        .execute(pool)
        .await?
        .rows_affected()))
    }
}
