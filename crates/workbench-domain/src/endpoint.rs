//! Endpoint definitions and their validation (spec §4.2).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use workbench_core::EndpointId;

use crate::validation::is_valid_header_name;
use crate::{Method, MockResponse, PathTemplate, ResponseRule, ValueSource};

/// A user-defined mock endpoint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Endpoint {
    #[serde(default)]
    pub id: EndpointId,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub method: Method,
    pub path: PathTemplate,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub request: RequestSpec,
    /// Returned when no rule matches.
    #[serde(default)]
    pub response: MockResponse,
    #[serde(default)]
    pub rules: Vec<ResponseRule>,
    #[serde(default)]
    pub auth: AuthSimulation,
}

const fn default_enabled() -> bool {
    true
}

/// What the endpoint expects from a request.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RequestSpec {
    #[serde(default)]
    pub headers: Vec<ParamSpec>,
    #[serde(default)]
    pub query: Vec<ParamSpec>,
    /// JSON Schema the request body must satisfy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_schema: Option<Value>,
    /// Example body shown in the UI and exported to OpenAPI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_example: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParamSpec {
    pub name: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub description: String,
}

/// Simulated authentication. Any credential is accepted; only its presence is checked.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AuthSimulation {
    #[default]
    None,
    /// Requires `Authorization: Bearer <token>`.
    Bearer,
    /// Requires a non-empty value in the named header.
    ApiKey { header: String },
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum EndpointValidationError {
    #[error("endpoint name must not be empty")]
    EmptyName,
    #[error("{context}: status {status} is outside 100-599")]
    InvalidStatus { context: String, status: u16 },
    #[error("{context}: invalid header name `{name}`")]
    InvalidHeaderName { context: String, name: String },
    #[error("rule `{rule}` has no conditions")]
    RuleWithoutConditions { rule: String },
    #[error("rule `{rule}`: operator `{operator}` requires a value")]
    MissingConditionValue { rule: String, operator: String },
    #[error("rule `{rule}`: path parameter `{name}` is not in the route template")]
    UnknownPathParam { rule: String, name: String },
    #[error("rule `{rule}`: `{pointer}` is not a JSON pointer (must be empty or start with `/`)")]
    InvalidJsonPointer { rule: String, pointer: String },
    #[error("request body schema must be a JSON object or boolean")]
    InvalidBodySchema,
}

fn check_response(
    context: &str,
    response: &MockResponse,
    errors: &mut Vec<EndpointValidationError>,
) {
    if !(100..=599).contains(&response.status) {
        errors.push(EndpointValidationError::InvalidStatus {
            context: context.to_owned(),
            status: response.status,
        });
    }
    for name in response.headers.keys() {
        if !is_valid_header_name(name) {
            errors.push(EndpointValidationError::InvalidHeaderName {
                context: context.to_owned(),
                name: name.clone(),
            });
        }
    }
}

impl Endpoint {
    /// Creates an enabled endpoint with an empty 200 response.
    pub fn new(name: impl Into<String>, method: Method, path: PathTemplate) -> Self {
        Self {
            id: EndpointId::new(),
            name: name.into(),
            description: String::new(),
            method,
            path,
            enabled: true,
            request: RequestSpec::default(),
            response: MockResponse::default(),
            rules: Vec::new(),
            auth: AuthSimulation::None,
        }
    }

    /// Checks everything that can be checked without compiling schemas.
    /// Returns every problem found, not just the first.
    pub fn validate(&self) -> Result<(), Vec<EndpointValidationError>> {
        use EndpointValidationError as E;
        let mut errors = Vec::new();

        if self.name.trim().is_empty() {
            errors.push(E::EmptyName);
        }
        check_response("default response", &self.response, &mut errors);

        for spec in self.request.headers.iter() {
            if !is_valid_header_name(&spec.name) {
                errors.push(E::InvalidHeaderName {
                    context: "request headers".to_owned(),
                    name: spec.name.clone(),
                });
            }
        }
        if let AuthSimulation::ApiKey { header } = &self.auth
            && !is_valid_header_name(header)
        {
            errors.push(E::InvalidHeaderName {
                context: "api key auth".to_owned(),
                name: header.clone(),
            });
        }
        if let Some(schema) = &self.request.body_schema
            && !(schema.is_object() || schema.is_boolean())
        {
            errors.push(E::InvalidBodySchema);
        }

        for rule in &self.rules {
            check_response(
                &format!("rule `{}`", rule.name),
                &rule.response,
                &mut errors,
            );
            if rule.when.is_empty() {
                errors.push(E::RuleWithoutConditions {
                    rule: rule.name.clone(),
                });
            }
            for cond in &rule.when {
                if cond.operator.requires_value() && cond.value.is_none() {
                    errors.push(E::MissingConditionValue {
                        rule: rule.name.clone(),
                        operator: format!("{:?}", cond.operator).to_lowercase(),
                    });
                }
                match &cond.source {
                    ValueSource::Path { name } if !self.path.has_param(name) => {
                        errors.push(E::UnknownPathParam {
                            rule: rule.name.clone(),
                            name: name.clone(),
                        });
                    }
                    ValueSource::Body { pointer }
                        if !pointer.is_empty() && !pointer.starts_with('/') =>
                    {
                        errors.push(E::InvalidJsonPointer {
                            rule: rule.name.clone(),
                            pointer: pointer.clone(),
                        });
                    }
                    _ => {}
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

/// Two or more endpoints that answer the same method and URL shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteConflict {
    pub method: Method,
    /// Shape with parameter names erased, e.g. `/users/:`.
    pub shape: String,
    pub endpoints: Vec<EndpointId>,
}

/// Finds endpoints that would answer the same requests.
/// `/users/:id` and `/users/:userId` conflict; `/users/me` and `/users/:id` do not
/// (the static route wins at match time).
pub fn find_route_conflicts<'a>(
    endpoints: impl IntoIterator<Item = &'a Endpoint>,
) -> Vec<RouteConflict> {
    let mut groups: BTreeMap<(Method, String), Vec<EndpointId>> = BTreeMap::new();
    for ep in endpoints {
        groups
            .entry((ep.method, ep.path.shape()))
            .or_default()
            .push(ep.id);
    }
    groups
        .into_iter()
        .filter(|(_, ids)| ids.len() > 1)
        .map(|((method, shape), endpoints)| RouteConflict {
            method,
            shape,
            endpoints,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Condition, Operator};

    fn ep(method: Method, path: &str) -> Endpoint {
        Endpoint::new("test", method, PathTemplate::parse(path).unwrap())
    }

    #[test]
    fn valid_endpoint_passes() {
        assert!(ep(Method::Get, "/users").validate().is_ok());
    }

    #[test]
    fn collects_all_errors() {
        let mut e = ep(Method::Get, "/users/:id");
        e.name = " ".into();
        e.response.status = 42;
        e.response.headers.insert("bad header".into(), "x".into());
        e.request.body_schema = Some(serde_json::json!("string"));
        e.rules.push(ResponseRule {
            name: "r".into(),
            when: vec![
                Condition {
                    source: ValueSource::Path {
                        name: "nope".into(),
                    },
                    operator: Operator::Equals,
                    value: None,
                },
                Condition {
                    source: ValueSource::Body {
                        pointer: "user.name".into(),
                    },
                    operator: Operator::Exists,
                    value: None,
                },
            ],
            response: MockResponse::default(),
        });
        let errs = e.validate().unwrap_err();
        assert!(errs.contains(&EndpointValidationError::EmptyName));
        assert!(
            errs.iter()
                .any(|e| matches!(e, EndpointValidationError::InvalidStatus { status: 42, .. }))
        );
        assert!(
            errs.iter()
                .any(|e| matches!(e, EndpointValidationError::InvalidHeaderName { .. }))
        );
        assert!(errs.contains(&EndpointValidationError::InvalidBodySchema));
        assert!(
            errs.iter()
                .any(|e| matches!(e, EndpointValidationError::MissingConditionValue { .. }))
        );
        assert!(
            errs.iter()
                .any(|e| matches!(e, EndpointValidationError::UnknownPathParam { .. }))
        );
        assert!(
            errs.iter()
                .any(|e| matches!(e, EndpointValidationError::InvalidJsonPointer { .. }))
        );
    }

    #[test]
    fn rule_without_conditions_is_rejected() {
        let mut e = ep(Method::Get, "/a");
        e.rules.push(ResponseRule {
            name: "always".into(),
            when: vec![],
            response: MockResponse::default(),
        });
        assert_eq!(
            e.validate().unwrap_err(),
            vec![EndpointValidationError::RuleWithoutConditions {
                rule: "always".into()
            }]
        );
    }

    #[test]
    fn detects_duplicate_method_and_shape() {
        let a = ep(Method::Get, "/users/:id");
        let b = ep(Method::Get, "/users/:userId");
        let c = ep(Method::Post, "/users/:id");
        let d = ep(Method::Get, "/users/me");
        let conflicts = find_route_conflicts([&a, &b, &c, &d]);
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].method, Method::Get);
        assert_eq!(conflicts[0].shape, "/users/:");
        assert_eq!(conflicts[0].endpoints, vec![a.id, b.id]);
    }

    #[test]
    fn deserializes_minimal_endpoint() {
        let e: Endpoint = serde_json::from_value(serde_json::json!({
            "name": "List users",
            "method": "GET",
            "path": "/users"
        }))
        .unwrap();
        assert!(e.enabled);
        assert_eq!(e.response.status, 200);
        assert_eq!(e.auth, AuthSimulation::None);
    }
}
