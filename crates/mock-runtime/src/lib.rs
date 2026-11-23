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
//!
//! [`MockRuntimeManager`] runs several isolated servers side by side.

mod config;
mod error;
mod history;
mod manager;
mod server;

pub use config::{HistoryConfig, MockServerConfig};
pub use error::StartError;
pub use history::{CapturedBody, HistoryFilter, HistoryRecord, HistoryStore};
pub use manager::{MockRuntimeManager, ServerStatus};
pub use server::RunningServer;
