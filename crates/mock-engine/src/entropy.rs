//! Sources of non-determinism (random IDs, the clock) behind a trait so
//! tests can make template output predictable.

use chrono::{DateTime, Utc};

pub trait Entropy: Send + Sync {
    fn uuid(&self) -> String;
    fn now(&self) -> DateTime<Utc>;
}

/// Real random UUIDs and the system clock.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemEntropy;

impl Entropy for SystemEntropy {
    fn uuid(&self) -> String {
        uuid::Uuid::new_v4().to_string()
    }

    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}
