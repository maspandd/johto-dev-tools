//! API errors, always returned as the shared envelope (spec §8.7):
//! `{ "error": { "code", "message", "details", "request_id" } }`.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use mock_runtime::StartError;
use workbench_core::{BindError, ErrorCode, ErrorDetail, ErrorEnvelope};
use workbench_storage::StorageError;

use crate::middleware::current_request_id;

#[derive(Debug)]
pub struct ApiError {
    pub code: ErrorCode,
    pub message: String,
    pub details: Vec<ErrorDetail>,
}

pub type ApiResult<T> = Result<T, ApiError>;

impl ApiError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_details(mut self, details: Vec<ErrorDetail>) -> Self {
        self.details = details;
        self
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::BadRequest, message)
    }

    pub fn validation(details: Vec<ErrorDetail>) -> Self {
        Self::new(
            ErrorCode::RequestValidationFailed,
            "The request did not pass validation.",
        )
        .with_details(details)
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Conflict, message)
    }

    pub fn internal() -> Self {
        Self::new(ErrorCode::InternalError, "An internal error occurred.")
    }

    pub fn not_found(entity: &str, id: impl std::fmt::Display) -> Self {
        let code = match entity {
            "workspace" => ErrorCode::WorkspaceNotFound,
            "project" => ErrorCode::ProjectNotFound,
            "environment" => ErrorCode::EnvironmentNotFound,
            "endpoint" => ErrorCode::EndpointNotFound,
            "resource" => ErrorCode::ResourceNotFound,
            "dataset" => ErrorCode::DatasetNotFound,
            "mock server" => ErrorCode::MockServerNotFound,
            "secret" => ErrorCode::SecretNotFound,
            _ => ErrorCode::NotFound,
        };
        Self::new(
            code,
            format!("The requested {entity} `{id}` does not exist."),
        )
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = StatusCode::from_u16(self.code.http_status())
            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let mut envelope = ErrorEnvelope::new(self.code, self.message).with_details(self.details);
        if let Some(id) = current_request_id() {
            envelope = envelope.with_request_id(id);
        }
        (status, Json(envelope)).into_response()
    }
}

impl From<StorageError> for ApiError {
    fn from(err: StorageError) -> Self {
        match err {
            StorageError::NotFound { entity, id } => Self::not_found(entity, id),
            StorageError::Invalid { errors, .. } => Self::validation(
                errors
                    .into_iter()
                    .map(|e| ErrorDetail::new(e.field, e.message))
                    .collect(),
            ),
            StorageError::Conflict(message) => Self::conflict(message),
            other => {
                // Never leak database details to clients.
                tracing::error!(error = %other, "storage failure");
                Self::internal()
            }
        }
    }
}

impl From<StartError> for ApiError {
    fn from(err: StartError) -> Self {
        match err {
            StartError::Bind(BindError::NetworkExposureNotAllowed { host }) => {
                Self::validation(vec![ErrorDetail::new(
                    "host",
                    format!("binding to {host} requires allow_network_exposure"),
                )])
            }
            StartError::PortInUse { .. } => Self::conflict(err.to_string()),
            StartError::Build(errors) => Self::validation(
                errors
                    .0
                    .iter()
                    .map(|p| ErrorDetail::new("project", p.to_string()))
                    .collect(),
            ),
            StartError::Seed(e) => Self::validation(vec![ErrorDetail::new("seed", e.to_string())]),
            other => {
                tracing::error!(error = %other, "mock server failed to start");
                Self::new(ErrorCode::InternalError, other.to_string())
            }
        }
    }
}
