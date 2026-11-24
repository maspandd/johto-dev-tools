//! Persistence for Developer Workbench (spec §7).
//!
//! One [`Store`] API over two backends:
//!
//! - **SQLite** for the desktop app (local file, WAL mode, foreign keys on)
//! - **PostgreSQL** for cloud and self-hosted deployments
//!
//! Both share the same SQL; schemas live in `migrations/sqlite` and
//! `migrations/postgres` and are applied automatically on connect.
//! Frequently queried fields are columns; flexible configuration
//! (endpoint request/response/rules, resource and dataset settings,
//! headers, variables) is JSON (`JSONB` on PostgreSQL). CRUD resource
//! records are stored one row per key.

mod datasets;
mod endpoints;
mod environments;
mod error;
mod projects;
mod request_logs;
mod resources;
mod secrets;
mod store;
mod workspaces;

pub use endpoints::EndpointRecord;
pub use error::{StorageError, StorageResult};
pub use resources::ResourceRecord;
pub use secrets::{EncryptedSecret, SecretMeta};
pub use store::{Backend, Store};
