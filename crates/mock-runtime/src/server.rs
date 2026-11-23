//! A single running mock HTTP server.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use arc_swap::ArcSwap;
use axum::Router;
use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderName, HeaderValue, StatusCode};
use axum::response::Response;
use chrono::{DateTime, Utc};
use mock_engine::{
    BuildErrors, DataStore, Engine, EngineConfig, MockOutcome, MockRequest, SeedError,
};
use serde_json::Value;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use workbench_core::{ErrorCode, ErrorEnvelope, MockServerId, ResourceId};
use workbench_domain::{Endpoint, RecordChange, Resource};

use crate::history::{HistoryRecord, HistorySink, Redactor};
use crate::{HistoryStore, MemoryRecordStore, MockServerConfig, RecordStore, StartError};

/// How long `stop` waits for in-flight requests before aborting them.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);
/// How long `stop` waits for pending record changes to be written.
const FLUSH_GRACE: Duration = Duration::from_secs(10);
const HISTORY_QUEUE: usize = 1024;

struct AppState {
    server_id: MockServerId,
    engine: Arc<ArcSwap<Engine>>,
    max_body_bytes: usize,
    max_delay: Duration,
    history: Option<(HistorySink, Redactor)>,
}

enum WriterMsg {
    Change(ResourceId, RecordChange),
    Flush(oneshot::Sender<()>),
}

/// Applies record changes to the store one at a time, in order.
async fn run_writer(store: Arc<dyn RecordStore>, mut rx: mpsc::UnboundedReceiver<WriterMsg>) {
    while let Some(msg) = rx.recv().await {
        match msg {
            WriterMsg::Change(id, change) => {
                if let Err(e) = store.apply(id, change).await {
                    tracing::error!(resource = %id, error = %e, "failed to persist record change");
                }
            }
            WriterMsg::Flush(done) => {
                let _ = done.send(());
            }
        }
    }
}

/// Handle to a listening server. Dropping it without calling
/// [`RunningServer::stop`] aborts the server task.
pub struct RunningServer {
    id: MockServerId,
    addr: SocketAddr,
    config: MockServerConfig,
    engine: Arc<ArcSwap<Engine>>,
    data: Arc<DataStore>,
    store: Arc<dyn RecordStore>,
    writer_tx: mpsc::UnboundedSender<WriterMsg>,
    endpoints: Mutex<Vec<Endpoint>>,
    resources: Mutex<Vec<Resource>>,
    history: HistoryStore,
    started_at: DateTime<Utc>,
    shutdown: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<std::io::Result<()>>>,
    writer: Option<JoinHandle<()>>,
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
        default_headers: config.default_headers.clone(),
    }
}

/// Loads every resource's records that are not in memory yet, seeding
/// (and persisting) resources that have never been initialized.
async fn prepare_data(
    engine: &Engine,
    data: &DataStore,
    store: &dyn RecordStore,
) -> Result<(), StartError> {
    for &id in engine.seed_order() {
        if data.contains(id) {
            continue;
        }
        let name = engine
            .resources()
            .find(|r| r.id == id)
            .map(|r| r.name.clone())
            .unwrap_or_default();
        match store.load(id).await {
            Ok(Some(records)) => data.load(id, records),
            Ok(None) => data.replace(id, engine.seed_records(id)?),
            Err(source) => {
                return Err(StartError::Records {
                    resource: name,
                    source,
                });
            }
        }
    }
    Ok(())
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl RunningServer {
    /// Starts a server with endpoints only. Resource data, if any, lives in memory.
    pub async fn start(
        config: MockServerConfig,
        endpoints: &[Endpoint],
    ) -> Result<Self, StartError> {
        Self::start_project(config, endpoints, &[], Arc::new(MemoryRecordStore::new())).await
    }

    /// Validates the configuration, compiles endpoints and resources, binds
    /// the listener, loads or seeds resource data from `store`, and starts
    /// serving. Seed data is persisted before the first request is accepted.
    pub async fn start_project(
        config: MockServerConfig,
        endpoints: &[Endpoint],
        resources: &[Resource],
        store: Arc<dyn RecordStore>,
    ) -> Result<Self, StartError> {
        let addr = config.bind.validate()?;

        let (writer_tx, writer_rx) = mpsc::unbounded_channel();
        let listener_tx = writer_tx.clone();
        let data = Arc::new(DataStore::with_listener(Box::new(move |id, change| {
            let _ = listener_tx.send(WriterMsg::Change(id, change.clone()));
        })));
        let engine = Engine::build_with_resources(
            &engine_config(&config),
            endpoints,
            resources,
            Arc::clone(&data),
        )?;

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

        let writer = tokio::spawn(run_writer(Arc::clone(&store), writer_rx));
        if let Err(e) = prepare_data(&engine, &data, store.as_ref()).await {
            writer.abort();
            return Err(e);
        }

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

        let server = Self {
            id,
            addr,
            config,
            engine,
            data,
            store,
            writer_tx,
            endpoints: Mutex::new(endpoints.to_vec()),
            resources: Mutex::new(resources.to_vec()),
            history,
            started_at: Utc::now(),
            shutdown: Some(shutdown_tx),
            task: Some(task),
            writer: Some(writer),
        };
        server.flush_records().await;
        tracing::info!(server_id = %id, %addr, "mock server started");
        Ok(server)
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

    /// Number of active endpoints (resources not included).
    #[must_use]
    pub fn route_count(&self) -> usize {
        self.engine.load().route_count()
    }

    #[must_use]
    pub fn history(&self) -> &HistoryStore {
        &self.history
    }

    /// Current records of a resource, in display order.
    #[must_use]
    pub fn resource_records(&self, id: ResourceId) -> Option<Vec<Value>> {
        self.data.records(id)
    }

    /// Swaps in new endpoint definitions, keeping the current resources.
    /// In-flight requests finish on the old definitions; on error the
    /// current definitions stay active.
    pub fn reload(&self, endpoints: &[Endpoint]) -> Result<(), BuildErrors> {
        let resources = lock(&self.resources).clone();
        let engine = Engine::build_with_resources(
            &engine_config(&self.config),
            endpoints,
            &resources,
            Arc::clone(&self.data),
        )?;
        self.engine.store(Arc::new(engine));
        *lock(&self.endpoints) = endpoints.to_vec();
        tracing::info!(server_id = %self.id, "mock server reloaded");
        Ok(())
    }

    /// Swaps in new endpoints and resources. Data of existing resources is
    /// kept; new resources are loaded or seeded before the swap.
    pub async fn reload_project(
        &self,
        endpoints: &[Endpoint],
        resources: &[Resource],
    ) -> Result<(), StartError> {
        let engine = Engine::build_with_resources(
            &engine_config(&self.config),
            endpoints,
            resources,
            Arc::clone(&self.data),
        )?;
        prepare_data(&engine, &self.data, self.store.as_ref()).await?;
        self.engine.store(Arc::new(engine));
        *lock(&self.endpoints) = endpoints.to_vec();
        *lock(&self.resources) = resources.to_vec();
        self.flush_records().await;
        tracing::info!(server_id = %self.id, "mock server reloaded");
        Ok(())
    }

    /// Replaces a resource's records with freshly generated seed data
    /// (empty if the resource has no seed) and persists the reset.
    pub async fn reset_resource(&self, id: ResourceId) -> Result<(), SeedError> {
        let records = self.engine.load().seed_records(id)?;
        self.data.replace(id, records);
        self.flush_records().await;
        Ok(())
    }

    /// Waits until every record change so far has been written to the store.
    pub async fn flush_records(&self) {
        let (tx, rx) = oneshot::channel();
        if self.writer_tx.send(WriterMsg::Flush(tx)).is_ok() {
            let _ = rx.await;
        }
    }

    /// Stops accepting connections, lets in-flight requests finish (up to a
    /// grace period), writes pending record changes, and releases the listener.
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
        if tokio::time::timeout(FLUSH_GRACE, self.flush_records())
            .await
            .is_err()
        {
            tracing::warn!(server_id = %self.id, "timed out writing pending record changes");
        }
        if let Some(writer) = self.writer.take() {
            writer.abort();
        }
        tracing::info!(server_id = %self.id, addr = %self.addr, "mock server stopped");
    }
}

impl Drop for RunningServer {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
        if let Some(writer) = self.writer.take() {
            writer.abort();
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
        headers: vec![("content-type".to_owned(), "application/json".to_owned())],
        body: serde_json::to_vec(&envelope).unwrap_or_default(),
        error: Some(ErrorCode::PayloadTooLarge),
        ..MockOutcome::new(ErrorCode::PayloadTooLarge.http_status())
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
    let timestamp = workbench_core::time::now();
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
            matched_resource: outcome.matched_resource,
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
