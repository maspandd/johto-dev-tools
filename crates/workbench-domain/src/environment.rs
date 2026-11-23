//! Project environments such as Development and Testing (spec §4.9).
//!
//! Only non-secret values live here. Secret variables are stored
//! separately and encrypted (planned for the management API phase).

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use workbench_core::{BindAddress, EnvironmentId, ProjectId, time};

use crate::validation::{Errors, FieldError, is_identifier};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Environment {
    pub id: EnvironmentId,
    pub project_id: ProjectId,
    pub name: String,
    /// Base URL used by the built-in request client.
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub default_headers: BTreeMap<String, String>,
    /// Non-secret variables. Names are identifiers (`API_VERSION`).
    #[serde(default)]
    pub variables: BTreeMap<String, String>,
    /// Where this environment's mock server listens.
    #[serde(default)]
    pub bind: BindAddress,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Environment {
    #[must_use]
    pub fn new(project_id: ProjectId, name: impl Into<String>) -> Self {
        let now = time::now();
        Self {
            id: EnvironmentId::new(),
            project_id,
            name: name.into().trim().to_owned(),
            base_url: None,
            default_headers: BTreeMap::new(),
            variables: BTreeMap::new(),
            bind: BindAddress::default(),
            created_at: now,
            updated_at: now,
        }
    }

    pub fn validate(&self) -> Result<(), Vec<FieldError>> {
        let mut e = Errors::default();
        e.name("name", &self.name);
        e.headers("default_headers", &self.default_headers);
        for name in self.variables.keys() {
            if !is_identifier(name) {
                e.push(
                    format!("variables.{name}"),
                    "use letters, digits and `_`, not starting with a digit",
                );
            }
        }
        if let Some(url) = &self.base_url
            && !(url.starts_with("http://") || url.starts_with("https://"))
        {
            e.push("base_url", "must start with http:// or https://");
        }
        if self.bind.validate().is_err() {
            e.push(
                "bind.host",
                "non-loopback hosts require allow_network_exposure",
            );
        }
        e.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    #[test]
    fn defaults_are_valid_and_loopback() {
        let env = Environment::new(ProjectId::new(), "Development");
        assert!(env.validate().is_ok());
        assert!(env.bind.host.is_loopback());
    }

    #[test]
    fn rejects_bad_fields() {
        let mut env = Environment::new(ProjectId::new(), "Dev");
        env.variables.insert("1bad".into(), "x".into());
        env.base_url = Some("ftp://x".into());
        env.bind.host = IpAddr::V4(Ipv4Addr::UNSPECIFIED);
        let fields: Vec<_> = env
            .validate()
            .unwrap_err()
            .into_iter()
            .map(|e| e.field)
            .collect();
        assert_eq!(fields, ["variables.1bad", "base_url", "bind.host"]);
    }
}
