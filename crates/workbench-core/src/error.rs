//! Machine-readable error codes and the JSON error envelope shared by the
//! management API and the mock runtime (spec §8.7).
//!
//! ```json
//! { "error": { "code": "ROUTE_NOT_FOUND", "message": "...", "details": [], "request_id": "..." } }
//! ```

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::RequestId;

/// Stable error codes. Serialized as `SCREAMING_SNAKE_CASE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[non_exhaustive]
pub enum ErrorCode {
    /// No enabled mock route matches the request path.
    RouteNotFound,
    /// A route matches the path but not the HTTP method.
    MethodNotAllowed,
    /// The request body is not valid JSON.
    InvalidJsonBody,
    /// The request failed parameter or schema validation.
    RequestValidationFailed,
    /// The request body exceeds the configured size limit.
    PayloadTooLarge,
    /// The endpoint simulates authentication and the request lacks credentials.
    Unauthorized,
    /// No record with the requested key exists in a CRUD resource.
    RecordNotFound,
    /// A record with the same primary key already exists.
    RecordConflict,
    /// An unexpected internal failure.
    InternalError,
}

impl ErrorCode {
    /// The HTTP status code conventionally paired with this error.
    #[must_use]
    pub const fn http_status(self) -> u16 {
        match self {
            Self::RouteNotFound => 404,
            Self::MethodNotAllowed => 405,
            Self::InvalidJsonBody => 400,
            Self::RequestValidationFailed => 422,
            Self::PayloadTooLarge => 413,
            Self::Unauthorized => 401,
            Self::RecordNotFound => 404,
            Self::RecordConflict => 409,
            Self::InternalError => 500,
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Serialize through serde so Display always matches the wire format.
        let json = serde_json::to_value(self).map_err(|_| fmt::Error)?;
        f.write_str(json.as_str().ok_or(fmt::Error)?)
    }
}

/// One specific problem inside an error, e.g. a failed schema rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorDetail {
    /// Where the problem is: a JSON pointer such as `/email`, or `query.page`.
    pub location: String,
    /// Human-readable explanation.
    pub message: String,
}

impl ErrorDetail {
    #[must_use]
    pub fn new(location: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            location: location.into(),
            message: message.into(),
        }
    }
}

/// The body inside the `error` key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorBody {
    pub code: ErrorCode,
    pub message: String,
    #[serde(default)]
    pub details: Vec<ErrorDetail>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<RequestId>,
}

/// Top-level error response: `{ "error": { ... } }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorEnvelope {
    pub error: ErrorBody,
}

impl ErrorEnvelope {
    #[must_use]
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            error: ErrorBody {
                code,
                message: message.into(),
                details: Vec::new(),
                request_id: None,
            },
        }
    }

    #[must_use]
    pub fn with_details(mut self, details: Vec<ErrorDetail>) -> Self {
        self.error.details = details;
        self
    }

    #[must_use]
    pub fn with_request_id(mut self, request_id: RequestId) -> Self {
        self.error.request_id = Some(request_id);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_serializes_to_spec_shape() {
        let id = RequestId::new();
        let env = ErrorEnvelope::new(ErrorCode::RouteNotFound, "No route.")
            .with_details(vec![ErrorDetail::new("path", "/nope")])
            .with_request_id(id);
        let json = serde_json::to_value(&env).unwrap();
        assert_eq!(json["error"]["code"], "ROUTE_NOT_FOUND");
        assert_eq!(json["error"]["message"], "No route.");
        assert_eq!(json["error"]["details"][0]["location"], "path");
        assert_eq!(json["error"]["request_id"], id.to_string());
    }

    #[test]
    fn display_matches_wire_format() {
        assert_eq!(
            ErrorCode::RequestValidationFailed.to_string(),
            "REQUEST_VALIDATION_FAILED"
        );
    }
}
