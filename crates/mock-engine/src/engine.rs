//! The request pipeline that ties routing, validation, rules and templates together.

use std::collections::BTreeMap;
use std::fmt::{self, Display};
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use thiserror::Error;
use workbench_core::{EndpointId, ErrorCode, ErrorDetail, ErrorEnvelope};
use workbench_domain::{
    AuthSimulation, Endpoint, EndpointValidationError, Method, MockResponse, PathTemplate,
    ResponseBody, RouteConflict, Segment, find_route_conflicts,
};

use crate::request::path_segments;
use crate::router::{Lookup, RouteTable};
use crate::rules::RuleInput;
use crate::template::TemplateContext;
use crate::{Entropy, MockRequest, SystemEntropy};

#[derive(Debug, Clone, Default)]
pub struct EngineConfig {
    /// Optional prefix for every route, e.g. `/mock`. Must not contain parameters.
    pub base_path: Option<String>,
    /// Headers added to every response unless the response already sets them
    /// (project-level defaults, spec §4.1).
    pub default_headers: BTreeMap<String, String>,
}

/// The result of handling one request.
#[derive(Debug, Clone, PartialEq)]
pub struct MockOutcome {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// Simulated latency the runtime must wait before responding.
    pub delay: Duration,
    pub matched_endpoint: Option<EndpointId>,
    pub matched_rule: Option<String>,
    /// Set when the engine itself produced an error response.
    pub error: Option<ErrorCode>,
}

impl MockOutcome {
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

fn join<T: Display>(items: &[T]) -> String {
    items
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}

/// One reason an engine could not be built.
#[derive(Debug, Error)]
pub enum BuildProblem {
    #[error("endpoint `{name}` ({id}): {}", join(.errors))]
    InvalidEndpoint {
        id: EndpointId,
        name: String,
        errors: Vec<EndpointValidationError>,
    },
    #[error("endpoint `{name}` ({id}): invalid request body schema: {message}")]
    InvalidSchema {
        id: EndpointId,
        name: String,
        message: String,
    },
    #[error("route conflict: {} {} is defined by {} enabled endpoints", .0.method, .0.shape, .0.endpoints.len())]
    Conflict(RouteConflict),
    #[error("invalid base path `{path}`: {message}")]
    InvalidBasePath { path: String, message: String },
}

/// Every problem found while building an engine.
#[derive(Debug, Error)]
pub struct BuildErrors(pub Vec<BuildProblem>);

impl Display for BuildErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} problem(s): {}", self.0.len(), join(&self.0))
    }
}

struct CompiledEndpoint {
    endpoint: Endpoint,
    schema: Option<jsonschema::Validator>,
}

/// An immutable, compiled set of routes. Rebuild it to apply configuration changes.
pub struct Engine {
    base: Vec<String>,
    default_headers: Vec<(String, String)>,
    endpoints: Vec<CompiledEndpoint>,
    routes: RouteTable,
    entropy: Arc<dyn Entropy>,
}

impl fmt::Debug for Engine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Engine")
            .field("base", &self.base)
            .field("endpoints", &self.endpoints.len())
            .finish_non_exhaustive()
    }
}

fn parse_base_path(raw: Option<&str>) -> Result<Vec<String>, BuildProblem> {
    let Some(raw) = raw.filter(|s| !s.trim().is_empty()) else {
        return Ok(Vec::new());
    };
    let invalid = |message: String| BuildProblem::InvalidBasePath {
        path: raw.to_owned(),
        message,
    };
    let template = PathTemplate::parse(raw).map_err(|e| invalid(e.to_string()))?;
    template
        .segments()
        .iter()
        .map(|seg| match seg {
            Segment::Static(s) => Ok(s.clone()),
            Segment::Param(_) => Err(invalid("parameters are not allowed".to_owned())),
        })
        .collect()
}

impl Engine {
    /// Compiles the enabled endpoints. Disabled endpoints are skipped entirely,
    /// so they neither respond nor cause conflicts.
    pub fn build(config: &EngineConfig, endpoints: &[Endpoint]) -> Result<Self, BuildErrors> {
        let mut problems = Vec::new();
        let base = parse_base_path(config.base_path.as_deref()).unwrap_or_else(|p| {
            problems.push(p);
            Vec::new()
        });

        let enabled: Vec<&Endpoint> = endpoints.iter().filter(|e| e.enabled).collect();
        problems.extend(
            find_route_conflicts(enabled.iter().copied())
                .into_iter()
                .map(BuildProblem::Conflict),
        );

        let mut compiled = Vec::with_capacity(enabled.len());
        let mut routes = RouteTable::default();
        for endpoint in enabled {
            if let Err(errors) = endpoint.validate() {
                problems.push(BuildProblem::InvalidEndpoint {
                    id: endpoint.id,
                    name: endpoint.name.clone(),
                    errors,
                });
                continue;
            }
            let schema = match &endpoint.request.body_schema {
                None => None,
                Some(schema) => match jsonschema::validator_for(schema) {
                    Ok(v) => Some(v),
                    Err(e) => {
                        problems.push(BuildProblem::InvalidSchema {
                            id: endpoint.id,
                            name: endpoint.name.clone(),
                            message: e.to_string(),
                        });
                        continue;
                    }
                },
            };
            routes.insert(
                endpoint.method,
                endpoint.path.segments().to_vec(),
                compiled.len(),
            );
            compiled.push(CompiledEndpoint {
                endpoint: endpoint.clone(),
                schema,
            });
        }

        if !problems.is_empty() {
            return Err(BuildErrors(problems));
        }
        Ok(Self {
            base,
            default_headers: config
                .default_headers
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            endpoints: compiled,
            routes,
            entropy: Arc::new(SystemEntropy),
        })
    }

    /// Replaces the source of random IDs and time (for deterministic tests).
    #[must_use]
    pub fn with_entropy(mut self, entropy: Arc<dyn Entropy>) -> Self {
        self.entropy = entropy;
        self
    }

    /// Number of active (enabled) endpoints.
    #[must_use]
    pub fn route_count(&self) -> usize {
        self.endpoints.len()
    }

    /// Runs the full pipeline for one request. Never panics on bad input.
    #[must_use]
    pub fn handle(&self, req: &MockRequest) -> MockOutcome {
        let mut out = self.route(req);
        for (name, value) in &self.default_headers {
            if !has_header(&out.headers, name) {
                out.headers.push((name.clone(), value.clone()));
            }
        }
        out
    }

    fn route(&self, req: &MockRequest) -> MockOutcome {
        let segments = path_segments(&req.path);
        let Some(rest) = segments.strip_prefix(self.base.as_slice()) else {
            return self.not_found(req);
        };
        match self.routes.lookup(Method::from_http(&req.method), rest) {
            Lookup::NotFound => self.not_found(req),
            Lookup::MethodNotAllowed { allowed } => {
                let allow = allowed
                    .iter()
                    .map(|m| m.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                let mut out = error_outcome(
                    req,
                    ErrorCode::MethodNotAllowed,
                    format!(
                        "{} is not allowed for {}. Allowed: {allow}.",
                        req.method, req.path
                    ),
                    Vec::new(),
                );
                out.headers.push(("allow".to_owned(), allow));
                out
            }
            Lookup::Found { target, params } => self.execute(&self.endpoints[target], req, &params),
        }
    }

    fn not_found(&self, req: &MockRequest) -> MockOutcome {
        error_outcome(
            req,
            ErrorCode::RouteNotFound,
            format!("No enabled mock route matches {} {}.", req.method, req.path),
            Vec::new(),
        )
    }

    fn execute(
        &self,
        compiled: &CompiledEndpoint,
        req: &MockRequest,
        params: &BTreeMap<String, String>,
    ) -> MockOutcome {
        let ep = &compiled.endpoint;
        let tag = |mut out: MockOutcome| {
            out.matched_endpoint = Some(ep.id);
            out
        };

        if let Some(message) = check_auth(&ep.auth, req) {
            return tag(error_outcome(
                req,
                ErrorCode::Unauthorized,
                message,
                Vec::new(),
            ));
        }

        let mut details = Vec::new();
        for spec in ep.request.headers.iter().filter(|s| s.required) {
            if req.header(&spec.name).is_none() {
                details.push(ErrorDetail::new(
                    format!("header.{}", spec.name.to_ascii_lowercase()),
                    "required header is missing",
                ));
            }
        }
        for spec in ep.request.query.iter().filter(|s| s.required) {
            if req.query_param(&spec.name).is_none() {
                details.push(ErrorDetail::new(
                    format!("query.{}", spec.name),
                    "required query parameter is missing",
                ));
            }
        }

        let body = match parse_body(req, compiled.schema.is_some()) {
            Ok(body) => body,
            Err(message) => {
                return tag(error_outcome(
                    req,
                    ErrorCode::InvalidJsonBody,
                    "The request body is not valid JSON.",
                    vec![ErrorDetail::new("body", message)],
                ));
            }
        };

        if let Some(schema) = &compiled.schema {
            match &body {
                None => details.push(ErrorDetail::new("body", "request body is required")),
                Some(value) => details.extend(schema.iter_errors(value).map(|e| {
                    ErrorDetail::new(format!("body{}", e.instance_path()), e.to_string())
                })),
            }
        }

        if !details.is_empty() {
            return tag(error_outcome(
                req,
                ErrorCode::RequestValidationFailed,
                "The request did not pass validation.",
                details,
            ));
        }

        let rule = RuleInput {
            request: req,
            params,
            body: body.as_ref(),
        }
        .first_match(&ep.rules);
        let response = rule.map_or(&ep.response, |r| &r.response);

        let ctx = TemplateContext {
            request: req,
            params,
            body: body.as_ref(),
            entropy: self.entropy.as_ref(),
        };
        let mut out = render(response, &ctx);
        out.matched_rule = rule.map(|r| r.name.clone());
        tag(out)
    }
}

fn check_auth(auth: &AuthSimulation, req: &MockRequest) -> Option<String> {
    match auth {
        AuthSimulation::None => None,
        AuthSimulation::Bearer => {
            let ok = req.header("authorization").is_some_and(|v| {
                v.split_once(' ').is_some_and(|(scheme, token)| {
                    scheme.eq_ignore_ascii_case("bearer") && !token.trim().is_empty()
                })
            });
            (!ok).then(|| {
                "This endpoint requires an `Authorization: Bearer <token>` header.".to_owned()
            })
        }
        AuthSimulation::ApiKey { header } => {
            let ok = req.header(header).is_some_and(|v| !v.trim().is_empty());
            (!ok).then(|| format!("This endpoint requires a non-empty `{header}` header."))
        }
    }
}

/// Parses the body as JSON when it is declared as JSON or a schema expects it.
/// Otherwise JSON parsing is best-effort so rules and templates can still read it.
fn parse_body(req: &MockRequest, schema_expected: bool) -> Result<Option<Value>, String> {
    if req.body.is_empty() {
        return Ok(None);
    }
    let declared_json = req
        .header("content-type")
        .is_some_and(|ct| ct.to_ascii_lowercase().contains("json"));
    if declared_json || schema_expected {
        serde_json::from_slice(&req.body)
            .map(Some)
            .map_err(|e| e.to_string())
    } else {
        Ok(serde_json::from_slice(&req.body).ok())
    }
}

fn has_header(headers: &[(String, String)], name: &str) -> bool {
    headers.iter().any(|(k, _)| k.eq_ignore_ascii_case(name))
}

fn render(response: &MockResponse, ctx: &TemplateContext<'_>) -> MockOutcome {
    let mut headers: Vec<(String, String)> = response
        .headers
        .iter()
        .map(|(k, v)| (k.clone(), ctx.render_str(v)))
        .collect();
    let (body, default_type) = match &response.body {
        ResponseBody::Empty => (Vec::new(), None),
        ResponseBody::Json(value) => (
            serde_json::to_vec(&ctx.render_json(value)).unwrap_or_default(),
            Some("application/json"),
        ),
        ResponseBody::Text(text) => (
            ctx.render_str(text).into_bytes(),
            Some("text/plain; charset=utf-8"),
        ),
    };
    if let Some(ct) = default_type
        && !has_header(&headers, "content-type")
    {
        headers.push(("content-type".to_owned(), ct.to_owned()));
    }
    MockOutcome {
        status: response.status,
        headers,
        body,
        delay: Duration::from_millis(response.delay_ms),
        matched_endpoint: None,
        matched_rule: None,
        error: None,
    }
}

fn error_outcome(
    req: &MockRequest,
    code: ErrorCode,
    message: impl Into<String>,
    details: Vec<ErrorDetail>,
) -> MockOutcome {
    let envelope = ErrorEnvelope::new(code, message)
        .with_details(details)
        .with_request_id(req.request_id);
    MockOutcome {
        status: code.http_status(),
        headers: vec![("content-type".to_owned(), "application/json".to_owned())],
        body: serde_json::to_vec(&envelope).unwrap_or_default(),
        delay: Duration::ZERO,
        matched_endpoint: None,
        matched_rule: None,
        error: Some(code),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use workbench_domain::{Condition, Operator, ParamSpec, ResponseRule, ValueSource};

    fn endpoint(method: Method, path: &str, response: MockResponse) -> Endpoint {
        let mut e = Endpoint::new(path, method, PathTemplate::parse(path).unwrap());
        e.response = response;
        e
    }

    fn json_body(out: &MockOutcome) -> Value {
        serde_json::from_slice(&out.body).unwrap()
    }

    fn build(endpoints: &[Endpoint]) -> Engine {
        Engine::build(&EngineConfig::default(), endpoints).unwrap()
    }

    #[test]
    fn returns_configured_status_headers_and_body() {
        let mut resp = MockResponse::json(201, json!({ "ok": true }));
        resp.headers.insert("X-Mock".into(), "yes".into());
        resp.delay_ms = 250;
        let e = endpoint(Method::Get, "/health", resp);
        let id = e.id;
        let out = build(&[e]).handle(&MockRequest::new("GET", "/health"));
        assert_eq!(out.status, 201);
        assert_eq!(out.header("x-mock"), Some("yes"));
        assert_eq!(out.header("content-type"), Some("application/json"));
        assert_eq!(json_body(&out), json!({ "ok": true }));
        assert_eq!(out.delay, Duration::from_millis(250));
        assert_eq!(out.matched_endpoint, Some(id));
        assert_eq!(out.error, None);
    }

    #[test]
    fn user_content_type_is_kept() {
        let mut resp = MockResponse::json(200, json!({}));
        resp.headers
            .insert("Content-Type".into(), "application/vnd.api+json".into());
        let out =
            build(&[endpoint(Method::Get, "/a", resp)]).handle(&MockRequest::new("GET", "/a"));
        assert_eq!(out.header("content-type"), Some("application/vnd.api+json"));
        assert_eq!(
            out.headers
                .iter()
                .filter(|(k, _)| k.eq_ignore_ascii_case("content-type"))
                .count(),
            1
        );
    }

    #[test]
    fn not_found_and_method_not_allowed() {
        let engine = build(&[endpoint(Method::Get, "/users", MockResponse::default())]);
        let out = engine.handle(&MockRequest::new("GET", "/nope"));
        assert_eq!(out.status, 404);
        assert_eq!(json_body(&out)["error"]["code"], "ROUTE_NOT_FOUND");

        let out = engine.handle(&MockRequest::new("DELETE", "/users"));
        assert_eq!(out.status, 405);
        assert_eq!(out.header("allow"), Some("GET"));
        assert_eq!(json_body(&out)["error"]["code"], "METHOD_NOT_ALLOWED");
    }

    #[test]
    fn disabled_endpoints_do_not_respond() {
        let mut e = endpoint(Method::Get, "/users", MockResponse::default());
        e.enabled = false;
        let engine = build(&[e]);
        assert_eq!(engine.route_count(), 0);
        assert_eq!(
            engine.handle(&MockRequest::new("GET", "/users")).status,
            404
        );
    }

    #[test]
    fn disabled_duplicates_do_not_conflict() {
        let a = endpoint(Method::Get, "/users/:id", MockResponse::default());
        let mut b = endpoint(Method::Get, "/users/:userId", MockResponse::default());
        b.enabled = false;
        assert!(Engine::build(&EngineConfig::default(), &[a, b]).is_ok());
    }

    #[test]
    fn build_reports_every_problem() {
        let a = endpoint(Method::Get, "/users/:id", MockResponse::default());
        let b = endpoint(Method::Get, "/users/:userId", MockResponse::default());
        let mut c = endpoint(Method::Post, "/users", MockResponse::default());
        c.response.status = 9;
        let mut d = endpoint(Method::Put, "/users", MockResponse::default());
        d.request.body_schema = Some(json!({ "type": "nonsense" }));
        let err = Engine::build(
            &EngineConfig {
                base_path: Some("/mock/:x".into()),
                ..EngineConfig::default()
            },
            &[a, b, c, d],
        )
        .unwrap_err();
        assert_eq!(err.0.len(), 4, "{err}");
        assert!(matches!(err.0[0], BuildProblem::InvalidBasePath { .. }));
        assert!(err.0.iter().any(|p| matches!(p, BuildProblem::Conflict(_))));
        assert!(
            err.0
                .iter()
                .any(|p| matches!(p, BuildProblem::InvalidEndpoint { .. }))
        );
        assert!(
            err.0
                .iter()
                .any(|p| matches!(p, BuildProblem::InvalidSchema { .. }))
        );
    }

    #[test]
    fn base_path_prefixes_all_routes() {
        let engine = Engine::build(
            &EngineConfig {
                base_path: Some("/mock/v1/".into()),
                ..EngineConfig::default()
            },
            &[endpoint(Method::Get, "/users", MockResponse::default())],
        )
        .unwrap();
        assert_eq!(
            engine
                .handle(&MockRequest::new("GET", "/mock/v1/users"))
                .status,
            200
        );
        assert_eq!(
            engine.handle(&MockRequest::new("GET", "/users")).status,
            404
        );
    }

    #[test]
    fn path_params_feed_templates() {
        let e = endpoint(
            Method::Get,
            "/users/:id",
            MockResponse::json(
                200,
                json!({ "data": { "id": "{{path.id}}", "name": "User {{ path.id }}" } }),
            ),
        );
        let out = build(&[e]).handle(&MockRequest::new("GET", "/users/b7e7f6c4"));
        assert_eq!(
            json_body(&out),
            json!({ "data": { "id": "b7e7f6c4", "name": "User b7e7f6c4" } })
        );
    }

    fn create_user() -> Endpoint {
        let mut e = endpoint(
            Method::Post,
            "/users",
            MockResponse::json(
                201,
                json!({ "name": "{{body.name}}", "email": "{{ body.email }}" }),
            ),
        );
        e.request.body_schema = Some(json!({
            "type": "object",
            "required": ["name", "email"],
            "properties": {
                "name": { "type": "string", "minLength": 1 },
                "email": { "type": "string", "format": "email" },
                "age": { "type": "integer", "minimum": 0 }
            }
        }));
        e
    }

    #[test]
    fn valid_body_passes_schema() {
        let req = MockRequest::new("POST", "/users")
            .with_header("content-type", "application/json")
            .with_body(r#"{"name":"Alex","email":"alex@example.test"}"#);
        let out = build(&[create_user()]).handle(&req);
        assert_eq!(out.status, 201, "{}", String::from_utf8_lossy(&out.body));
        assert_eq!(
            json_body(&out),
            json!({ "name": "Alex", "email": "alex@example.test" })
        );
    }

    #[test]
    fn schema_violations_are_reported_with_locations() {
        let req = MockRequest::new("POST", "/users")
            .with_header("content-type", "application/json")
            .with_body(r#"{"name":"","age":-1}"#);
        let out = build(&[create_user()]).handle(&req);
        assert_eq!(out.status, 422);
        let body = json_body(&out);
        assert_eq!(body["error"]["code"], "REQUEST_VALIDATION_FAILED");
        let locations: Vec<&str> = body["error"]["details"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d["location"].as_str().unwrap())
            .collect();
        assert!(locations.contains(&"body"), "{locations:?}"); // missing `email`
        assert!(locations.contains(&"body/name"), "{locations:?}");
        assert!(locations.contains(&"body/age"), "{locations:?}");
    }

    #[test]
    fn missing_and_malformed_bodies() {
        let engine = build(&[create_user()]);
        let out = engine.handle(&MockRequest::new("POST", "/users"));
        assert_eq!(out.status, 422);
        assert_eq!(
            json_body(&out)["error"]["details"][0]["message"],
            "request body is required"
        );

        let out = engine.handle(
            &MockRequest::new("POST", "/users")
                .with_header("content-type", "application/json")
                .with_body("{not json"),
        );
        assert_eq!(out.status, 400);
        assert_eq!(json_body(&out)["error"]["code"], "INVALID_JSON_BODY");
    }

    #[test]
    fn required_headers_and_query_params() {
        let mut e = endpoint(Method::Get, "/search", MockResponse::default());
        e.request.query.push(ParamSpec {
            name: "q".into(),
            required: true,
            description: String::new(),
        });
        e.request.headers.push(ParamSpec {
            name: "X-Tenant".into(),
            required: true,
            description: String::new(),
        });
        let engine = build(&[e]);

        let out = engine.handle(&MockRequest::new("GET", "/search"));
        assert_eq!(out.status, 422);
        let details = json_body(&out)["error"]["details"].clone();
        assert_eq!(details[0]["location"], "header.x-tenant");
        assert_eq!(details[1]["location"], "query.q");

        let ok =
            engine.handle(&MockRequest::new("GET", "/search?q=x").with_header("x-tenant", "a"));
        assert_eq!(ok.status, 200);
    }

    #[test]
    fn rules_override_default_response() {
        let mut e = endpoint(
            Method::Get,
            "/orders/:id",
            MockResponse::json(200, json!({ "status": "ok" })),
        );
        e.rules.push(ResponseRule {
            name: "missing order".into(),
            when: vec![Condition {
                source: ValueSource::Path { name: "id".into() },
                operator: Operator::Equals,
                value: Some("404".into()),
            }],
            response: MockResponse::json(404, json!({ "error": "order {{path.id}} not found" })),
        });
        let engine = build(&[e]);

        let hit = engine.handle(&MockRequest::new("GET", "/orders/404"));
        assert_eq!(hit.status, 404);
        assert_eq!(hit.matched_rule.as_deref(), Some("missing order"));
        assert_eq!(json_body(&hit)["error"], "order 404 not found");

        let miss = engine.handle(&MockRequest::new("GET", "/orders/1"));
        assert_eq!(miss.status, 200);
        assert_eq!(miss.matched_rule, None);
    }

    #[test]
    fn auth_simulation() {
        let mut bearer = endpoint(Method::Get, "/me", MockResponse::default());
        bearer.auth = AuthSimulation::Bearer;
        let mut key = endpoint(Method::Get, "/keyed", MockResponse::default());
        key.auth = AuthSimulation::ApiKey {
            header: "X-Api-Key".into(),
        };
        let engine = build(&[bearer, key]);

        assert_eq!(engine.handle(&MockRequest::new("GET", "/me")).status, 401);
        assert_eq!(
            engine
                .handle(&MockRequest::new("GET", "/me").with_header("Authorization", "Bearer "))
                .status,
            401
        );
        assert_eq!(
            engine
                .handle(&MockRequest::new("GET", "/me").with_header("Authorization", "bearer abc"))
                .status,
            200
        );
        assert_eq!(
            engine.handle(&MockRequest::new("GET", "/keyed")).status,
            401
        );
        assert_eq!(
            engine
                .handle(&MockRequest::new("GET", "/keyed").with_header("x-api-key", "k"))
                .status,
            200
        );
    }

    #[test]
    fn text_and_empty_bodies() {
        let text = MockResponse {
            body: ResponseBody::Text("hello {{query.name}}".into()),
            ..MockResponse::default()
        };
        let engine = build(&[
            endpoint(Method::Get, "/hi", text),
            endpoint(
                Method::Delete,
                "/hi",
                MockResponse {
                    status: 204,
                    ..MockResponse::default()
                },
            ),
        ]);
        let out = engine.handle(&MockRequest::new("GET", "/hi?name=ana"));
        assert_eq!(out.body, b"hello ana");
        assert_eq!(
            out.header("content-type"),
            Some("text/plain; charset=utf-8")
        );

        let out = engine.handle(&MockRequest::new("DELETE", "/hi"));
        assert_eq!(out.status, 204);
        assert!(out.body.is_empty());
        assert_eq!(out.header("content-type"), None);
    }

    #[test]
    fn error_bodies_carry_request_id() {
        let engine = build(&[]);
        let req = MockRequest::new("GET", "/x");
        let out = engine.handle(&req);
        assert_eq!(
            json_body(&out)["error"]["request_id"],
            req.request_id.to_string()
        );
    }

    #[test]
    fn default_headers_apply_unless_overridden() {
        let mut own = MockResponse::default();
        own.headers.insert("X-Mock".into(), "endpoint".into());
        let config = EngineConfig {
            default_headers: BTreeMap::from([
                ("x-mock".to_owned(), "project".to_owned()),
                ("X-Team".to_owned(), "platform".to_owned()),
            ]),
            ..EngineConfig::default()
        };
        let engine = Engine::build(&config, &[endpoint(Method::Get, "/a", own)]).unwrap();

        let out = engine.handle(&MockRequest::new("GET", "/a"));
        assert_eq!(
            out.header("x-mock"),
            Some("endpoint"),
            "endpoint header wins"
        );
        assert_eq!(out.header("x-team"), Some("platform"));
        let count = out
            .headers
            .iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case("x-mock"))
            .count();
        assert_eq!(count, 1);

        let missing = engine.handle(&MockRequest::new("GET", "/nope"));
        assert_eq!(
            missing.header("x-team"),
            Some("platform"),
            "errors get defaults too"
        );
    }
}
