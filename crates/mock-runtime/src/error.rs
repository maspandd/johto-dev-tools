use std::net::SocketAddr;

use mock_engine::{BuildErrors, SeedError};
use thiserror::Error;
use workbench_core::BindError;

/// Why a mock server could not start.
#[derive(Debug, Error)]
pub enum StartError {
    #[error(transparent)]
    Bind(#[from] BindError),
    #[error("port {} on {} is already in use; choose another port or stop the process using it", .addr.port(), .addr.ip())]
    PortInUse { addr: SocketAddr },
    #[error("could not listen on {addr}: {source}")]
    Listen {
        addr: SocketAddr,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid endpoint configuration: {0}")]
    Build(#[from] BuildErrors),
    #[error("cannot generate seed data: {0}")]
    Seed(#[from] SeedError),
    #[error("cannot load records for resource `{resource}`: {source}")]
    Records {
        resource: String,
        #[source]
        source: crate::RecordStoreError,
    },
}
