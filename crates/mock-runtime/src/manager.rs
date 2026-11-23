//! Runs several isolated mock servers (one per project or environment).

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use mock_engine::BuildErrors;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use workbench_core::MockServerId;
use workbench_domain::{Endpoint, Resource};

use crate::{HistoryStore, MockServerConfig, RecordStore, RunningServer, StartError};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerStatus {
    pub id: MockServerId,
    pub addr: SocketAddr,
    pub base_path: Option<String>,
    pub route_count: usize,
    pub started_at: DateTime<Utc>,
}

impl From<&RunningServer> for ServerStatus {
    fn from(s: &RunningServer) -> Self {
        Self {
            id: s.id(),
            addr: s.local_addr(),
            base_path: s.config().base_path.clone(),
            route_count: s.route_count(),
            started_at: s.started_at(),
        }
    }
}

#[derive(Debug, Default)]
pub struct MockRuntimeManager {
    servers: Mutex<HashMap<MockServerId, RunningServer>>,
}

impl MockRuntimeManager {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn start(
        &self,
        config: MockServerConfig,
        endpoints: &[Endpoint],
    ) -> Result<ServerStatus, StartError> {
        let server = RunningServer::start(config, endpoints).await?;
        let status = ServerStatus::from(&server);
        self.servers.lock().await.insert(server.id(), server);
        Ok(status)
    }

    /// Starts a server with endpoints and CRUD resources backed by `store`.
    pub async fn start_project(
        &self,
        config: MockServerConfig,
        endpoints: &[Endpoint],
        resources: &[Resource],
        store: Arc<dyn RecordStore>,
    ) -> Result<ServerStatus, StartError> {
        let server = RunningServer::start_project(config, endpoints, resources, store).await?;
        let status = ServerStatus::from(&server);
        self.servers.lock().await.insert(server.id(), server);
        Ok(status)
    }

    /// Returns `false` if no server has that ID.
    pub async fn stop(&self, id: MockServerId) -> bool {
        let server = self.servers.lock().await.remove(&id);
        match server {
            Some(server) => {
                server.stop().await;
                true
            }
            None => false,
        }
    }

    /// Stops every server concurrently. Call on application exit.
    pub async fn stop_all(&self) {
        let servers: Vec<RunningServer> =
            self.servers.lock().await.drain().map(|(_, s)| s).collect();
        let stops = servers
            .into_iter()
            .map(RunningServer::stop)
            .map(tokio::spawn);
        for handle in stops.collect::<Vec<_>>() {
            let _ = handle.await;
        }
    }

    pub async fn status(&self) -> Vec<ServerStatus> {
        let mut list: Vec<ServerStatus> = self
            .servers
            .lock()
            .await
            .values()
            .map(ServerStatus::from)
            .collect();
        list.sort_by_key(|s| s.started_at);
        list
    }

    pub async fn history(&self, id: MockServerId) -> Option<HistoryStore> {
        self.servers
            .lock()
            .await
            .get(&id)
            .map(|s| s.history().clone())
    }

    /// `None` if no server has that ID.
    pub async fn reload(
        &self,
        id: MockServerId,
        endpoints: &[Endpoint],
    ) -> Option<Result<(), BuildErrors>> {
        self.servers
            .lock()
            .await
            .get(&id)
            .map(|s| s.reload(endpoints))
    }
}
