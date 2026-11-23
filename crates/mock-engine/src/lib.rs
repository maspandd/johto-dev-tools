//! The mock execution engine (spec §6.6).
//!
//! It turns a [`MockRequest`] into a [`MockOutcome`] by running the pipeline
//! route match → method check → auth simulation → parameter and body
//! validation → rule evaluation → response rendering.
//!
//! The engine does no I/O and never sleeps: delays are returned in the
//! outcome for the runtime to apply. This keeps it usable from any host
//! (Axum server, Tauri desktop, tests) and makes it fully unit-testable.

mod engine;
mod entropy;
mod request;
mod router;
mod rules;
mod template;

pub use engine::{BuildErrors, BuildProblem, Engine, EngineConfig, MockOutcome};
pub use entropy::{Entropy, SystemEntropy};
pub use request::{MockRequest, parse_query};
