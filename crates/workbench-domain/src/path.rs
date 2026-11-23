//! URL path templates such as `/users/:id/posts/:postId`.
//!
//! Both `:name` and OpenAPI-style `{name}` parameters are accepted on input;
//! the canonical form uses `:name`.

use std::collections::HashSet;
use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Segment {
    Static(String),
    Param(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct PathTemplate {
    segments: Vec<Segment>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PathTemplateError {
    #[error("path must start with `/`")]
    MissingLeadingSlash,
    #[error("path contains an empty segment (`//`)")]
    EmptySegment,
    #[error("invalid parameter name `{0}`: use letters, digits and `_`, not starting with a digit")]
    InvalidParamName(String),
    #[error("parameter `{0}` appears more than once")]
    DuplicateParam(String),
    #[error("segment `{0}` has unbalanced or misplaced braces")]
    MalformedSegment(String),
}

fn is_valid_param_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

impl PathTemplate {
    /// Parses and normalizes a template. A trailing slash is ignored.
    pub fn parse(raw: &str) -> Result<Self, PathTemplateError> {
        let raw = raw.trim();
        let rest = raw
            .strip_prefix('/')
            .ok_or(PathTemplateError::MissingLeadingSlash)?;
        let rest = rest.strip_suffix('/').unwrap_or(rest);

        let mut segments = Vec::new();
        let mut seen = HashSet::new();
        if rest.is_empty() {
            return Ok(Self { segments });
        }
        for part in rest.split('/') {
            if part.is_empty() {
                return Err(PathTemplateError::EmptySegment);
            }
            let param = if let Some(name) = part.strip_prefix(':') {
                Some(name)
            } else if let Some(inner) = part.strip_prefix('{') {
                Some(
                    inner
                        .strip_suffix('}')
                        .ok_or_else(|| PathTemplateError::MalformedSegment(part.to_owned()))?,
                )
            } else {
                None
            };
            match param {
                Some(name) => {
                    if !is_valid_param_name(name) {
                        return Err(PathTemplateError::InvalidParamName(name.to_owned()));
                    }
                    if !seen.insert(name.to_owned()) {
                        return Err(PathTemplateError::DuplicateParam(name.to_owned()));
                    }
                    segments.push(Segment::Param(name.to_owned()));
                }
                None => {
                    if part.contains(['{', '}']) {
                        return Err(PathTemplateError::MalformedSegment(part.to_owned()));
                    }
                    segments.push(Segment::Static(part.to_owned()));
                }
            }
        }
        Ok(Self { segments })
    }

    #[must_use]
    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }

    /// Parameter names in order of appearance.
    pub fn params(&self) -> impl Iterator<Item = &str> {
        self.segments.iter().filter_map(|s| match s {
            Segment::Param(name) => Some(name.as_str()),
            Segment::Static(_) => None,
        })
    }

    #[must_use]
    pub fn has_param(&self, name: &str) -> bool {
        self.params().any(|p| p == name)
    }

    /// The template with parameter names erased, e.g. `/users/:`.
    /// Two templates with the same shape match exactly the same URLs.
    #[must_use]
    pub fn shape(&self) -> String {
        self.render(|_| ":".to_owned())
    }

    /// OpenAPI form, e.g. `/users/{id}`.
    #[must_use]
    pub fn to_openapi(&self) -> String {
        self.render(|name| format!("{{{name}}}"))
    }

    fn render(&self, param: impl Fn(&str) -> String) -> String {
        if self.segments.is_empty() {
            return "/".to_owned();
        }
        let mut out = String::new();
        for seg in &self.segments {
            out.push('/');
            match seg {
                Segment::Static(s) => out.push_str(s),
                Segment::Param(name) => out.push_str(&param(name)),
            }
        }
        out
    }
}

impl fmt::Display for PathTemplate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.render(|name| format!(":{name}")))
    }
}

impl TryFrom<String> for PathTemplate {
    type Error = PathTemplateError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<PathTemplate> for String {
    fn from(value: PathTemplate) -> Self {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_static_and_params() {
        let t = PathTemplate::parse("/users/:id/posts/{postId}/").unwrap();
        assert_eq!(t.to_string(), "/users/:id/posts/:postId");
        assert_eq!(t.params().collect::<Vec<_>>(), ["id", "postId"]);
        assert_eq!(t.shape(), "/users/:/posts/:");
        assert_eq!(t.to_openapi(), "/users/{id}/posts/{postId}");
    }

    #[test]
    fn root_path() {
        let t = PathTemplate::parse("/").unwrap();
        assert!(t.segments().is_empty());
        assert_eq!(t.to_string(), "/");
    }

    #[test]
    fn rejects_bad_templates() {
        use PathTemplateError::*;
        assert_eq!(PathTemplate::parse("users"), Err(MissingLeadingSlash));
        assert_eq!(PathTemplate::parse("/a//b"), Err(EmptySegment));
        assert_eq!(
            PathTemplate::parse("/a/:1x"),
            Err(InvalidParamName("1x".into()))
        );
        assert_eq!(
            PathTemplate::parse("/a/:id/b/:id"),
            Err(DuplicateParam("id".into()))
        );
        assert_eq!(
            PathTemplate::parse("/a/{id"),
            Err(MalformedSegment("{id".into()))
        );
        assert_eq!(
            PathTemplate::parse("/a/x{id}"),
            Err(MalformedSegment("x{id}".into()))
        );
    }

    #[test]
    fn serde_round_trip_and_validation() {
        let t: PathTemplate = serde_json::from_str("\"/users/{id}\"").unwrap();
        assert_eq!(serde_json::to_string(&t).unwrap(), "\"/users/:id\"");
        assert!(serde_json::from_str::<PathTemplate>("\"nope\"").is_err());
    }

    #[test]
    fn same_shape_for_differently_named_params() {
        let a = PathTemplate::parse("/users/:id").unwrap();
        let b = PathTemplate::parse("/users/:userId").unwrap();
        assert_eq!(a.shape(), b.shape());
    }
}
