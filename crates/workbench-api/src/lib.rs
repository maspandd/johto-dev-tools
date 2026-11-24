//! The management API (spec §8): an Axum application under `/api/v1` that
//! configures workspaces, projects, environments, endpoints, resources and
//! datasets, and starts and stops mock servers built from them.
//!
//! The management API and the mock servers listen on different ports
//! (spec §6.7), so mock routes can never collide with management routes.
//!
//! Cross-cutting behaviour:
//! - every response carries `x-request-id`; errors use the shared envelope
//! - collections are paginated with `?page=&limit=`
//! - CORS is limited to an explicit origin list
//! - an optional bearer token protects every `/api/v1` route
//! - environment secrets are encrypted at rest and never returned

mod api_doc;
mod error;
mod extract;
mod middleware;
mod routes;
mod runtime;
pub mod secrets;
mod util;

use std::sync::Arc;

use axum::Router;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::{HeaderValue, Method, header};
use axum::response::IntoResponse;
use axum::routing::get;
use serde_json::json;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::trace::TraceLayer;
use workbench_core::ErrorCode;
use workbench_storage::{Backend, Store};

pub use error::{ApiError, ApiResult};
pub use runtime::{MockServerInfo, MockServers, StartRequest};
pub use secrets::SecretKey;

/// Default origins of the web UI's development server.
pub const DEFAULT_CORS_ORIGINS: &[&str] = &["http://localhost:5173", "http://127.0.0.1:5173"];

#[derive(Debug, Clone)]
pub struct ApiConfig {
    /// When set, every `/api/v1` request needs `Authorization: Bearer <token>`.
    pub api_token: Option<String>,
    /// Browser origins allowed to call the API. Empty disables CORS.
    pub cors_origins: Vec<String>,
    /// Request logs kept per project.
    pub history_retention: usize,
    /// Largest accepted request body, in bytes.
    pub max_body_bytes: usize,
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            api_token: None,
            cors_origins: DEFAULT_CORS_ORIGINS
                .iter()
                .map(|s| (*s).to_owned())
                .collect(),
            history_retention: 1_000,
            max_body_bytes: 5 * 1024 * 1024,
        }
    }
}

/// Shared by every handler. Cheap to clone.
#[derive(Clone)]
pub struct AppState {
    pub store: Store,
    pub runtime: Arc<MockServers>,
    pub secret_key: Arc<SecretKey>,
    pub config: Arc<ApiConfig>,
}

impl AppState {
    /// Must be called inside a Tokio runtime (it spawns the history writer).
    #[must_use]
    pub fn new(store: Store, secret_key: SecretKey, config: ApiConfig) -> Self {
        let runtime = Arc::new(MockServers::new(store.clone(), config.history_retention));
        Self {
            store,
            runtime,
            secret_key: Arc::new(secret_key),
            config: Arc::new(config),
        }
    }
}

async fn health(State(s): State<AppState>) -> impl IntoResponse {
    let database = match s.store.backend() {
        Backend::Sqlite => "sqlite",
        Backend::Postgres => "postgres",
    };
    axum::Json(json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "database": database,
        "mock_servers": s.runtime.list(None).await.len(),
    }))
}

async fn not_found() -> ApiError {
    ApiError::new(ErrorCode::NotFound, "No API route matches this path.")
}

async fn method_not_allowed() -> ApiError {
    ApiError::new(
        ErrorCode::MethodNotAllowed,
        "This method is not allowed for this path.",
    )
}

fn cors(origins: &[String]) -> Option<CorsLayer> {
    let list: Vec<HeaderValue> = origins
        .iter()
        .filter_map(|o| HeaderValue::from_str(o).ok())
        .collect();
    if list.is_empty() {
        return None;
    }
    Some(
        CorsLayer::new()
            .allow_origin(AllowOrigin::list(list))
            .allow_methods([
                Method::GET,
                Method::POST,
                Method::PUT,
                Method::PATCH,
                Method::DELETE,
            ])
            .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION])
            .expose_headers([header::HeaderName::from_static(
                middleware::REQUEST_ID_HEADER,
            )]),
    )
}

/// The complete application: `/health` plus the API under `/api/v1`.
pub fn router(state: AppState) -> Router {
    let api = routes::api().route_layer(axum::middleware::from_fn_with_state(
        state.clone(),
        middleware::require_token,
    ));
    let mut app = Router::new()
        .route("/health", get(health))
        .nest("/api/v1", api)
        .fallback(not_found)
        .method_not_allowed_fallback(method_not_allowed)
        .layer(DefaultBodyLimit::max(state.config.max_body_bytes));
    if let Some(cors) = cors(&state.config.cors_origins) {
        app = app.layer(cors);
    }
    app.layer(TraceLayer::new_for_http())
        .layer(axum::middleware::from_fn(middleware::request_id))
        .with_state(state)
}
