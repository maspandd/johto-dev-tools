//! Endpoints and CRUD resources → OpenAPI 3.1.
//!
//! Standard OpenAPI carries everything a client needs. Workbench-only
//! settings (response rules, delays, disabled endpoints) travel in an
//! `x-workbench` extension so a document exported here imports back
//! without losing them.

use std::collections::{HashMap, HashSet};

use serde_json::{Map, Value, json};
use workbench_domain::{
    AuthSimulation, DataSchema, Endpoint, FieldKind, KeyStrategy, MockResponse, PathTemplate,
    Resource, ResponseBody,
};

use crate::issue::escape;
use crate::schema::{DEFS, infer};

/// The parts of a project that make up its API.
#[derive(Debug, Clone, Copy)]
pub struct ApiDefinition<'a> {
    pub title: &'a str,
    pub description: &'a str,
    /// `info.version`.
    pub version: &'a str,
    pub base_path: Option<&'a str>,
    /// In display order.
    pub endpoints: &'a [Endpoint],
    pub resources: &'a [Resource],
}

/// Builds an OpenAPI 3.1 document.
#[must_use]
pub fn export(def: &ApiDefinition<'_>) -> Value {
    let mut b = Builder::default();
    for ep in def.endpoints {
        b.endpoint(ep);
    }
    for r in def.resources.iter().filter(|r| r.enabled) {
        b.resource(r);
    }

    let mut info = Map::new();
    info.insert("title".into(), json!(def.title));
    if !def.description.trim().is_empty() {
        info.insert("description".into(), json!(def.description));
    }
    info.insert(
        "version".into(),
        json!(if def.version.trim().is_empty() {
            "1.0.0"
        } else {
            def.version
        }),
    );

    let mut doc = Map::new();
    doc.insert("openapi".into(), json!("3.1.0"));
    doc.insert("info".into(), Value::Object(info));
    if let Some(base) = def.base_path.filter(|b| !b.is_empty() && *b != "/") {
        doc.insert("servers".into(), json!([{ "url": base }]));
    }
    if !b.tags.is_empty() {
        doc.insert("tags".into(), Value::Array(b.tags));
    }
    doc.insert("paths".into(), Value::Object(b.paths));
    let mut components = Map::new();
    if !b.schemas.is_empty() {
        components.insert("schemas".into(), Value::Object(b.schemas));
    }
    if !b.security_schemes.is_empty() {
        components.insert("securitySchemes".into(), Value::Object(b.security_schemes));
    }
    if !components.is_empty() {
        doc.insert("components".into(), Value::Object(components));
    }
    Value::Object(doc)
}

#[derive(Default)]
struct Builder {
    paths: Map<String, Value>,
    schemas: Map<String, Value>,
    /// Component name → schema as first seen, to share identical schemas.
    seen: HashMap<String, Value>,
    security_schemes: Map<String, Value>,
    api_key_names: HashMap<String, String>,
    operation_ids: HashSet<String>,
    /// Path shape → the OpenAPI path key used for it.
    shapes: HashMap<String, String>,
    tags: Vec<Value>,
}

impl Builder {
    /// OpenAPI treats `/users/{id}` and `/users/{userId}` as the same path,
    /// so every template with one shape shares the first one's key.
    fn path_key(&mut self, template: &PathTemplate) -> (String, bool) {
        let own = template.to_openapi();
        let key = self
            .shapes
            .entry(template.shape())
            .or_insert_with(|| own.clone())
            .clone();
        let differs = key != own;
        (key, differs)
    }

    fn operation_id(&mut self, name: &str) -> String {
        let mut base = String::new();
        for c in name.chars() {
            if c.is_ascii_alphanumeric() {
                base.push(c.to_ascii_lowercase());
            } else if !base.ends_with('_') {
                base.push('_');
            }
        }
        let base = base.trim_matches('_');
        let base = if base.is_empty() { "operation" } else { base }.to_owned();
        let mut id = base.clone();
        let mut n = 2;
        while !self.operation_ids.insert(id.clone()) {
            id = format!("{base}_{n}");
            n += 1;
        }
        id
    }

    /// OpenAPI allows one operation per method and path. When two
    /// endpoints share one, an enabled endpoint wins over a disabled one,
    /// and otherwise the first wins.
    fn add_operation(&mut self, key: String, method: &str, op: Map<String, Value>) {
        let item = self
            .paths
            .entry(key)
            .or_insert_with(|| Value::Object(Map::new()));
        let Value::Object(item) = item else {
            return;
        };
        let disabled = |op: &Map<String, Value>| {
            op.get("x-workbench")
                .and_then(|w| w.get("enabled"))
                .and_then(Value::as_bool)
                == Some(false)
        };
        match item.get(method) {
            Some(Value::Object(existing)) if !(disabled(existing) && !disabled(&op)) => {}
            _ => {
                item.insert(method.to_owned(), Value::Object(op));
            }
        }
    }

    /// Adds a component schema under `name`, or a variant of it when the
    /// name is taken by a different schema. Returns the name used.
    fn add_schema(&mut self, name: &str, schema: Value) -> String {
        let schema = self.hoist(schema);
        let final_name = self.allocate(name, &schema);
        if !self.schemas.contains_key(&final_name) {
            self.schemas.insert(final_name.clone(), schema);
        }
        final_name
    }

    fn allocate(&mut self, name: &str, schema: &Value) -> String {
        let mut base: String = name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        if base.is_empty() {
            base.push_str("Schema");
        }
        let mut candidate = base.clone();
        let mut n = 2;
        loop {
            match self.seen.get(&candidate) {
                None => {
                    self.seen.insert(candidate.clone(), schema.clone());
                    return candidate;
                }
                Some(existing) if existing == schema => return candidate,
                Some(_) => {
                    candidate = format!("{base}_{n}");
                    n += 1;
                }
            }
        }
    }

    /// Moves a schema's `$defs` into `components/schemas` and rewrites
    /// `#/$defs/X` references to point there.
    fn hoist(&mut self, mut schema: Value) -> Value {
        let Some(Value::Object(defs)) =
            schema.as_object_mut().and_then(|m| m.shift_remove("$defs"))
        else {
            return schema;
        };
        let renames: HashMap<String, String> = defs
            .iter()
            .map(|(name, def)| (name.clone(), self.allocate(name, def)))
            .collect();
        for (name, mut def) in defs {
            let target = &renames[&name];
            if !self.schemas.contains_key(target) {
                rewrite_refs(&mut def, &renames);
                self.schemas.insert(target.clone(), def);
            }
        }
        rewrite_refs(&mut schema, &renames);
        schema
    }

    fn security(&mut self, auth: &AuthSimulation) -> Option<Value> {
        let name = match auth {
            AuthSimulation::None => return None,
            AuthSimulation::Bearer => {
                self.security_schemes
                    .entry("bearerAuth")
                    .or_insert_with(|| json!({ "type": "http", "scheme": "bearer" }));
                "bearerAuth".to_owned()
            }
            AuthSimulation::ApiKey { header } => {
                let count = self.api_key_names.len();
                let name = self
                    .api_key_names
                    .entry(header.to_ascii_lowercase())
                    .or_insert_with(|| {
                        if count == 0 {
                            "apiKey".to_owned()
                        } else {
                            format!("apiKey{}", count + 1)
                        }
                    })
                    .clone();
                self.security_schemes
                    .entry(name.clone())
                    .or_insert_with(|| json!({ "type": "apiKey", "in": "header", "name": header }));
                name
            }
        };
        Some(json!([{ name: [] }]))
    }

    fn endpoint(&mut self, ep: &Endpoint) {
        let (key, differs) = self.path_key(&ep.path);
        let mut op = Map::new();
        op.insert("operationId".into(), json!(self.operation_id(&ep.name)));
        op.insert("summary".into(), json!(ep.name));
        if !ep.description.trim().is_empty() {
            op.insert("description".into(), json!(ep.description));
        }

        let mut params = Vec::new();
        if let Ok(t) = PathTemplate::parse(&key) {
            for name in t.params() {
                params.push(json!({
                    "name": name, "in": "path", "required": true, "schema": { "type": "string" }
                }));
            }
        }
        for (location, specs) in [
            ("query", &ep.request.query),
            ("header", &ep.request.headers),
        ] {
            for p in specs {
                let mut param = Map::new();
                param.insert("name".into(), json!(p.name));
                param.insert("in".into(), json!(location));
                if p.required {
                    param.insert("required".into(), json!(true));
                }
                if !p.description.is_empty() {
                    param.insert("description".into(), json!(p.description));
                }
                param.insert("schema".into(), json!({ "type": "string" }));
                params.push(Value::Object(param));
            }
        }
        if !params.is_empty() {
            op.insert("parameters".into(), Value::Array(params));
        }

        if ep.request.body_schema.is_some() || ep.request.body_example.is_some() {
            let mut media = Map::new();
            let schema = ep.request.body_schema.clone().unwrap_or_else(|| {
                ep.request
                    .body_example
                    .as_ref()
                    .map_or_else(|| json!({}), infer)
            });
            media.insert("schema".into(), self.hoist(schema));
            if let Some(example) = &ep.request.body_example {
                media.insert("example".into(), example.clone());
            }
            op.insert(
                "requestBody".into(),
                json!({ "content": { "application/json": media } }),
            );
        }

        op.insert("responses".into(), self.responses(ep));
        if let Some(security) = self.security(&ep.auth) {
            op.insert("security".into(), security);
        }

        let mut workbench = Map::new();
        if !ep.enabled {
            workbench.insert("enabled".into(), json!(false));
        }
        if ep.response.delay_ms > 0 {
            workbench.insert("delay_ms".into(), json!(ep.response.delay_ms));
        }
        if !ep.rules.is_empty() {
            workbench.insert("default_status".into(), json!(ep.response.status));
            workbench.insert(
                "rules".into(),
                serde_json::to_value(&ep.rules).unwrap_or_default(),
            );
        }
        if differs {
            workbench.insert("path".into(), json!(ep.path.to_string()));
        }
        if !workbench.is_empty() {
            op.insert("x-workbench".into(), Value::Object(workbench));
        }
        self.add_operation(key, &ep.method.as_str().to_ascii_lowercase(), op);
    }

    fn responses(&mut self, ep: &Endpoint) -> Value {
        let mut groups: Vec<(u16, Vec<(&str, &MockResponse)>)> = Vec::new();
        let members = std::iter::once(("default", &ep.response))
            .chain(ep.rules.iter().map(|r| (r.name.as_str(), &r.response)));
        for (label, resp) in members {
            match groups.iter_mut().find(|(s, _)| *s == resp.status) {
                Some((_, list)) => list.push((label, resp)),
                None => groups.push((resp.status, vec![(label, resp)])),
            }
        }

        let mut out = Map::new();
        for (status, members) in groups {
            let mut response = Map::new();
            response.insert("description".into(), json!(reason(status)));

            let mut headers = Map::new();
            for (_, r) in &members {
                for (name, value) in &r.headers {
                    if !name.eq_ignore_ascii_case("content-type") && !headers.contains_key(name) {
                        headers.insert(
                            name.clone(),
                            json!({ "schema": { "type": "string" }, "example": value }),
                        );
                    }
                }
            }
            if !headers.is_empty() {
                response.insert("headers".into(), Value::Object(headers));
            }

            let mut content = Map::new();
            let json_bodies: Vec<(&str, &Value)> = members
                .iter()
                .filter_map(|(label, r)| match &r.body {
                    ResponseBody::Json(v) => Some((*label, v)),
                    _ => None,
                })
                .collect();
            if let Some((_, first)) = json_bodies.first() {
                content.insert(
                    "application/json".into(),
                    with_examples(self.hoist(infer(first)), &json_bodies),
                );
            }
            for (label, r) in &members {
                if let ResponseBody::Text(text) = &r.body {
                    let media = r
                        .headers
                        .iter()
                        .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
                        .map_or("text/plain", |(_, v)| v.as_str())
                        .split(';')
                        .next()
                        .unwrap_or("text/plain")
                        .trim()
                        .to_owned();
                    content.entry(media).or_insert_with(|| {
                        with_examples(json!({ "type": "string" }), &[(*label, &json!(text))])
                    });
                }
            }
            if !content.is_empty() {
                response.insert("content".into(), Value::Object(content));
            }
            out.insert(status.to_string(), Value::Object(response));
        }
        Value::Object(out)
    }

    fn resource(&mut self, r: &Resource) {
        let schema = r
            .body_schema
            .clone()
            .unwrap_or_else(|| record_schema(&r.schema, &r.primary_key, r.key_strategy));
        let record = format!(
            "#/components/schemas/{}",
            escape(&self.add_schema(&r.name, schema))
        );
        let record = json!({ "$ref": record });
        let error = format!(
            "#/components/schemas/{}",
            escape(&self.add_schema("Error", error_schema()))
        );
        let meta = format!(
            "#/components/schemas/{}",
            escape(&self.add_schema("PageMeta", page_meta_schema()))
        );
        let error_response = |description: &str| {
            json!({
                "description": description,
                "content": { "application/json": { "schema": { "$ref": error } } }
            })
        };
        let data = |schema: &Value| json!({ "type": "object", "properties": { "data": schema }, "required": ["data"] });
        let ok = |status_desc: &str, schema: Value| {
            json!({
                "description": status_desc,
                "content": { "application/json": { "schema": schema } }
            })
        };

        self.tags.push(json!({
            "name": r.name,
            "description": format!("CRUD resource `{}`", r.name),
        }));
        let pk: String = {
            let clean: String = r
                .primary_key
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                .collect();
            if clean.starts_with(|c: char| c.is_ascii_digit()) || clean.is_empty() {
                format!("_{clean}")
            } else {
                clean
            }
        };
        let (collection, _) = self.path_key(&r.path);
        let item_template =
            PathTemplate::parse(&format!("{}/:{pk}", r.path)).unwrap_or_else(|_| r.path.clone());
        let (item, _) = self.path_key(&item_template);
        let item_param = json!({
            "name": pk, "in": "path", "required": true, "schema": { "type": "string" }
        });
        let body =
            json!({ "required": true, "content": { "application/json": { "schema": record } } });
        let tag = json!([r.name]);
        let marker = json!({ "resource": r.name });

        let mut ops: Vec<(String, &str, Value)> = Vec::new();
        ops.push((collection.clone(), "get", json!({
            "operationId": self.operation_id(&format!("list_{}", r.name)),
            "summary": format!("List {}", r.name),
            "description": "Filter with `?field=value` (repeat a field for any-of).",
            "tags": tag,
            "parameters": [
                { "name": "_page", "in": "query", "schema": { "type": "integer", "minimum": 1 } },
                { "name": "_limit", "in": "query", "schema": { "type": "integer", "minimum": 1, "maximum": r.pagination.max_limit } },
                { "name": "_q", "in": "query", "description": "Full-text search", "schema": { "type": "string" } },
                { "name": "_sort", "in": "query", "description": "Comma-separated fields; prefix `-` for descending", "schema": { "type": "string" } },
                { "name": "_order", "in": "query", "schema": { "type": "string", "enum": ["asc", "desc"] } }
            ],
            "responses": {
                "200": {
                    "description": "OK",
                    "headers": { "X-Total-Count": { "schema": { "type": "integer" } } },
                    "content": { "application/json": { "schema": {
                        "type": "object",
                        "properties": {
                            "data": { "type": "array", "items": record },
                            "meta": { "$ref": meta }
                        },
                        "required": ["data", "meta"]
                    }}}
                }
            },
            "x-workbench": marker,
        })));
        ops.push((
            collection,
            "post",
            json!({
                "operationId": self.operation_id(&format!("create_{}", r.name)),
                "summary": format!("Create a {} record", r.name),
                "tags": tag,
                "requestBody": body,
                "responses": {
                    "201": ok("Created", data(&record)),
                    "400": error_response("Validation failed"),
                    "409": error_response("Conflict"),
                },
                "x-workbench": marker,
            }),
        ));
        ops.push((
            item.clone(),
            "get",
            json!({
                "operationId": self.operation_id(&format!("get_{}", r.name)),
                "summary": format!("Get a {} record", r.name),
                "tags": tag,
                "parameters": [item_param],
                "responses": {
                    "200": ok("OK", data(&record)),
                    "404": error_response("Not found"),
                },
                "x-workbench": marker,
            }),
        ));
        ops.push((
            item.clone(),
            "put",
            json!({
                "operationId": self.operation_id(&format!("replace_{}", r.name)),
                "summary": format!("Replace a {} record", r.name),
                "tags": tag,
                "parameters": [item_param],
                "requestBody": body,
                "responses": {
                    "200": ok("OK", data(&record)),
                    "400": error_response("Validation failed"),
                    "404": error_response("Not found"),
                },
                "x-workbench": marker,
            }),
        ));
        ops.push((item.clone(), "patch", json!({
            "operationId": self.operation_id(&format!("update_{}", r.name)),
            "summary": format!("Update a {} record", r.name),
            "description": "JSON merge patch (RFC 7386).",
            "tags": tag,
            "parameters": [item_param],
            "requestBody": { "required": true, "content": { "application/merge-patch+json": { "schema": { "type": "object" } } } },
            "responses": {
                "200": ok("OK", data(&record)),
                "400": error_response("Validation failed"),
                "404": error_response("Not found"),
            },
            "x-workbench": marker,
        })));
        ops.push((
            item,
            "delete",
            json!({
                "operationId": self.operation_id(&format!("delete_{}", r.name)),
                "summary": format!("Delete a {} record", r.name),
                "tags": tag,
                "parameters": [item_param],
                "responses": {
                    "204": { "description": "Deleted" },
                    "404": error_response("Not found"),
                },
                "x-workbench": marker,
            }),
        ));
        for (key, method, op) in ops {
            if let Value::Object(op) = op {
                self.add_operation(key, method, op);
            }
        }
    }
}

fn with_examples(schema: Value, bodies: &[(&str, &Value)]) -> Value {
    let mut media = Map::new();
    media.insert("schema".into(), schema);
    if let [(_, only)] = bodies {
        media.insert("example".into(), (*only).clone());
    } else {
        let mut examples = Map::new();
        for (label, body) in bodies {
            let mut key = (*label).to_owned();
            let mut n = 2;
            while examples.contains_key(&key) {
                key = format!("{label} ({n})");
                n += 1;
            }
            examples.insert(key, json!({ "value": body }));
        }
        media.insert("examples".into(), Value::Object(examples));
    }
    Value::Object(media)
}

fn rewrite_refs(value: &mut Value, renames: &HashMap<String, String>) {
    match value {
        Value::Object(map) => {
            for (k, v) in map.iter_mut() {
                if k == "$ref"
                    && let Value::String(reference) = v
                    && let Some(name) = reference.strip_prefix(DEFS)
                {
                    let name = name.replace("~1", "/").replace("~0", "~");
                    if let Some(target) = renames.get(&name) {
                        *reference = format!("#/components/schemas/{}", escape(target));
                    }
                } else {
                    rewrite_refs(v, renames);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|v| rewrite_refs(v, renames)),
        _ => {}
    }
}

/// JSON Schema for records generated from a data schema.
fn record_schema(schema: &DataSchema, primary_key: &str, keys: KeyStrategy) -> Value {
    let mut out = object_schema(&schema.fields);
    if let Some(Value::Object(props)) = out.get_mut("properties")
        && !props.contains_key(primary_key)
    {
        let key = match keys {
            KeyStrategy::Uuid => json!({ "type": "string", "format": "uuid" }),
            KeyStrategy::Increment => json!({ "type": "integer" }),
        };
        props.shift_insert(0, primary_key.to_owned(), key);
    }
    out
}

fn object_schema(fields: &[workbench_domain::FieldSpec]) -> Value {
    let mut props = Map::new();
    for f in fields {
        let mut s = field_schema(&f.kind);
        if f.null_rate > 0.0
            && let Some(Value::String(t)) = s.get("type").cloned()
        {
            s["type"] = json!([t, "null"]);
        }
        props.insert(f.name.clone(), s);
    }
    json!({ "type": "object", "properties": props })
}

fn field_schema(kind: &FieldKind) -> Value {
    match kind {
        FieldKind::Uuid => json!({ "type": "string", "format": "uuid" }),
        FieldKind::Sequence { .. } => json!({ "type": "integer" }),
        FieldKind::Integer { min, max } => {
            json!({ "type": "integer", "minimum": min, "maximum": max })
        }
        FieldKind::Decimal { min, max, .. } => {
            json!({ "type": "number", "minimum": min, "maximum": max })
        }
        FieldKind::Boolean => json!({ "type": "boolean" }),
        FieldKind::Email => json!({ "type": "string", "format": "email" }),
        FieldKind::Date { .. } => json!({ "type": "string", "format": "date" }),
        FieldKind::Timestamp { .. } => json!({ "type": "string", "format": "date-time" }),
        FieldKind::Enum { values } => json!({ "enum": values }),
        FieldKind::Constant { value } => json!({ "const": value }),
        FieldKind::Object { fields } => object_schema(fields),
        FieldKind::Array { items, .. } => json!({ "type": "array", "items": field_schema(items) }),
        FieldKind::Reference { resource, field } => {
            json!({ "description": format!("A `{field}` of a `{resource}` record") })
        }
        FieldKind::FirstName
        | FieldKind::LastName
        | FieldKind::FullName
        | FieldKind::Phone
        | FieldKind::Address
        | FieldKind::City
        | FieldKind::Company
        | FieldKind::Words { .. }
        | FieldKind::Template { .. } => json!({ "type": "string" }),
    }
}

fn error_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "error": {
                "type": "object",
                "properties": {
                    "code": { "type": "string" },
                    "message": { "type": "string" },
                    "details": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "location": { "type": "string" },
                                "message": { "type": "string" }
                            }
                        }
                    },
                    "request_id": { "type": "string" }
                },
                "required": ["code", "message"]
            }
        },
        "required": ["error"]
    })
}

fn page_meta_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "page": { "type": "integer" },
            "limit": { "type": "integer" },
            "total": { "type": "integer" },
            "total_pages": { "type": "integer" }
        }
    })
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        410 => "Gone",
        422 => "Unprocessable Content",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "Response",
    }
}
