//! The mock HTTP runtime (spec §4.3, §6.7).
//!
//! It owns real TCP listeners and delegates every request to a
//! [`mock_engine::Engine`]. Responsibilities:
//!
//! - start and stop listeners, with loopback-only binding unless exposure is opted into
//! - report port conflicts as errors instead of crashing
//! - hot-reload endpoint definitions without restarting the listener
//! - apply simulated delays and body-size limits
//! - capture request history off the response path, with redaction
//! - serve CRUD resources, loading or seeding their records and persisting
//!   every change through a pluggable [`RecordStore`]
//!
//! [`MockRuntimeManager`] runs several isolated servers side by side.

mod config;
mod error;
mod history;
mod manager;
mod record_store;
mod server;

pub use config::{HistoryConfig, MockServerConfig};
pub use error::StartError;
pub use history::{CapturedBody, HistoryFilter, HistoryListener, HistoryRecord, HistoryStore};
pub use manager::{MockRuntimeManager, ServerStatus};
pub use record_store::{
    JsonFileRecordStore, MemoryRecordStore, RecordFuture, RecordStore, RecordStoreError,
};
pub use server::{RunningServer, ServerOptions};
