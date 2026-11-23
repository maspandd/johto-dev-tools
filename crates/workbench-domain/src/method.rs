//! HTTP methods supported by the MVP (spec §4.2).

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Method {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

#[derive(Debug, Error, PartialEq, Eq)]
#[error("unsupported HTTP method `{0}`; expected one of GET, POST, PUT, PATCH, DELETE")]
pub struct ParseMethodError(pub String);

impl Method {
    pub const ALL: [Self; 5] = [Self::Get, Self::Post, Self::Put, Self::Patch, Self::Delete];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
            Self::Delete => "DELETE",
        }
    }

    /// Parses a method from an incoming HTTP request line.
    /// HTTP methods are case-sensitive, so only the uppercase form matches.
    #[must_use]
    pub fn from_http(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.as_str() == s)
    }
}

impl fmt::Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Lenient parse for user-authored configuration (case-insensitive).
impl FromStr for Method {
    type Err = ParseMethodError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|m| m.as_str().eq_ignore_ascii_case(s.trim()))
            .ok_or_else(|| ParseMethodError(s.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_case_insensitively_from_config() {
        assert_eq!("patch".parse::<Method>().unwrap(), Method::Patch);
        assert!("HEAD".parse::<Method>().is_err());
    }

    #[test]
    fn http_parse_is_case_sensitive() {
        assert_eq!(Method::from_http("GET"), Some(Method::Get));
        assert_eq!(Method::from_http("get"), None);
    }

    #[test]
    fn serializes_uppercase() {
        assert_eq!(
            serde_json::to_string(&Method::Delete).unwrap(),
            "\"DELETE\""
        );
    }
}
