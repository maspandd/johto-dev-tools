//! Request IDs and bearer-token protection.

use axum::extract::{Request, State};
use axum::http::{HeaderValue, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use tracing::Instrument;
use workbench_core::{ErrorCode, RequestId};

use crate::{ApiError, AppState};

tokio::task_local! {
    static REQUEST_ID: RequestId;
}

/// The ID of the request being handled, if called inside [`request_id`].
pub(crate) fn current_request_id() -> Option<RequestId> {
    REQUEST_ID.try_with(|id| *id).ok()
}

pub(crate) const REQUEST_ID_HEADER: &str = "x-request-id";

/// Gives every request an ID, available to error responses and returned
/// in the `x-request-id` header.
pub(crate) async fn request_id(req: Request, next: Next) -> Response {
    let id = RequestId::new();
    let span = tracing::info_span!("api", request_id = %id, method = %req.method(), path = %req.uri().path());
    let mut response = REQUEST_ID.scope(id, next.run(req)).instrument(span).await;
    if let Ok(value) = HeaderValue::from_str(&id.to_string()) {
        response.headers_mut().insert(REQUEST_ID_HEADER, value);
    }
    response
}

/// Constant-time comparison so token checks do not leak timing.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// When an API token is configured, every management request must send
/// `Authorization: Bearer <token>`.
pub(crate) async fn require_token(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    let Some(expected) = state.config.api_token.as_deref() else {
        return next.run(req).await;
    };
    let provided = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| {
            v.strip_prefix("Bearer ")
                .or_else(|| v.strip_prefix("bearer "))
        });
    match provided {
        Some(token) if same(token.trim().as_bytes(), expected.as_bytes()) => next.run(req).await,
        _ => ApiError::new(
            ErrorCode::Unauthorized,
            "A valid `Authorization: Bearer <token>` header is required.",
        )
        .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::same;

    #[test]
    fn constant_time_compare() {
        assert!(same(b"abc", b"abc"));
        assert!(!same(b"abc", b"abd"));
        assert!(!same(b"abc", b"abcd"));
    }
}
