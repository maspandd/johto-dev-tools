//! A single running mock HTTP server.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use arc_swap::ArcSwap;
use axum::Router;
use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderName, HeaderValue, StatusCode};
use axum::response::Response;
use chrono::{DateTime, Utc};
use mock_engine::{BuildErrors, Engine, EngineConfig, MockOutcome, MockRequest};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use workbench_core::{ErrorCode, ErrorEnvelope, MockServerId};
use workbench_domain::Endpoint;

use crate::history::{HistoryRecord, HistorySink, Redactor};
use crate::{HistoryStore, MockServerConfig, StartError};

/// How long `stop` waits for in-flight requests before aborting them.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);
const HISTORY_QUEUE: usize = 1024;

struct AppState {
    server_id: MockServerId,
    engine: Arc<ArcSwap<Engine>>,
    max_body_bytes: usize,
    max_delay: Duration,
    history: Option<(HistorySink, Redactor)>,
}

/// Handle to a listening server. Dropping it without calling
/// [`RunningServer::stop`] aborts the server task.
pub struct RunningServer {
    id: MockServerId,
    addr: SocketAddr,
    config: MockServerConfig,
    engine: Arc<ArcSwap<Engine>>,
    history: HistoryStore,
    started_at: DateTime<Utc>,
    shutdown: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<std::io::Result<()>>>,
}

impl std::fmt::Debug for RunningServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunningServer")
            .field("id", &self.id)
            .field("addr", &self.addr)
            .finish_non_exhaustive()
    }
}

fn engine_config(config: &MockServerConfig) -> EngineConfig {
    EngineConfig {
        base_path: config.base_path.clone(),
    }
}

impl RunningServer {
    /// Validates the configuration, compiles the endpoints, binds the
    /// listener and starts serving.
    pub async fn start(
        config: MockServerConfig,
        endpoints: &[Endpoint],
    ) -> Result<Self, StartError> {
        let addr = config.bind.validate()?;
        let engine = Engine::build(&engine_config(&config), endpoints)?;

        let listener = TcpListener::bind(addr).await.map_err(|source| {
            if source.kind() == std::io::ErrorKind::AddrInUse {
                StartError::PortInUse { addr }
            } else {
                StartError::Listen { addr, source }
            }
        })?;
        let addr = listener
            .local_addr()
            .map_err(|source| StartError::Listen { addr, source })?;

        let id = MockServerId::new();
        let engine = Arc::new(ArcSwap::from_pointee(engine));
        let history = HistoryStore::new(config.history.retention);
        let state = Arc::new(AppState {
            server_id: id,
            engine: Arc::clone(&engine),
            max_body_bytes: config.max_body_bytes,
            max_delay: config.max_delay(),
            history: config.history.enabled.then(|| {
                (
                    HistorySink::spawn(history.clone(), HISTORY_QUEUE),
                    Redactor::new(&config.history),
                )
            }),
        });

        let app = Router::new().fallback(handle).with_state(state);
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        let task = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    // Resolves on an explicit stop or when the handle is dropped.
                    let _ = shutdown_rx.await;
                })
                .await
        });

        tracing::info!(server_id = %id, %addr, "mock server started");
        Ok(Self {
            id,
            addr,
            config,
            engine,
            history,
            started_at: Utc::now(),
            shutdown: Some(shutdown_tx),
            task: Some(task),
        })
    }

    #[must_use]
    pub const fn id(&self) -> MockServerId {
        self.id
    }

    /// The bound address (with the real port when `0` was requested).
    #[must_use]
    pub const fn local_addr(&self) -> SocketAddr {
        self.addr
    }

    #[must_use]
    pub const fn config(&self) -> &MockServerConfig {
        &self.config
    }

    #[must_use]
    pub const fn started_at(&self) -> DateTime<Utc> {
        self.started_at
    }

    #[must_use]
    pub fn route_count(&self) -> usize {
        self.engine.load().route_count()
    }

    #[must_use]
    pub fn history(&self) -> &HistoryStore {
        &self.history
    }

    /// Swaps in new endpoint definitions. In-flight requests finish on the
    /// old definitions; on error the current definitions stay active.
    pub fn reload(&self, endpoints: &[Endpoint]) -> Result<(), BuildErrors> {
        let engine = Engine::build(&engine_config(&self.config), endpoints)?;
        self.engine.store(Arc::new(engine));
        tracing::info!(server_id = %self.id, "mock server reloaded");
        Ok(())
    }

    /// Stops accepting connections, lets in-flight requests finish (up to a
    /// grace period), and releases the listener.
    pub async fn stop(mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(mut task) = self.task.take() {
            match tokio::time::timeout(SHUTDOWN_GRACE, &mut task).await {
                Ok(Ok(Ok(()))) => {}
                Ok(Ok(Err(e))) => {
                    tracing::warn!(server_id = %self.id, error = %e, "mock server exited with error")
                }
                Ok(Err(e)) => {
                    tracing::warn!(server_id = %self.id, error = %e, "mock server task failed")
                }
                Err(_) => {
                    tracing::warn!(server_id = %self.id, "grace period elapsed; aborting in-flight requests");
                    task.abort();
                    let _ = task.await;
                }
            }
        }
        tracing::info!(server_id = %self.id, addr = %self.addr, "mock server stopped");
    }
}

impl Drop for RunningServer {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

fn header_pairs(headers: &axum::http::HeaderMap) -> Vec<(String, String)> {
    headers
        .iter()
        .map(|(k, v)| {
            (
                k.as_str().to_owned(),
                String::from_utf8_lossy(v.as_bytes()).into_owned(),
            )
        })
        .collect()
}

fn too_large(mreq: &MockRequest, limit: usize) -> MockOutcome {
    let envelope = ErrorEnvelope::new(
        ErrorCode::PayloadTooLarge,
        format!("The request body exceeds the {limit}-byte limit."),
    )
    .with_request_id(mreq.request_id);
    MockOutcome {
        status: ErrorCode::PayloadTooLarge.http_status(),
        headers: vec![("content-type".to_owned(), "application/json".to_owned())],
        body: serde_json::to_vec(&envelope).unwrap_or_default(),
        delay: Duration::ZERO,
        matched_endpoint: None,
        matched_rule: None,
        error: Some(ErrorCode::PayloadTooLarge),
    }
}

fn to_response(outcome: &MockOutcome) -> Response {
    let mut response = Response::new(Body::from(outcome.body.clone()));
    *response.status_mut() =
        StatusCode::from_u16(outcome.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let headers = response.headers_mut();
    for (name, value) in &outcome.headers {
        match (
            HeaderName::from_bytes(name.as_bytes()),
            HeaderValue::from_str(value),
        ) {
            (Ok(n), Ok(v)) => {
                headers.append(n, v);
            }
            _ => tracing::warn!(header = %name, "skipping invalid response header"),
        }
    }
    response
}

async fn handle(State(state): State<Arc<AppState>>, req: Request) -> Response {
    let started = Instant::now();
    let timestamp = Utc::now();
    let (parts, body) = req.into_parts();
    let path_and_query = parts.uri.path_and_query().map_or("/", |pq| pq.as_str());
    let mut mreq = MockRequest::new(parts.method.as_str(), path_and_query);
    mreq.headers = header_pairs(&parts.headers);

    let outcome = match axum::body::to_bytes(body, state.max_body_bytes).await {
        Ok(bytes) => {
            mreq.body = bytes.to_vec();
            // `load_full` so no guard is held across the await below.
            let engine = state.engine.load_full();
            engine.handle(&mreq)
        }
        Err(_) => too_large(&mreq, state.max_body_bytes),
    };

    if !outcome.delay.is_zero() {
        tokio::time::sleep(outcome.delay.min(state.max_delay)).await;
    }
    let response = to_response(&outcome);

    if let Some((sink, redactor)) = &state.history {
        sink.record(HistoryRecord {
            id: mreq.request_id,
            server_id: state.server_id,
            timestamp,
            method: mreq.method.clone(),
            path: mreq.path.clone(),
            query: redactor.query(&mreq.query),
            request_headers: redactor.headers(&mreq.headers),
            request_body: redactor.body(&mreq.body),
            matched_endpoint: outcome.matched_endpoint,
            matched_rule: outcome.matched_rule.clone(),
            status: outcome.status,
            duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            response_headers: redactor.headers(&outcome.headers),
            response_body: redactor.body(&outcome.body),
        });
    }

    tracing::debug!(
        method = %mreq.method,
        path = %mreq.path,
        status = outcome.status,
        request_id = %mreq.request_id,
        "mock request"
    );
    response
}
