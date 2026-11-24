//! Running mock servers on behalf of the management API (spec §8.4).
//!
//! Servers are built from stored projects: endpoints and resources come
//! from the database, resource records are persisted to it, and every
//! captured request is written to `request_logs` in the background.

use std::collections::{BTreeMap, HashMap};
use std::net::IpAddr;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use mock_runtime::{
    CorsConfig, MockServerConfig, RecordFuture, RecordStore, RunningServer, ServerOptions,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::{Mutex, mpsc};
use workbench_core::{EnvironmentId, MockServerId, ProjectId, ResourceId};
use workbench_domain::{HistoryRecord, RecordChange, StoredRecord};
use workbench_storage::{StorageError, Store};

use crate::{ApiError, ApiResult};

/// Resource records persisted in the project database.
struct DbRecords(Store);

impl RecordStore for DbRecords {
    fn load(&self, id: ResourceId) -> RecordFuture<'_, Option<Vec<StoredRecord>>> {
        Box::pin(async move { Ok(self.0.load_resource_records(id).await?) })
    }

    fn apply(&self, id: ResourceId, change: RecordChange) -> RecordFuture<'_, ()> {
        Box::pin(async move { Ok(self.0.apply_record_change(id, &change).await?) })
    }
}

/// How to start a mock server. Unset fields come from the environment
/// (or the loopback default `127.0.0.1:4010`).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartRequest {
    pub environment_id: Option<EnvironmentId>,
    pub host: Option<IpAddr>,
    pub port: Option<u16>,
    pub allow_network_exposure: Option<bool>,
    /// Browser access. Default: pages on this machine (`localhost`,
    /// `127.0.0.1`, `[::1]`) may call the server.
    pub cors: Option<CorsConfig>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MockServerInfo {
    pub id: MockServerId,
    pub project_id: ProjectId,
    pub environment_id: Option<EnvironmentId>,
    pub status: &'static str,
    pub url: String,
    pub host: IpAddr,
    pub port: u16,
    pub base_path: Option<String>,
    pub endpoints: usize,
    pub resources: usize,
    pub started_at: DateTime<Utc>,
    pub cors: CorsConfig,
    /// Set when applying the latest project changes failed; the server keeps
    /// running the previous definitions.
    pub last_reload_error: Option<String>,
}

struct Managed {
    server: RunningServer,
    project_id: ProjectId,
    environment_id: Option<EnvironmentId>,
    resources: usize,
    last_reload_error: Option<String>,
    public_host: Option<String>,
}

/// Where mock servers listen and how their URLs are reported, set by the
/// operator (for example in a container).
#[derive(Debug, Clone, Default)]
pub struct MockDefaults {
    /// Used when a start request names no host and the environment keeps
    /// the loopback default. A non-loopback address here is the
    /// operator's opt-in to network exposure, e.g. `0.0.0.0` in a
    /// container whose ports are published.
    pub host: Option<IpAddr>,
    /// Host name in reported URLs, e.g. `localhost` for published ports.
    pub public_host: Option<String>,
}

/// `host:port` for a URL. Unspecified addresses (`0.0.0.0`, `::`) are
/// shown as `localhost`, which is where a browser on this machine reaches them.
fn url_authority(addr: std::net::SocketAddr, public_host: Option<&str>) -> String {
    let host = match public_host {
        Some(h) => h.to_owned(),
        None if addr.ip().is_unspecified() => "localhost".to_owned(),
        None if addr.is_ipv6() => format!("[{}]", addr.ip()),
        None => addr.ip().to_string(),
    };
    format!("{host}:{}", addr.port())
}

impl Managed {
    fn info(&self) -> MockServerInfo {
        let addr = self.server.local_addr();
        let base = self.server.config().base_path.clone();
        MockServerInfo {
            id: self.server.id(),
            project_id: self.project_id,
            environment_id: self.environment_id,
            status: "running",
            url: format!(
                "http://{}{}",
                url_authority(addr, self.public_host.as_deref()),
                base.as_deref().unwrap_or("").trim_end_matches('/')
            ),
            host: addr.ip(),
            port: addr.port(),
            base_path: base,
            endpoints: self.server.route_count(),
            resources: self.resources,
            started_at: self.server.started_at(),
            cors: self.server.config().cors.clone(),
            last_reload_error: self.last_reload_error.clone(),
        }
    }
}

/// Capacity of the queue between mock servers and the history writer.
const HISTORY_QUEUE: usize = 4096;
/// Prune a project's logs after this many inserts.
const PRUNE_EVERY: u32 = 100;

pub struct MockServers {
    store: Store,
    defaults: MockDefaults,
    servers: Mutex<HashMap<MockServerId, Managed>>,
    history_tx: mpsc::Sender<(ProjectId, HistoryRecord)>,
}

/// Writes captured requests to the database and enforces retention.
async fn persist_history(
    store: Store,
    retention: usize,
    mut rx: mpsc::Receiver<(ProjectId, HistoryRecord)>,
) {
    let mut since_prune: HashMap<ProjectId, u32> = HashMap::new();
    while let Some((project_id, record)) = rx.recv().await {
        match store.insert_request_log(project_id, &record).await {
            Ok(()) => {}
            // The project was deleted while its server was still answering.
            Err(StorageError::NotFound { .. }) => continue,
            Err(e) => {
                tracing::warn!(error = %e, "failed to persist request log");
                continue;
            }
        }
        let n = since_prune.entry(project_id).or_default();
        *n += 1;
        if *n >= PRUNE_EVERY {
            *n = 0;
            if let Err(e) = store.prune_request_logs(project_id, retention).await {
                tracing::warn!(error = %e, "failed to prune request logs");
            }
        }
    }
}

impl MockServers {
    /// Spawns the history writer. `retention` is the number of request
    /// logs kept per project.
    #[must_use]
    pub fn new(store: Store, retention: usize) -> Self {
        Self::with_defaults(store, retention, MockDefaults::default())
    }

    /// Like [`MockServers::new`], with operator defaults for binding.
    #[must_use]
    pub fn with_defaults(store: Store, retention: usize, defaults: MockDefaults) -> Self {
        let (history_tx, rx) = mpsc::channel(HISTORY_QUEUE);
        tokio::spawn(persist_history(store.clone(), retention.max(1), rx));
        Self {
            store,
            defaults,
            servers: Mutex::new(HashMap::new()),
            history_tx,
        }
    }

    pub async fn start(
        &self,
        project_id: ProjectId,
        req: StartRequest,
    ) -> ApiResult<MockServerInfo> {
        let project = self.store.get_project(project_id).await?;
        if project.is_archived() {
            return Err(ApiError::conflict(
                "Restore the project before starting a mock server.",
            ));
        }
        let environment = match req.environment_id {
            Some(id) => {
                let env = self.store.get_environment(id).await?;
                if env.project_id != project_id {
                    return Err(ApiError::not_found("environment", id));
                }
                Some(env)
            }
            None => None,
        };
        let endpoints = self.store.list_endpoints(project_id).await?;
        let resources = self.store.list_resources(project_id).await?;
        let active_resources = resources.iter().filter(|r| r.enabled).count();

        let mut servers = self.servers.lock().await;
        if active_resources > 0
            && let Some(other) = servers
                .values()
                .find(|m| m.project_id == project_id && m.resources > 0)
        {
            return Err(ApiError::conflict(format!(
                "Resource data is shared by the whole project; stop mock server {} first.",
                other.server.id()
            )));
        }

        let mut bind = environment.as_ref().map(|e| e.bind).unwrap_or_default();
        if let Some(host) = req.host {
            bind.host = host;
        } else if let Some(host) = self.defaults.host
            && bind.host.is_loopback()
        {
            bind.host = host;
            bind.allow_network_exposure = !host.is_loopback();
        }
        if let Some(port) = req.port {
            bind.port = port;
        }
        if let Some(allow) = req.allow_network_exposure {
            bind.allow_network_exposure = allow;
        }
        let mut default_headers: BTreeMap<String, String> = project.default_headers.clone();
        let mut variables = BTreeMap::new();
        if let Some(env) = &environment {
            default_headers.extend(env.default_headers.clone());
            variables = env.variables.clone();
        }
        let config = MockServerConfig {
            bind,
            base_path: project.base_path.clone(),
            default_headers,
            variables,
            cors: req.cors.unwrap_or_default(),
            ..MockServerConfig::default()
        };

        let tx = self.history_tx.clone();
        let on_history = Arc::new(move |record: &HistoryRecord| {
            if tx.try_send((project_id, record.clone())).is_err() {
                tracing::debug!("request log queue full; record not persisted");
            }
        });
        let server = RunningServer::start_with(
            config,
            ServerOptions {
                endpoints,
                resources,
                record_store: Arc::new(DbRecords(self.store.clone())),
                on_history: Some(on_history),
            },
        )
        .await?;
        let managed = Managed {
            server,
            project_id,
            environment_id: environment.map(|e| e.id),
            resources: active_resources,
            last_reload_error: None,
            public_host: self.defaults.public_host.clone(),
        };
        let info = managed.info();
        servers.insert(info.id, managed);
        tracing::info!(server_id = %info.id, %project_id, url = %info.url, "mock server started");
        Ok(info)
    }

    pub async fn list(&self, project_id: Option<ProjectId>) -> Vec<MockServerInfo> {
        let servers = self.servers.lock().await;
        let mut list: Vec<MockServerInfo> = servers
            .values()
            .filter(|m| project_id.is_none_or(|p| p == m.project_id))
            .map(Managed::info)
            .collect();
        list.sort_by_key(|i| i.started_at);
        list
    }

    pub async fn get(&self, id: MockServerId) -> ApiResult<MockServerInfo> {
        self.servers
            .lock()
            .await
            .get(&id)
            .map(Managed::info)
            .ok_or_else(|| ApiError::not_found("mock server", id))
    }

    /// Stops a server, writes pending data, and releases its port.
    pub async fn stop(&self, id: MockServerId) -> ApiResult<()> {
        let managed = self
            .servers
            .lock()
            .await
            .remove(&id)
            .ok_or_else(|| ApiError::not_found("mock server", id))?;
        managed.server.stop().await;
        Ok(())
    }

    /// Stops every server of a project (e.g. before deleting it).
    pub async fn stop_project(&self, project_id: ProjectId) {
        let ids: Vec<MockServerId> = self
            .servers
            .lock()
            .await
            .iter()
            .filter(|(_, m)| m.project_id == project_id)
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            let _ = self.stop(id).await;
        }
    }

    /// Stops everything. Call on shutdown.
    pub async fn stop_all(&self) {
        let all: Vec<Managed> = self.servers.lock().await.drain().map(|(_, m)| m).collect();
        for m in all {
            m.server.stop().await;
        }
    }

    /// Re-reads one server's endpoints and resources from the database.
    /// On failure the server keeps its previous definitions.
    pub async fn reload(&self, id: MockServerId) -> ApiResult<MockServerInfo> {
        let mut servers = self.servers.lock().await;
        let managed = servers
            .get_mut(&id)
            .ok_or_else(|| ApiError::not_found("mock server", id))?;
        let result = self.reload_managed(managed).await;
        match result {
            Ok(()) => Ok(managed.info()),
            Err(e) => Err(e),
        }
    }

    async fn reload_managed(&self, managed: &mut Managed) -> ApiResult<()> {
        let endpoints = self.store.list_endpoints(managed.project_id).await?;
        let resources = self.store.list_resources(managed.project_id).await?;
        match managed.server.reload_project(&endpoints, &resources).await {
            Ok(()) => {
                managed.resources = resources.iter().filter(|r| r.enabled).count();
                managed.last_reload_error = None;
                Ok(())
            }
            Err(e) => {
                managed.last_reload_error = Some(e.to_string());
                Err(e.into())
            }
        }
    }

    /// Applies stored changes to every running server of a project. Errors
    /// are recorded on each server (`last_reload_error`) rather than returned,
    /// because the change itself has already been saved.
    pub async fn refresh_project(&self, project_id: ProjectId) {
        let mut servers = self.servers.lock().await;
        for managed in servers.values_mut().filter(|m| m.project_id == project_id) {
            if let Err(e) = self.reload_managed(managed).await {
                tracing::warn!(server_id = %managed.server.id(), error = %e.message, "reload failed");
            }
        }
    }

    /// Resets a resource on the server that serves it. Returns `false` if
    /// no running server serves the resource.
    pub async fn reset_resource(&self, project_id: ProjectId, id: ResourceId) -> ApiResult<bool> {
        let servers = self.servers.lock().await;
        let Some(m) = servers
            .values()
            .find(|m| m.project_id == project_id && m.server.resource_records(id).is_some())
        else {
            return Ok(false);
        };
        m.server.reset_resource(id).await.map_err(|e| {
            ApiError::validation(vec![workbench_core::ErrorDetail::new(
                "seed",
                e.to_string(),
            )])
        })?;
        Ok(true)
    }

    /// Live records of a resource, if a running server serves it.
    pub async fn resource_records(
        &self,
        project_id: ProjectId,
        id: ResourceId,
    ) -> Option<Vec<Value>> {
        self.servers
            .lock()
            .await
            .values()
            .filter(|m| m.project_id == project_id)
            .find_map(|m| m.server.resource_records(id))
    }
}
