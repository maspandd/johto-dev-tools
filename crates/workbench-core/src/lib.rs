//! Shared primitives for Developer Workbench: identifiers, error codes,
//! the API error envelope, and network bind settings.
//!
//! This crate has no I/O and no async runtime so every other crate can depend on it.

pub mod bind;
pub mod error;
pub mod id;

pub use bind::{BindAddress, BindError, DEFAULT_MOCK_PORT};
pub use error::{ErrorBody, ErrorCode, ErrorDetail, ErrorEnvelope};
pub use id::{EndpointId, EnvironmentId, MockServerId, ProjectId, RequestId, WorkspaceId};
