//! OpenAPI 3.x → endpoint drafts.

use std::collections::HashMap;

use serde::Serialize;
use serde_json::{Map, Value};
use workbench_domain::{
    AuthSimulation, Endpoint, MAX_NAME_LEN, Method, MockResponse, ParamSpec, PathTemplate,
    ResponseBody, ResponseRule, find_route_conflicts,
};

use crate::issue::{Issue, Severity, join};
use crate::schema::{is_json_media, json_content, resolve, sample, to_json_schema};

/// Operation keys look like `GET /users/{id}` (method and path as written
/// in the document).
#[must_use]
pub fn operation_key(method: &str, path: &str) -> String {
    format!("{} {path}", method.to_ascii_uppercase())
}

/// What a document contains and how each operation maps to an endpoint.
#[derive(Debug, Clone, Serialize)]
pub struct Analysis {
    /// The `openapi` version string.
    pub openapi: String,
    pub title: String,
    pub description: String,
    /// `info.version`.
    pub version: String,
    /// Path of the first server URL, e.g. `/v1`.
    pub base_path: Option<String>,
    pub operations: Vec<Operation>,
    /// Problems that do not belong to one operation.
    pub issues: Vec<Issue>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Operation {
    pub key: String,
    pub method: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
    pub name: String,
    /// The draft endpoint, or `None` when the operation cannot be imported.
    pub endpoint: Option<Endpoint>,
    pub issues: Vec<Issue>,
}

impl Operation {
    fn matches(&self, selector: &str) -> bool {
        let selector = selector.trim();
        self.operation_id.as_deref() == Some(selector)
            || selector
                .split_once(char::is_whitespace)
                .is_some_and(|(m, p)| m.eq_ignore_ascii_case(&self.method) && p.trim() == self.path)
    }
}

impl Analysis {
    /// Every issue, document-level first.
    pub fn all_issues(&self) -> impl Iterator<Item = &Issue> {
        self.issues
            .iter()
            .chain(self.operations.iter().flat_map(|o| o.issues.iter()))
    }

    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.all_issues().any(|i| i.severity == Severity::Error)
    }

    /// The endpoints to import. With `selection` (operation keys or
    /// operation IDs), only those; asking for an unknown operation or one
    /// that cannot be imported is an error.
    pub fn endpoints(&self, selection: Option<&[String]>) -> Result<Vec<Endpoint>, Vec<Issue>> {
        let Some(selection) = selection else {
            return Ok(self
                .operations
                .iter()
                .filter_map(|o| o.endpoint.clone())
                .collect());
        };
        let mut problems = Vec::new();
        let mut chosen = vec![false; self.operations.len()];
        for selector in selection {
            match self.operations.iter().position(|o| o.matches(selector)) {
                Some(i) if self.operations[i].endpoint.is_some() => chosen[i] = true,
                Some(i) => {
                    let op = &self.operations[i];
                    let reason = op
                        .issues
                        .iter()
                        .find(|i| i.severity == Severity::Error)
                        .map_or("it has errors", |i| i.message.as_str());
                    problems.push(Issue::error(
                        op.issues.first().map_or("", |i| i.pointer.as_str()),
                        format!("`{}` cannot be imported: {reason}", op.key),
                    ));
                }
                None => problems.push(Issue::error(
                    "",
                    format!("no operation `{selector}` in the document"),
                )),
            }
        }
        if !problems.is_empty() {
            return Err(problems);
        }
        Ok(self
            .operations
            .iter()
            .zip(chosen)
            .filter(|(_, c)| *c)
            .filter_map(|(o, _)| o.endpoint.clone())
            .collect())
    }
}

const SUPPORTED: &[(&str, Method)] = &[
    ("get", Method::Get),
    ("put", Method::Put),
    ("post", Method::Post),
    ("delete", Method::Delete),
    ("patch", Method::Patch),
];
const UNSUPPORTED: &[&str] = &["head", "options", "trace", "query"];
/// Header parameters OpenAPI says to ignore.
const IGNORED_HEADERS: &[&str] = &["accept", "content-type", "authorization"];

struct Ctx<'a> {
    doc: &'a Value,
    openapi_30: bool,
}

/// Reads an OpenAPI document. Returns `Err` only when the document cannot
/// be read as OpenAPI 3 at all; everything else is reported as issues.
pub fn analyze(doc: &Value) -> Result<Analysis, Vec<Issue>> {
    let openapi_30 = check_version(doc)?;
    let Some(root) = doc.as_object() else {
        return Err(vec![Issue::error("", "document must be an object")]);
    };
    let mut issues = Vec::new();
    let info = root.get("info").and_then(Value::as_object);
    let text = |key: &str| {
        info.and_then(|i| i.get(key))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_owned()
    };
    let mut title = text("title");
    if title.is_empty() {
        issues.push(Issue::warning(
            "/info/title",
            "missing; using `Imported API`",
        ));
        title = "Imported API".to_owned();
    }

    let base_path = base_path(doc, &mut issues);
    if root
        .get("webhooks")
        .is_some_and(|w| w.as_object().is_some_and(|m| !m.is_empty()))
    {
        issues.push(Issue::warning("/webhooks", "webhooks are not imported"));
    }

    let ctx = Ctx { doc, openapi_30 };
    let mut operations = Vec::new();
    match root.get("paths") {
        None => issues.push(Issue::warning("/paths", "the document has no paths")),
        Some(Value::Object(paths)) => {
            if paths.is_empty() {
                issues.push(Issue::warning("/paths", "the document has no paths"));
            }
            for (raw_path, item) in paths {
                if raw_path.starts_with("x-") {
                    continue;
                }
                path_item(&ctx, raw_path, item, &mut operations, &mut issues);
            }
        }
        Some(_) => return Err(vec![Issue::error("/paths", "must be an object")]),
    }
    flag_conflicts(&mut operations);

    Ok(Analysis {
        openapi: root
            .get("openapi")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        title,
        description: text("description"),
        version: text("version"),
        base_path,
        operations,
        issues,
    })
}

/// `Ok(true)` for 3.0, `Ok(false)` for 3.1 and 3.2.
fn check_version(doc: &Value) -> Result<bool, Vec<Issue>> {
    if doc.get("swagger").is_some() {
        return Err(vec![Issue::error(
            "/swagger",
            "Swagger 2.0 is not supported; convert the document to OpenAPI 3 first",
        )]);
    }
    let Some(version) = doc.get("openapi").and_then(Value::as_str) else {
        return Err(vec![Issue::error(
            "/openapi",
            "missing `openapi` version; this does not look like an OpenAPI 3 document",
        )]);
    };
    let mut parts = version.trim().split('.');
    match (parts.next(), parts.next()) {
        (Some("3"), Some("0")) => Ok(true),
        (Some("3"), Some("1" | "2")) => Ok(false),
        _ => Err(vec![Issue::error(
            "/openapi",
            format!("OpenAPI `{version}` is not supported; use 3.0, 3.1 or 3.2"),
        )]),
    }
}

fn base_path(doc: &Value, issues: &mut Vec<Issue>) -> Option<String> {
    let servers = doc.get("servers")?.as_array()?;
    let server = servers.first()?;
    let mut url = server.get("url")?.as_str()?.trim().to_owned();
    while let Some(start) = url.find('{') {
        let Some(len) = url[start..].find('}') else {
            break;
        };
        let name = url[start + 1..start + len].to_owned();
        let Some(default) = server
            .pointer(&format!(
                "/variables/{}/default",
                crate::issue::escape(&name)
            ))
            .and_then(Value::as_str)
        else {
            issues.push(Issue::warning(
                "/servers/0/url",
                format!("server variable `{name}` has no default; base path not set"),
            ));
            return None;
        };
        url.replace_range(start..=start + len, default);
    }
    let path = match url.find("://") {
        Some(i) => url[i + 3..].find('/').map_or("", |j| &url[i + 3 + j..]),
        None if url.starts_with('/') => url.as_str(),
        None => "",
    };
    let path = path.split(['?', '#']).next().unwrap_or_default();
    let path = path.trim_end_matches('/');
    if path.is_empty() {
        return None;
    }
    match PathTemplate::parse(path) {
        Ok(t) if t.params().next().is_none() => {
            if servers.len() > 1 {
                issues.push(Issue::info(
                    "/servers",
                    format!("several servers are listed; the base path `{t}` comes from the first"),
                ));
            }
            Some(t.to_string())
        }
        _ => {
            issues.push(Issue::warning(
                "/servers/0/url",
                format!("server path `{path}` cannot be used as a base path"),
            ));
            None
        }
    }
}

/// `/users/{user-id}` → `/users/{user_id}`. Segments that mix text and
/// parameters (`/files/{name}.json`) are rejected.
fn convert_path(raw: &str) -> Result<(String, Vec<String>), String> {
    let mut renamed = Vec::new();
    let mut out = String::new();
    for segment in raw.split('/').skip(1) {
        out.push('/');
        let inner = segment
            .strip_prefix('{')
            .and_then(|s| s.strip_suffix('}'))
            .filter(|s| !s.contains(['{', '}']));
        match inner {
            Some(name) => {
                let mut clean: String = name
                    .chars()
                    .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                    .collect();
                if clean.is_empty() || clean.starts_with(|c: char| c.is_ascii_digit()) {
                    clean.insert(0, '_');
                }
                if clean != name {
                    renamed.push(format!("`{name}` → `{clean}`"));
                }
                out.push(':');
                out.push_str(&clean);
            }
            None if segment.contains(['{', '}']) => {
                return Err(format!(
                    "segment `{segment}` mixes text and parameters, which is not supported"
                ));
            }
            None => out.push_str(segment),
        }
    }
    if out.is_empty() {
        out.push('/');
    }
    Ok((out, renamed))
}

fn path_item(
    ctx: &Ctx<'_>,
    raw_path: &str,
    item: &Value,
    operations: &mut Vec<Operation>,
    issues: &mut Vec<Issue>,
) {
    let at = join("/paths", raw_path);
    let Some(item) = resolve(ctx.doc, item, &at, issues) else {
        return;
    };
    let Some(item) = item.as_object() else {
        issues.push(Issue::error(at, "path item must be an object"));
        return;
    };
    let template = if raw_path.starts_with('/') {
        convert_path(raw_path).and_then(|(path, renamed)| {
            PathTemplate::parse(&path)
                .map(|t| (t, renamed))
                .map_err(|e| e.to_string())
        })
    } else {
        Err("path must start with `/`".to_owned())
    };
    let shared_params = item
        .get("parameters")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();

    for (key, op) in item {
        let lower = key.to_ascii_lowercase();
        let op_at = join(&at, key);
        if let Some((_, method)) = SUPPORTED.iter().find(|(m, _)| *m == lower) {
            let mut op_issues = Vec::new();
            let operation_id = op
                .get("operationId")
                .and_then(Value::as_str)
                .map(str::to_owned);
            let name = endpoint_name(op, &operation_id, &operation_key(key, raw_path));
            let endpoint = match &template {
                Ok((template, renamed)) => {
                    if !renamed.is_empty() {
                        op_issues.push(Issue::warning(
                            &at,
                            format!("path parameters renamed: {}", renamed.join(", ")),
                        ));
                    }
                    operation(
                        ctx,
                        *method,
                        template,
                        shared_params,
                        op,
                        &op_at,
                        &name,
                        &mut op_issues,
                    )
                }
                Err(message) => {
                    op_issues.push(Issue::error(&at, message.clone()));
                    None
                }
            };
            operations.push(Operation {
                key: operation_key(key, raw_path),
                method: key.to_ascii_uppercase(),
                path: raw_path.to_owned(),
                operation_id,
                name,
                endpoint,
                issues: op_issues,
            });
        } else if UNSUPPORTED.contains(&lower.as_str()) {
            operations.push(Operation {
                key: operation_key(key, raw_path),
                method: key.to_ascii_uppercase(),
                path: raw_path.to_owned(),
                operation_id: op
                    .get("operationId")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                name: endpoint_name(op, &None, &operation_key(key, raw_path)),
                endpoint: None,
                issues: vec![Issue::warning(
                    op_at,
                    format!(
                        "{} operations are not supported; use GET, POST, PUT, PATCH or DELETE",
                        key.to_ascii_uppercase()
                    ),
                )],
            });
        } else if lower == "servers" {
            issues.push(Issue::info(op_at, "path-level servers are ignored"));
        } else if lower == "additionaloperations" {
            issues.push(Issue::warning(
                op_at,
                "additional operations are not supported",
            ));
        }
    }
}

fn endpoint_name(op: &Value, operation_id: &Option<String>, key: &str) -> String {
    let summary = op
        .get("summary")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let name = summary.or(operation_id.as_deref()).unwrap_or(key).trim();
    name.chars().take(MAX_NAME_LEN).collect()
}

#[allow(clippy::too_many_arguments)]
fn operation(
    ctx: &Ctx<'_>,
    method: Method,
    template: &PathTemplate,
    shared_params: &[Value],
    op: &Value,
    at: &str,
    name: &str,
    issues: &mut Vec<Issue>,
) -> Option<Endpoint> {
    let workbench = op.get("x-workbench").and_then(Value::as_object);
    if let Some(resource) = workbench
        .and_then(|w| w.get("resource"))
        .and_then(Value::as_str)
    {
        issues.push(Issue::info(
            at,
            format!(
                "generated from the CRUD resource `{resource}`; import the project archive to restore resources"
            ),
        ));
        return None;
    }
    let template = workbench
        .and_then(|w| w.get("path"))
        .and_then(Value::as_str)
        .and_then(|p| PathTemplate::parse(p).ok())
        .filter(|p| p.shape() == template.shape())
        .unwrap_or_else(|| template.clone());

    let mut ep = Endpoint::new(name, method, template);
    ep.description = op
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_owned();

    parameters(ctx, &mut ep, shared_params, op, at, issues);
    request_body(ctx, &mut ep, op, at, issues);
    responses(ctx, &mut ep, op, workbench, at, issues);
    ep.auth = security(ctx, op, at, issues);
    if op.get("callbacks").is_some() {
        issues.push(Issue::warning(
            join(at, "callbacks"),
            "callbacks are not imported",
        ));
    }
    if let Some(w) = workbench {
        if let Some(enabled) = w.get("enabled").and_then(Value::as_bool) {
            ep.enabled = enabled;
        }
        if let Some(delay) = w.get("delay_ms").and_then(Value::as_u64) {
            ep.response.delay_ms = delay;
        }
    }

    match ep.validate() {
        Ok(()) => Some(ep),
        Err(errors) => {
            for e in errors {
                issues.push(Issue::error(at, e.to_string()));
            }
            None
        }
    }
}

fn parameters(
    ctx: &Ctx<'_>,
    ep: &mut Endpoint,
    shared: &[Value],
    op: &Value,
    at: &str,
    issues: &mut Vec<Issue>,
) {
    let own = op
        .get("parameters")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    // Operation parameters override path-level ones with the same name and location.
    let mut merged: Vec<(String, String, &Value, String)> = Vec::new();
    let sources = shared
        .iter()
        .enumerate()
        .map(|(i, p)| {
            (
                p,
                format!(
                    "{}/parameters/{i}",
                    at.rsplit_once('/').map_or(at, |(p, _)| p)
                ),
            )
        })
        .chain(
            own.iter()
                .enumerate()
                .map(|(i, p)| (p, format!("{at}/parameters/{i}"))),
        );
    for (raw, p_at) in sources {
        let Some(param) = resolve(ctx.doc, raw, &p_at, issues) else {
            continue;
        };
        let (Some(name), Some(location)) = (
            param.get("name").and_then(Value::as_str),
            param.get("in").and_then(Value::as_str),
        ) else {
            issues.push(Issue::error(&p_at, "parameter needs `name` and `in`"));
            continue;
        };
        let entry = (name.to_owned(), location.to_owned(), param, p_at);
        match merged
            .iter_mut()
            .find(|(n, l, _, _)| n == name && l == location)
        {
            Some(slot) => *slot = entry,
            None => merged.push(entry),
        }
    }
    for (name, location, param, p_at) in merged {
        let spec = ParamSpec {
            name: name.clone(),
            required: param
                .get("required")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            description: param
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim()
                .to_owned(),
        };
        match location.as_str() {
            "query" => ep.request.query.push(spec),
            "header" => {
                if !IGNORED_HEADERS.contains(&name.to_ascii_lowercase().as_str()) {
                    ep.request.headers.push(spec);
                }
            }
            "path" => {}
            "cookie" => issues.push(Issue::warning(
                p_at,
                format!("cookie parameter `{name}` is not supported and was skipped"),
            )),
            other => issues.push(Issue::warning(
                p_at,
                format!("parameter location `{other}` is not supported; `{name}` was skipped"),
            )),
        }
    }
}

fn request_body(ctx: &Ctx<'_>, ep: &mut Endpoint, op: &Value, at: &str, issues: &mut Vec<Issue>) {
    let Some(raw) = op.get("requestBody") else {
        return;
    };
    let body_at = join(at, "requestBody");
    let Some(body) = resolve(ctx.doc, raw, &body_at, issues) else {
        return;
    };
    let Some(content) = body.get("content").and_then(Value::as_object) else {
        return;
    };
    let Some((media_at, media)) = json_content(content, &body_at) else {
        if !content.is_empty() {
            let types: Vec<&str> = content.keys().map(String::as_str).collect();
            issues.push(Issue::warning(
                join(&body_at, "content"),
                format!(
                    "request bodies of type {} are accepted but not validated; only JSON bodies are",
                    types.join(", ")
                ),
            ));
        }
        return;
    };
    if let Some(schema) = media.get("schema") {
        ep.request.body_schema = Some(to_json_schema(
            ctx.doc,
            schema,
            ctx.openapi_30,
            &join(&media_at, "schema"),
            issues,
        ));
    }
    ep.request.body_example = media_example(ctx, media, &media_at, issues);
}

/// `example`, or the first of `examples`.
fn media_example(ctx: &Ctx<'_>, media: &Value, at: &str, issues: &mut Vec<Issue>) -> Option<Value> {
    if let Some(example) = media.get("example") {
        return Some(example.clone());
    }
    let (name, raw) = media.get("examples")?.as_object()?.iter().next()?;
    let ex_at = join(&join(at, "examples"), name);
    let example = resolve(ctx.doc, raw, &ex_at, issues)?;
    if let Some(value) = example.get("value").or_else(|| example.get("dataValue")) {
        return Some(value.clone());
    }
    if example.get("externalValue").is_some() {
        issues.push(Issue::warning(
            ex_at,
            "external example values are not fetched",
        ));
    }
    None
}

fn status_of(key: &str) -> Option<u16> {
    if let Ok(n) = key.parse::<u16>() {
        return (100..=599).contains(&n).then_some(n);
    }
    let bytes = key.as_bytes();
    if bytes.len() == 3 && key[1..].eq_ignore_ascii_case("xx") && (b'1'..=b'5').contains(&bytes[0])
    {
        return Some(u16::from(bytes[0] - b'0') * 100);
    }
    (key == "default").then_some(200)
}

fn responses(
    ctx: &Ctx<'_>,
    ep: &mut Endpoint,
    op: &Value,
    workbench: Option<&Map<String, Value>>,
    at: &str,
    issues: &mut Vec<Issue>,
) {
    let resp_at = join(at, "responses");
    let Some(responses) = op
        .get("responses")
        .and_then(Value::as_object)
        .filter(|r| !r.is_empty())
    else {
        issues.push(Issue::warning(
            resp_at,
            "no responses documented; the mock returns an empty 200",
        ));
        return;
    };
    let codes: Vec<(&String, Option<u16>)> = responses
        .keys()
        .filter(|k| !k.starts_with("x-"))
        .map(|k| (k, status_of(k)))
        .collect();
    let preferred = workbench
        .and_then(|w| w.get("default_status"))
        .and_then(Value::as_u64)
        .map(|s| s.to_string());
    let numeric = |range: std::ops::Range<u16>| {
        codes
            .iter()
            .filter(|(k, s)| k.parse::<u16>().is_ok() && s.is_some_and(|s| range.contains(&s)))
            .min_by_key(|(_, s)| *s)
            .map(|(k, _)| (*k).clone())
    };
    let chosen = preferred
        .filter(|p| responses.contains_key(p))
        .or_else(|| numeric(200..300))
        .or_else(|| {
            codes
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("2xx"))
                .map(|(k, _)| (*k).clone())
        })
        .or_else(|| {
            responses
                .contains_key("default")
                .then(|| "default".to_owned())
        })
        .or_else(|| numeric(100..600));
    let Some(chosen) = chosen else {
        issues.push(Issue::error(resp_at, "no usable response status codes"));
        return;
    };
    for (key, status) in &codes {
        if status.is_none() {
            issues.push(Issue::warning(
                join(&resp_at, key),
                format!("`{key}` is not a valid status code"),
            ));
        }
    }
    let status = status_of(&chosen).unwrap_or(200);
    ep.response = response(
        ctx,
        status,
        &responses[&chosen],
        &join(&resp_at, &chosen),
        issues,
    );

    let rules = workbench.and_then(|w| w.get("rules"));
    match rules.map(|r| serde_json::from_value::<Vec<ResponseRule>>(r.clone())) {
        Some(Ok(rules)) => ep.rules = rules,
        Some(Err(e)) => issues.push(Issue::warning(
            join(at, "x-workbench"),
            format!("response rules could not be read: {e}"),
        )),
        None => {
            let others: Vec<&str> = codes
                .iter()
                .filter(|(k, s)| **k != chosen && s.is_some())
                .map(|(k, _)| k.as_str())
                .collect();
            if !others.is_empty() {
                issues.push(Issue::info(
                    &resp_at,
                    format!(
                        "the mock returns `{chosen}`; responses {} are documented but need response rules to be returned",
                        others.join(", ")
                    ),
                ));
            }
        }
    }
}

fn response(
    ctx: &Ctx<'_>,
    status: u16,
    raw: &Value,
    at: &str,
    issues: &mut Vec<Issue>,
) -> MockResponse {
    let mut out = MockResponse {
        status,
        ..MockResponse::default()
    };
    let Some(resp) = resolve(ctx.doc, raw, at, issues) else {
        return out;
    };
    if let Some(headers) = resp.get("headers").and_then(Value::as_object) {
        for (name, raw) in headers {
            if name.eq_ignore_ascii_case("content-type") {
                continue;
            }
            let h_at = join(&join(at, "headers"), name);
            let Some(header) = resolve(ctx.doc, raw, &h_at, issues) else {
                continue;
            };
            let value = header
                .get("example")
                .or_else(|| header.pointer("/schema/example"))
                .or_else(|| header.pointer("/schema/default"))
                .or_else(|| {
                    header
                        .get("examples")
                        .and_then(Value::as_object)
                        .and_then(|m| m.values().next())
                        .and_then(|e| e.get("value"))
                });
            if let Some(value) = value {
                let text = value
                    .as_str()
                    .map_or_else(|| value.to_string(), str::to_owned);
                out.headers.insert(name.clone(), text);
            }
        }
    }
    let Some(content) = resp
        .get("content")
        .and_then(Value::as_object)
        .filter(|c| !c.is_empty())
    else {
        return out;
    };
    if let Some((media_at, media)) = json_content(content, at) {
        let example = media_example(ctx, media, &media_at, issues).or_else(|| {
            media.get("schema").map(|schema| {
                sample(&to_json_schema(
                    ctx.doc,
                    schema,
                    ctx.openapi_30,
                    &join(&media_at, "schema"),
                    issues,
                ))
            })
        });
        if let Some(body) = example {
            out.body = ResponseBody::Json(body);
        }
        return out;
    }
    if let Some((media, value)) = content
        .iter()
        .find(|(k, _)| k.to_ascii_lowercase().starts_with("text/"))
    {
        let media_at = join(&join(at, "content"), media);
        let text = media_example(ctx, value, &media_at, issues)
            .map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_owned))
            .unwrap_or_default();
        if !media.eq_ignore_ascii_case("text/plain") {
            out.headers
                .entry("content-type".to_owned())
                .or_insert_with(|| media.clone());
        }
        out.body = ResponseBody::Text(text);
        return out;
    }
    let types: Vec<&str> = content
        .keys()
        .map(String::as_str)
        .filter(|k| !is_json_media(k))
        .collect();
    issues.push(Issue::warning(
        join(at, "content"),
        format!(
            "response bodies of type {} are not mocked; the body is left empty",
            types.join(", ")
        ),
    ));
    out
}

fn security(ctx: &Ctx<'_>, op: &Value, at: &str, issues: &mut Vec<Issue>) -> AuthSimulation {
    let (requirements, sec_at) = match op.get("security") {
        Some(s) => (s, join(at, "security")),
        None => match ctx.doc.get("security") {
            Some(s) => (s, "/security".to_owned()),
            None => return AuthSimulation::None,
        },
    };
    let Some(requirements) = requirements.as_array() else {
        return AuthSimulation::None;
    };
    // An empty list, or an empty alternative, means no authentication is required.
    let Some(first) = requirements.first().and_then(Value::as_object) else {
        return AuthSimulation::None;
    };
    if first.is_empty()
        || requirements
            .iter()
            .any(|r| r.as_object().is_some_and(Map::is_empty))
    {
        return AuthSimulation::None;
    }
    if first.len() > 1 {
        issues.push(Issue::info(
            &sec_at,
            "several security schemes are required together; only the first supported one is simulated",
        ));
    }
    for name in first.keys() {
        let s_at = format!("/components/securitySchemes/{}", crate::issue::escape(name));
        let Some(raw) = ctx.doc.pointer(&s_at) else {
            issues.push(Issue::error(
                &sec_at,
                format!("security scheme `{name}` is not defined"),
            ));
            continue;
        };
        let Some(scheme) = resolve(ctx.doc, raw, &s_at, issues) else {
            continue;
        };
        let kind = scheme
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        match kind {
            "http" => {
                let http = scheme
                    .get("scheme")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if http.eq_ignore_ascii_case("bearer") {
                    return AuthSimulation::Bearer;
                }
                issues.push(Issue::warning(
                    &s_at,
                    format!("HTTP `{http}` authentication is not simulated"),
                ));
            }
            "apiKey" => {
                let location = scheme.get("in").and_then(Value::as_str).unwrap_or_default();
                let header = scheme
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if location == "header" && !header.is_empty() {
                    return AuthSimulation::ApiKey {
                        header: header.to_owned(),
                    };
                }
                issues.push(Issue::warning(
                    &s_at,
                    format!("API keys in `{location}` are not simulated; only header API keys are"),
                ));
            }
            "oauth2" | "openIdConnect" => {
                issues.push(Issue::info(
                    &s_at,
                    format!("`{name}` ({kind}) is simulated as a bearer token check"),
                ));
                return AuthSimulation::Bearer;
            }
            other => issues.push(Issue::warning(
                &s_at,
                format!("`{other}` security schemes are not simulated"),
            )),
        }
    }
    AuthSimulation::None
}

/// Operations with the same method and path shape cannot both be served;
/// later ones are not imported.
fn flag_conflicts(operations: &mut [Operation]) {
    let index: HashMap<_, _> = operations
        .iter()
        .enumerate()
        .filter_map(|(i, o)| o.endpoint.as_ref().map(|e| (e.id, i)))
        .collect();
    let endpoints: Vec<Endpoint> = operations
        .iter()
        .filter_map(|o| o.endpoint.clone())
        .collect();
    for conflict in find_route_conflicts(&endpoints) {
        let first = index[&conflict.endpoints[0]];
        let first_key = operations[first].key.clone();
        for id in &conflict.endpoints[1..] {
            let op = &mut operations[index[id]];
            op.endpoint = None;
            op.issues.push(Issue::error(
                join("/paths", &op.path),
                format!(
                    "`{}` answers the same requests as `{first_key}` and was not imported",
                    op.key
                ),
            ));
        }
    }
}
