//! Network bind settings with safe defaults (spec §6.9, §9.1).
//!
//! Listeners bind to loopback unless network exposure is explicitly allowed.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Default port for the mock runtime.
pub const DEFAULT_MOCK_PORT: u16 = 4010;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BindAddress {
    pub host: IpAddr,
    /// `0` asks the OS for a free port.
    pub port: u16,
    /// Must be `true` to bind to any non-loopback address.
    #[serde(default)]
    pub allow_network_exposure: bool,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum BindError {
    #[error(
        "refusing to bind to {host}: binding to a non-loopback address requires explicit network exposure"
    )]
    NetworkExposureNotAllowed { host: IpAddr },
}

impl BindAddress {
    /// Loopback on the given port.
    #[must_use]
    pub const fn loopback(port: u16) -> Self {
        Self {
            host: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port,
            allow_network_exposure: false,
        }
    }

    /// Returns the socket address, or an error if it would expose the
    /// listener to the network without consent.
    pub fn validate(&self) -> Result<SocketAddr, BindError> {
        if !self.host.is_loopback() && !self.allow_network_exposure {
            return Err(BindError::NetworkExposureNotAllowed { host: self.host });
        }
        Ok(SocketAddr::new(self.host, self.port))
    }
}

impl Default for BindAddress {
    fn default() -> Self {
        Self::loopback(DEFAULT_MOCK_PORT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv6Addr;

    #[test]
    fn default_is_loopback_4010() {
        let addr = BindAddress::default().validate().unwrap();
        assert_eq!(addr, "127.0.0.1:4010".parse().unwrap());
    }

    #[test]
    fn ipv6_loopback_is_allowed() {
        let b = BindAddress {
            host: IpAddr::V6(Ipv6Addr::LOCALHOST),
            port: 1,
            allow_network_exposure: false,
        };
        assert!(b.validate().is_ok());
    }

    #[test]
    fn non_loopback_requires_opt_in() {
        let mut b = BindAddress {
            host: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            port: 4010,
            allow_network_exposure: false,
        };
        assert!(matches!(
            b.validate(),
            Err(BindError::NetworkExposureNotAllowed { .. })
        ));
        b.allow_network_exposure = true;
        assert!(b.validate().is_ok());
    }
}
