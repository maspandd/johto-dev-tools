//! Extractors whose failures use the API error envelope.

use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
use axum::extract::{FromRequest, FromRequestParts};
use workbench_core::{ErrorCode, ErrorDetail};

use crate::ApiError;

/// JSON body. Malformed JSON is `400 INVALID_JSON_BODY`; JSON with the
/// wrong shape is `422 REQUEST_VALIDATION_FAILED`; oversized bodies are `413`.
#[derive(FromRequest)]
#[from_request(via(axum::Json), rejection(ApiError))]
pub struct ApiJson<T>(pub T);

/// Path parameters. Unparseable IDs are `400 BAD_REQUEST`.
#[derive(FromRequestParts)]
#[from_request(via(axum::extract::Path), rejection(ApiError))]
pub struct ApiPath<T>(pub T);

/// Query parameters. Invalid values are `422 REQUEST_VALIDATION_FAILED`.
#[derive(FromRequestParts)]
#[from_request(via(axum::extract::Query), rejection(ApiError))]
pub struct ApiQuery<T>(pub T);

impl From<JsonRejection> for ApiError {
    fn from(r: JsonRejection) -> Self {
        match r {
            JsonRejection::JsonDataError(e) => {
                Self::validation(vec![ErrorDetail::new("body", e.body_text())])
            }
            JsonRejection::JsonSyntaxError(e) => Self::new(
                ErrorCode::InvalidJsonBody,
                "The request body is not valid JSON.",
            )
            .with_details(vec![ErrorDetail::new("body", e.body_text())]),
            JsonRejection::MissingJsonContentType(_) => Self::new(
                ErrorCode::InvalidJsonBody,
                "Send the body with `Content-Type: application/json`.",
            ),
            JsonRejection::BytesRejection(e) if e.status().as_u16() == 413 => {
                Self::new(ErrorCode::PayloadTooLarge, "The request body is too large.")
            }
            other => Self::bad_request(other.body_text()),
        }
    }
}

impl From<PathRejection> for ApiError {
    fn from(r: PathRejection) -> Self {
        Self::bad_request(format!("Invalid path parameter: {}", r.body_text()))
    }
}

impl From<QueryRejection> for ApiError {
    fn from(r: QueryRejection) -> Self {
        Self::validation(vec![ErrorDetail::new("query", r.body_text())])
    }
}
