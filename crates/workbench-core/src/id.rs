//! Strongly typed UUID identifiers.
//!
//! Each resource gets its own newtype so an `EndpointId` can never be passed
//! where a `ProjectId` is expected.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! define_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            /// Creates a new random identifier.
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }

            /// Wraps an existing UUID.
            #[must_use]
            pub const fn from_uuid(uuid: Uuid) -> Self {
                Self(uuid)
            }

            /// Returns the underlying UUID.
            #[must_use]
            pub const fn as_uuid(&self) -> &Uuid {
                &self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }

        impl FromStr for $name {
            type Err = uuid::Error;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Uuid::parse_str(s).map(Self)
            }
        }
    };
}

define_id!(
    /// Identifies a workspace.
    WorkspaceId
);
define_id!(
    /// Identifies an API project.
    ProjectId
);
define_id!(
    /// Identifies an endpoint definition.
    EndpointId
);
define_id!(
    /// Identifies a project environment.
    EnvironmentId
);
define_id!(
    /// Identifies a running mock server instance.
    MockServerId
);
define_id!(
    /// Identifies a single handled request (used for tracing and error bodies).
    RequestId
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique() {
        assert_ne!(EndpointId::new(), EndpointId::new());
    }

    #[test]
    fn id_round_trips_through_string_and_json() {
        let id = ProjectId::new();
        let parsed: ProjectId = id.to_string().parse().unwrap();
        assert_eq!(id, parsed);

        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, format!("\"{id}\""));
        let back: ProjectId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, back);
    }

    #[test]
    fn invalid_id_is_rejected() {
        assert!("not-a-uuid".parse::<EndpointId>().is_err());
    }
}
