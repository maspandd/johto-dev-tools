//! OpenAPI 3.1 description of the management API, served at
//! `GET /api/v1/openapi.json`.
//!
//! Built from the [`ROUTES`] table. A test requests every documented route
//! against the real router, so the table cannot drift into describing
//! routes that do not exist.

use serde_json::{Map, Value, json};

/// What a successful call returns.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Reply {
    /// 200 `{ "data": ... }`.
    Data,
    /// 200 `{ "data": [...], "meta": {...} }`.
    Page,
    /// 201 `{ "data": ... }`.
    Created,
    /// 204, no body.
    NoContent,
    /// 200 with a file of this media type.
    File(&'static str),
}

/// What the request body is.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Body {
    None,
    /// JSON object; the text says which fields.
    Json(&'static str),
    /// Raw bytes of this media type.
    Raw(&'static str, &'static str),
    /// Optional JSON object.
    OptionalJson(&'static str),
}

pub(crate) struct Route {
    pub method: &'static str,
    pub path: &'static str,
    pub tag: &'static str,
    pub summary: &'static str,
    pub query: &'static [(&'static str, &'static str, &'static str)],
    pub body: Body,
    pub reply: Reply,
}

const PAGE: &[(&str, &str, &str)] = &[
    ("page", "integer", "Page number, from 1"),
    ("limit", "integer", "Page size, at most 200"),
];

macro_rules! route {
    ($method:literal $path:literal, $tag:literal, $summary:literal, $body:expr, $reply:expr) => {
        route!($method $path, $tag, $summary, &[], $body, $reply)
    };
    ($method:literal $path:literal, $tag:literal, $summary:literal, $query:expr, $body:expr, $reply:expr) => {
        Route {
            method: $method,
            path: $path,
            tag: $tag,
            summary: $summary,
            query: $query,
            body: $body,
            reply: $reply,
        }
    };
}

use Body::{Json, None as NoBody, OptionalJson, Raw};
use Reply::{Created, Data, File, NoContent, Page};

pub(crate) const ROUTES: &[Route] = &[
    route!("get" "/openapi.json", "Meta", "This document", NoBody, File("application/json")),
    route!("get" "/workspaces", "Workspaces", "List workspaces", &[("include_archived", "boolean", ""), PAGE[0], PAGE[1]], NoBody, Page),
    route!("post" "/workspaces", "Workspaces", "Create a workspace", Json("`name`"), Created),
    route!("get" "/workspaces/{id}", "Workspaces", "Get a workspace", NoBody, Data),
    route!("patch" "/workspaces/{id}", "Workspaces", "Rename, archive or restore a workspace", Json("`name`?, `archived`?"), Data),
    route!("delete" "/workspaces/{id}", "Workspaces", "Delete a workspace and its projects", NoBody, NoContent),
    route!("get" "/projects", "Projects", "List a workspace's projects", &[("workspace_id", "string", "Required"), ("include_archived", "boolean", ""), PAGE[0], PAGE[1]], NoBody, Page),
    route!("post" "/projects", "Projects", "Create a project", Json("`workspace_id`, `name`, `description`?, `base_path`?, `default_headers`?"), Created),
    route!("post" "/projects/import", "Projects", "Import a project archive as a new project", &[("workspace_id", "string", "Required"), ("preview", "boolean", "Validate and describe without saving"), ("name", "string", "Name for the new project")], Raw("application/zip", "The archive"), Created),
    route!("get" "/projects/{id}", "Projects", "Get a project", NoBody, Data),
    route!("patch" "/projects/{id}", "Projects", "Update, archive or restore a project", Json("JSON merge patch; `archived` (boolean)"), Data),
    route!("delete" "/projects/{id}", "Projects", "Delete a project with everything in it", NoBody, NoContent),
    route!("post" "/projects/{id}/duplicate", "Projects", "Copy a project (not secrets or logs)", OptionalJson("`name`?"), Created),
    route!("get" "/projects/{id}/export", "Projects", "Export a project archive (.zip)", &[("records", "boolean", "Include CRUD resource records")], NoBody, File("application/zip")),
    route!("post" "/projects/{id}/openapi/import", "OpenAPI", "Import endpoints from an OpenAPI 3.x document", Json("`document` (JSON/YAML text or object), `operations`?, `dry_run`?, `set_base_path`?"), Created),
    route!("get" "/projects/{id}/openapi/export", "OpenAPI", "Export the project as OpenAPI 3.1", &[("format", "string", "`yaml` (default) or `json`"), ("version", "string", "`info.version`")], NoBody, File("application/yaml")),
    route!("post" "/openapi/validate", "OpenAPI", "Validate an OpenAPI document", Raw("application/yaml", "The document as JSON or YAML text"), Data),
    route!("post" "/curl/parse", "OpenAPI", "Parse a cURL command into an endpoint draft (never executed)", Json("`command`, `keep_credentials`?, `project_id`?"), Data),
    route!("get" "/projects/{id}/environments", "Environments", "List environments", PAGE, NoBody, Page),
    route!("post" "/projects/{id}/environments", "Environments", "Create an environment", Json("`name`, `base_url`?, `default_headers`?, `variables`?, `bind`?"), Created),
    route!("get" "/environments/{id}", "Environments", "Get an environment", NoBody, Data),
    route!("patch" "/environments/{id}", "Environments", "Update an environment", Json("JSON merge patch"), Data),
    route!("delete" "/environments/{id}", "Environments", "Delete an environment", NoBody, NoContent),
    route!("get" "/environments/{id}/secrets", "Environments", "List secret names (values are never returned)", NoBody, Data),
    route!("put" "/environments/{id}/secrets/{name}", "Environments", "Set a secret (encrypted at rest)", Json("`value`"), NoContent),
    route!("delete" "/environments/{id}/secrets/{name}", "Environments", "Delete a secret", NoBody, NoContent),
    route!("get" "/projects/{id}/endpoints", "Endpoints", "List endpoints in order", PAGE, NoBody, Page),
    route!("post" "/projects/{id}/endpoints", "Endpoints", "Create an endpoint", Json("An endpoint definition: `name`, `method`, `path`, `request`?, `response`?, `rules`?, `auth`?"), Created),
    route!("post" "/projects/{id}/endpoints/reorder", "Endpoints", "Reorder endpoints", Json("`order`: every endpoint ID"), NoContent),
    route!("get" "/endpoints/{id}", "Endpoints", "Get an endpoint", NoBody, Data),
    route!("put" "/endpoints/{id}", "Endpoints", "Replace an endpoint definition", Json("A full endpoint definition (keeps `null` values)"), Data),
    route!("patch" "/endpoints/{id}", "Endpoints", "Update an endpoint", Json("JSON merge patch"), Data),
    route!("delete" "/endpoints/{id}", "Endpoints", "Delete an endpoint", NoBody, NoContent),
    route!("post" "/endpoints/{id}/duplicate", "Endpoints", "Copy an endpoint (the copy starts disabled)", NoBody, Created),
    route!("post" "/endpoints/{id}/enable", "Endpoints", "Enable an endpoint", NoBody, Data),
    route!("post" "/endpoints/{id}/disable", "Endpoints", "Disable an endpoint", NoBody, Data),
    route!("get" "/projects/{id}/resources", "Resources", "List CRUD resources", PAGE, NoBody, Page),
    route!("post" "/projects/{id}/resources", "Resources", "Create a CRUD resource", Json("`name`, `path`, `schema`?, `seed`?, `body_schema`?, `pagination`?, `searchable`?, `delete`?"), Created),
    route!("get" "/resources/{id}", "Resources", "Get a resource", NoBody, Data),
    route!("put" "/resources/{id}", "Resources", "Replace a resource definition (records are kept)", Json("A full resource definition"), Data),
    route!("patch" "/resources/{id}", "Resources", "Update a resource (records are kept)", Json("JSON merge patch"), Data),
    route!("delete" "/resources/{id}", "Resources", "Delete a resource and its records", NoBody, NoContent),
    route!("get" "/resources/{id}/records", "Resources", "Current records", PAGE, NoBody, Page),
    route!("post" "/resources/{id}/reset", "Resources", "Replace records with fresh seed data", NoBody, NoContent),
    route!("get" "/projects/{id}/datasets", "Datasets", "List datasets", PAGE, NoBody, Page),
    route!("post" "/projects/{id}/datasets", "Datasets", "Create a dataset", Json("`name`, `schema`, `count`, `seed`?, `locale`?"), Created),
    route!("post" "/projects/{id}/datasets/preview", "Datasets", "Preview records for an unsaved schema", Json("`schema`, `count`?, `seed`?, `locale`?"), Data),
    route!("get" "/datasets/{id}", "Datasets", "Get a dataset", NoBody, Data),
    route!("put" "/datasets/{id}", "Datasets", "Replace a dataset definition", Json("`name`, `schema`, `count`, `seed`?, `locale`?, `description`?"), Data),
    route!("patch" "/datasets/{id}", "Datasets", "Update a dataset", Json("JSON merge patch"), Data),
    route!("delete" "/datasets/{id}", "Datasets", "Delete a dataset", NoBody, NoContent),
    route!("get" "/datasets/{id}/preview", "Datasets", "Preview generated records", NoBody, Data),
    route!("get" "/datasets/{id}/export", "Datasets", "Download generated records", &[("format", "string", "`json` (default) or `csv`")], NoBody, File("application/json")),
    route!("get" "/projects/{id}/mock-servers", "Mock servers", "A project's running servers", NoBody, Data),
    route!("post" "/projects/{id}/mock-servers", "Mock servers", "Start a mock server", OptionalJson("`port`?, `host`?, `environment_id`?, `allow_network_exposure`?, `cors`? (`enabled`, `allowed_origins`)"), Created),
    route!("get" "/mock-servers", "Mock servers", "Every running server", NoBody, Data),
    route!("get" "/mock-servers/{id}", "Mock servers", "Server status", NoBody, Data),
    route!("patch" "/mock-servers/{id}", "Mock servers", "Reload a server from stored configuration", OptionalJson("Ignored"), Data),
    route!("delete" "/mock-servers/{id}", "Mock servers", "Stop a server", NoBody, NoContent),
    route!("get" "/mock-servers/{id}/logs", "Request logs", "A server's request history", &[("method", "string", ""), ("status", "integer", ""), ("limit", "integer", "")], NoBody, Data),
    route!("get" "/projects/{id}/request-logs", "Request logs", "A project's request history", &[("method", "string", ""), ("status", "integer", ""), ("endpoint_id", "string", ""), ("resource_id", "string", ""), ("server_id", "string", ""), ("since", "string", "RFC 3339"), ("until", "string", "RFC 3339"), ("limit", "integer", "")], NoBody, Data),
    route!("delete" "/projects/{id}/request-logs", "Request logs", "Clear a project's request history", NoBody, Data),
];

fn envelope(data: Value) -> Value {
    json!({ "type": "object", "properties": { "data": data }, "required": ["data"] })
}

/// The document.
pub(crate) fn document() -> Value {
    let mut paths: Map<String, Value> = Map::new();
    for r in ROUTES {
        let mut params: Vec<Value> = r
            .path
            .split('/')
            .filter_map(|s| s.strip_prefix('{').and_then(|s| s.strip_suffix('}')))
            .map(|name| json!({ "name": name, "in": "path", "required": true, "schema": { "type": "string" } }))
            .collect();
        for (name, ty, description) in r.query {
            let mut p = json!({ "name": name, "in": "query", "schema": { "type": ty } });
            if !description.is_empty() {
                p["description"] = json!(description);
            }
            if *description == "Required" {
                p["required"] = json!(true);
            }
            params.push(p);
        }

        let mut op = json!({
            "tags": [r.tag],
            "summary": r.summary,
            "operationId": format!("{}{}", r.method, r.path.replace(['/', '{', '}', '-', '.'], "_")),
        });
        if !params.is_empty() {
            op["parameters"] = Value::Array(params);
        }
        match r.body {
            Body::None => {}
            Body::Json(fields) | Body::OptionalJson(fields) => {
                op["requestBody"] = json!({
                    "required": matches!(r.body, Body::Json(_)),
                    "description": fields,
                    "content": { "application/json": { "schema": { "type": "object" } } }
                });
            }
            Body::Raw(media, description) => {
                op["requestBody"] = json!({
                    "required": true,
                    "description": description,
                    "content": { media: { "schema": { "type": "string", "format": "binary" } } }
                });
            }
        }
        let (status, success) = match r.reply {
            Reply::Data => (
                "200",
                json!({ "description": "OK", "content": { "application/json": { "schema": envelope(json!({})) } } }),
            ),
            Reply::Page => (
                "200",
                json!({ "description": "OK", "content": { "application/json": { "schema": {
                "type": "object",
                "properties": { "data": { "type": "array", "items": {} }, "meta": { "$ref": "#/components/schemas/PageMeta" } },
                "required": ["data", "meta"]
            } } } }),
            ),
            Reply::Created => (
                "201",
                json!({ "description": "Created", "content": { "application/json": { "schema": envelope(json!({})) } } }),
            ),
            Reply::NoContent => ("204", json!({ "description": "No Content" })),
            Reply::File(media) => (
                "200",
                json!({ "description": "A file", "content": { media: { "schema": { "type": "string", "format": "binary" } } } }),
            ),
        };
        op["responses"] = json!({
            status: success,
            "4XX": { "$ref": "#/components/responses/Error" },
            "5XX": { "$ref": "#/components/responses/Error" },
        });
        let item = paths.entry(r.path.to_owned()).or_insert_with(|| json!({}));
        item[r.method] = op;
    }

    json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Developer Workbench management API",
            "version": env!("CARGO_PKG_VERSION"),
            "description": "Configures workspaces, projects, environments, endpoints, CRUD resources and datasets, and runs mock servers. `GET /health` (outside `/api/v1`) needs no token. Every response has an `x-request-id` header.",
        },
        "servers": [{ "url": "/api/v1" }],
        "security": [{ "bearerAuth": [] }],
        "paths": paths,
        "components": {
            "securitySchemes": {
                "bearerAuth": {
                    "type": "http",
                    "scheme": "bearer",
                    "description": "Required only when the server runs with WORKBENCH_API_TOKEN",
                }
            },
            "schemas": {
                "PageMeta": {
                    "type": "object",
                    "properties": {
                        "page": { "type": "integer" },
                        "limit": { "type": "integer" },
                        "total": { "type": "integer" },
                        "total_pages": { "type": "integer" }
                    }
                },
                "Error": {
                    "type": "object",
                    "properties": {
                        "error": {
                            "type": "object",
                            "properties": {
                                "code": { "type": "string" },
                                "message": { "type": "string" },
                                "details": { "type": "array", "items": {
                                    "type": "object",
                                    "properties": { "location": { "type": "string" }, "message": { "type": "string" } }
                                } },
                                "request_id": { "type": "string" }
                            },
                            "required": ["code", "message"]
                        }
                    },
                    "required": ["error"]
                }
            },
            "responses": {
                "Error": {
                    "description": "Error envelope",
                    "content": { "application/json": { "schema": { "$ref": "#/components/schemas/Error" } } }
                }
            }
        }
    })
}
