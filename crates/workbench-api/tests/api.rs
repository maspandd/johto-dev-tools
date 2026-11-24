//! Management API tests through the real router, with mock servers started
//! via the API and called over real HTTP.

use std::time::{Duration, Instant};

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Method, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;
use workbench_api::{ApiConfig, AppState, SecretKey, router};
use workbench_storage::Store;

struct App {
    router: Router,
    state: AppState,
}

async fn app_with(config: ApiConfig) -> App {
    let store = Store::in_memory().await.unwrap();
    let state = AppState::new(store, SecretKey::generate().unwrap(), config);
    App {
        router: router(state.clone()),
        state,
    }
}

async fn app() -> App {
    app_with(ApiConfig::default()).await
}

struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    body: Value,
}

impl App {
    async fn send(
        &self,
        method: Method,
        uri: &str,
        body: Option<Value>,
        headers: &[(&str, &str)],
    ) -> Reply {
        let mut req = Request::builder().method(method).uri(uri);
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        let req = match body {
            Some(b) => req
                .header("content-type", "application/json")
                .body(Body::from(b.to_string()))
                .unwrap(),
            None => req.body(Body::empty()).unwrap(),
        };
        let res = self.router.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let headers = res.headers().clone();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        let body = serde_json::from_slice(&bytes)
            .unwrap_or(Value::String(String::from_utf8_lossy(&bytes).into_owned()));
        Reply {
            status,
            headers,
            body,
        }
    }

    /// Sends raw bytes; returns status, headers and raw response bytes.
    async fn send_raw(
        &self,
        method: Method,
        uri: &str,
        body: Vec<u8>,
        content_type: &str,
    ) -> (StatusCode, HeaderMap, Vec<u8>) {
        let req = Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", content_type)
            .body(Body::from(body))
            .unwrap();
        let res = self.router.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let headers = res.headers().clone();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        (status, headers, bytes.to_vec())
    }

    async fn get(&self, uri: &str) -> Reply {
        self.send(Method::GET, uri, None, &[]).await
    }
    async fn post(&self, uri: &str, body: Value) -> Reply {
        self.send(Method::POST, uri, Some(body), &[]).await
    }
    async fn patch(&self, uri: &str, body: Value) -> Reply {
        self.send(Method::PATCH, uri, Some(body), &[]).await
    }
    async fn delete(&self, uri: &str) -> Reply {
        self.send(Method::DELETE, uri, None, &[]).await
    }

    /// Creates a workspace and project; returns the project ID.
    async fn project(&self) -> String {
        let ws = self
            .post("/api/v1/workspaces", json!({ "name": "Team" }))
            .await;
        assert_eq!(ws.status, StatusCode::CREATED);
        let p = self
            .post(
                "/api/v1/projects",
                json!({ "workspace_id": ws.body["data"]["id"], "name": "Shop API" }),
            )
            .await;
        assert_eq!(p.status, StatusCode::CREATED, "{:?}", p.body);
        p.body["data"]["id"].as_str().unwrap().to_owned()
    }

    async fn start_server(&self, project: &str, body: Value) -> Value {
        let r = self
            .post(&format!("/api/v1/projects/{project}/mock-servers"), body)
            .await;
        assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
        r.body["data"].clone()
    }
}

fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap()
}

async fn eventually<F, Fut>(mut check: F)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = bool>,
{
    let deadline = Instant::now() + Duration::from_secs(5);
    while !check().await {
        assert!(Instant::now() < deadline, "condition not met in time");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn health_request_ids_and_error_envelope() {
    let app = app().await;
    let health = app.get("/health").await;
    assert_eq!(health.status, StatusCode::OK);
    assert_eq!(health.body["database"], "sqlite");
    assert!(health.headers.contains_key("x-request-id"));

    let missing = app.get("/api/v1/nope").await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
    assert_eq!(missing.body["error"]["code"], "NOT_FOUND");
    assert_eq!(
        missing.body["error"]["request_id"].as_str().unwrap(),
        missing.headers["x-request-id"].to_str().unwrap(),
        "the error names the request"
    );

    let wrong = app
        .send(Method::PUT, "/api/v1/workspaces", Some(json!({})), &[])
        .await;
    assert_eq!(wrong.status, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(wrong.body["error"]["code"], "METHOD_NOT_ALLOWED");
}

#[tokio::test]
async fn workspaces_and_projects() {
    let app = app().await;
    for name in ["A", "B", "C"] {
        app.post("/api/v1/workspaces", json!({ "name": name }))
            .await;
    }
    let page = app.get("/api/v1/workspaces?page=2&limit=2").await;
    assert_eq!(
        page.body["meta"],
        json!({ "page": 2, "limit": 2, "total": 3, "total_pages": 2 })
    );
    assert_eq!(page.body["data"][0]["name"], "C");

    let ws = page.body["data"][0]["id"].as_str().unwrap().to_owned();
    let renamed = app
        .patch(
            &format!("/api/v1/workspaces/{ws}"),
            json!({ "name": "Platform", "archived": true }),
        )
        .await;
    assert_eq!(renamed.body["data"]["name"], "Platform");
    assert!(renamed.body["data"]["archived_at"].is_string());
    assert_eq!(app.get("/api/v1/workspaces").await.body["meta"]["total"], 2);

    let project = app.project().await;
    let url = format!("/api/v1/projects/{project}");
    let patched = app
        .patch(&url, json!({ "description": "Store mock", "base_path": "/api", "id": "ignored", "default_headers": { "X-Mock": "1" } }))
        .await;
    assert_eq!(patched.status, StatusCode::OK, "{:?}", patched.body);
    assert_eq!(
        patched.body["data"]["id"], project,
        "fixed fields cannot change"
    );
    assert_eq!(patched.body["data"]["base_path"], "/api");

    let bad = app.patch(&url, json!({ "base_path": "/api/:v" })).await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(bad.body["error"]["details"][0]["location"], "base_path");

    let copy = app
        .post(
            &format!("{url}/duplicate"),
            json!({ "name": "Shop API v2" }),
        )
        .await;
    assert_eq!(copy.status, StatusCode::CREATED);
    assert_eq!(copy.body["data"]["name"], "Shop API v2");
    let unnamed = app
        .send(Method::POST, &format!("{url}/duplicate"), None, &[])
        .await;
    assert_eq!(unnamed.body["data"]["name"], "Shop API (copy)");

    let archived = app.patch(&url, json!({ "archived": true })).await;
    assert!(archived.body["data"]["archived_at"].is_string());
    let ws_of_project = archived.body["data"]["workspace_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let listed = app
        .get(&format!("/api/v1/projects?workspace_id={ws_of_project}"))
        .await;
    assert_eq!(listed.body["meta"]["total"], 2, "archived project hidden");
    let all = app
        .get(&format!(
            "/api/v1/projects?workspace_id={ws_of_project}&include_archived=true"
        ))
        .await;
    assert_eq!(all.body["meta"]["total"], 3);

    assert_eq!(app.delete(&url).await.status, StatusCode::NO_CONTENT);
    let gone = app.get(&url).await;
    assert_eq!(gone.status, StatusCode::NOT_FOUND);
    assert_eq!(gone.body["error"]["code"], "PROJECT_NOT_FOUND");
}

#[tokio::test]
async fn input_errors_use_the_envelope() {
    let app = app().await;
    let malformed = app
        .send(
            Method::POST,
            "/api/v1/workspaces",
            None,
            &[("content-type", "application/json")],
        )
        .await;
    assert_eq!(malformed.status, StatusCode::BAD_REQUEST);

    let syntax = app
        .router
        .clone()
        .oneshot(
            Request::post("/api/v1/workspaces")
                .header("content-type", "application/json")
                .body(Body::from("{nope"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(syntax.status(), StatusCode::BAD_REQUEST);

    let shape = app.post("/api/v1/workspaces", json!({ "name": 5 })).await;
    assert_eq!(shape.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(shape.body["error"]["code"], "REQUEST_VALIDATION_FAILED");

    let unknown_field = app
        .post("/api/v1/workspaces", json!({ "name": "x", "owner": "y" }))
        .await;
    assert_eq!(unknown_field.status, StatusCode::UNPROCESSABLE_ENTITY);

    let bad_id = app.get("/api/v1/projects/not-a-uuid").await;
    assert_eq!(bad_id.status, StatusCode::BAD_REQUEST);
    assert_eq!(bad_id.body["error"]["code"], "BAD_REQUEST");

    let no_ws = app.get("/api/v1/projects").await;
    assert_eq!(no_ws.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        no_ws.body["error"]["details"][0]["location"],
        "query.workspace_id"
    );

    let empty_name = app.post("/api/v1/workspaces", json!({ "name": " " })).await;
    assert_eq!(empty_name.status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn endpoints_drive_a_running_mock_server() {
    let app = app().await;
    let project = app.project().await;
    app.patch(
        &format!("/api/v1/projects/{project}"),
        json!({ "default_headers": { "X-Project": "shop" } }),
    )
    .await;
    let env = app
        .post(
            &format!("/api/v1/projects/{project}/environments"),
            json!({ "name": "Development", "variables": { "REGION": "id-jkt" }, "default_headers": { "X-Env": "dev" } }),
        )
        .await;
    assert_eq!(env.status, StatusCode::CREATED, "{:?}", env.body);
    let env_id = env.body["data"]["id"].as_str().unwrap().to_owned();

    let ep = app
        .post(
            &format!("/api/v1/projects/{project}/endpoints"),
            json!({
                "name": "Get user", "method": "GET", "path": "/users/:id",
                "response": { "status": 200, "body": { "type": "json", "content": { "id": "{{ path.id }}", "region": "{{ env.REGION }}" } } }
            }),
        )
        .await;
    assert_eq!(ep.status, StatusCode::CREATED, "{:?}", ep.body);
    assert_eq!(ep.body["data"]["position"], 0);
    let ep_id = ep.body["data"]["id"].as_str().unwrap().to_owned();

    let server = app
        .start_server(&project, json!({ "environment_id": env_id, "port": 0 }))
        .await;
    let base = server["url"].as_str().unwrap().to_owned();
    let server_id = server["id"].as_str().unwrap().to_owned();
    assert_eq!(server["endpoints"], 1);

    let res = http().get(format!("{base}/users/7")).send().await.unwrap();
    assert_eq!(res.status(), 200);
    assert_eq!(res.headers()["x-project"], "shop");
    assert_eq!(res.headers()["x-env"], "dev");
    assert_eq!(
        res.json::<Value>().await.unwrap(),
        json!({ "id": "7", "region": "id-jkt" })
    );

    // Disabling an endpoint unregisters its route immediately.
    let disabled = app
        .post(&format!("/api/v1/endpoints/{ep_id}/disable"), json!({}))
        .await;
    assert_eq!(disabled.body["data"]["enabled"], false);
    assert_eq!(
        http()
            .get(format!("{base}/users/7"))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    app.post(&format!("/api/v1/endpoints/{ep_id}/enable"), json!({}))
        .await;
    assert_eq!(
        http()
            .get(format!("{base}/users/7"))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );

    // Patching changes the live response.
    let patched = app
        .patch(
            &format!("/api/v1/endpoints/{ep_id}"),
            json!({ "response": { "status": 202 } }),
        )
        .await;
    assert_eq!(patched.status, StatusCode::OK, "{:?}", patched.body);
    assert_eq!(
        http()
            .get(format!("{base}/users/7"))
            .send()
            .await
            .unwrap()
            .status(),
        202
    );

    // A conflicting endpoint is saved, but the server keeps the old routes and reports why.
    let conflict = app
        .post(
            &format!("/api/v1/projects/{project}/endpoints"),
            json!({ "name": "Dup", "method": "GET", "path": "/users/:userId" }),
        )
        .await;
    assert_eq!(conflict.status, StatusCode::CREATED);
    let info = app.get(&format!("/api/v1/mock-servers/{server_id}")).await;
    assert!(
        info.body["data"]["last_reload_error"]
            .as_str()
            .unwrap()
            .contains("route conflict")
    );
    assert_eq!(
        http()
            .get(format!("{base}/users/7"))
            .send()
            .await
            .unwrap()
            .status(),
        202
    );
    let dup_id = conflict.body["data"]["id"].as_str().unwrap().to_owned();
    app.delete(&format!("/api/v1/endpoints/{dup_id}")).await;
    let info = app.get(&format!("/api/v1/mock-servers/{server_id}")).await;
    assert!(info.body["data"]["last_reload_error"].is_null());

    // Duplicates start disabled.
    let copy = app
        .post(&format!("/api/v1/endpoints/{ep_id}/duplicate"), json!({}))
        .await;
    assert_eq!(copy.body["data"]["enabled"], false);

    // Stop releases the port.
    let port = server["port"].as_u64().unwrap();
    assert_eq!(
        app.delete(&format!("/api/v1/mock-servers/{server_id}"))
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    assert!(http().get(format!("{base}/users/7")).send().await.is_err());
    assert_eq!(
        app.get(&format!("/api/v1/mock-servers/{server_id}"))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    tokio::net::TcpListener::bind(("127.0.0.1", u16::try_from(port).unwrap()))
        .await
        .expect("port released");
}

#[tokio::test]
async fn port_conflicts_and_exposure_are_reported() {
    let app = app().await;
    let project = app.project().await;
    let blocker = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = blocker.local_addr().unwrap().port();
    let busy = app
        .post(
            &format!("/api/v1/projects/{project}/mock-servers"),
            json!({ "port": port }),
        )
        .await;
    assert_eq!(busy.status, StatusCode::CONFLICT);
    assert_eq!(busy.body["error"]["code"], "CONFLICT");

    let exposed = app
        .post(
            &format!("/api/v1/projects/{project}/mock-servers"),
            json!({ "host": "0.0.0.0", "port": 0 }),
        )
        .await;
    assert_eq!(exposed.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(app.state.runtime.list(None).await.is_empty());
}

#[tokio::test]
async fn resources_persist_through_server_restarts() {
    let app = app().await;
    let project = app.project().await;
    let res = app
        .post(
            &format!("/api/v1/projects/{project}/resources"),
            json!({
                "name": "users", "path": "/users", "key_strategy": "increment",
                "schema": { "fields": [{ "name": "name", "type": "full_name" }] },
                "seed": { "count": 12, "seed": 5 }
            }),
        )
        .await;
    assert_eq!(res.status, StatusCode::CREATED, "{:?}", res.body);
    assert_eq!(res.body["data"]["data_initialized"], false);
    let rid = res.body["data"]["id"].as_str().unwrap().to_owned();

    let server = app.start_server(&project, json!({ "port": 0 })).await;
    let base = server["url"].as_str().unwrap().to_owned();
    let c = http();
    let created: Value = c
        .post(format!("{base}/users"))
        .json(&json!({ "name": "Ayu" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(created["data"]["id"], 13);
    assert_eq!(
        c.delete(format!("{base}/users/1"))
            .send()
            .await
            .unwrap()
            .status(),
        204
    );

    let live = app
        .get(&format!("/api/v1/resources/{rid}/records?limit=100"))
        .await;
    assert_eq!(live.body["meta"]["total"], 12);

    // A second server with resources for the same project would split the data.
    let second = app
        .post(
            &format!("/api/v1/projects/{project}/mock-servers"),
            json!({ "port": 0 }),
        )
        .await;
    assert_eq!(second.status, StatusCode::CONFLICT);

    app.delete(&format!(
        "/api/v1/mock-servers/{}",
        server["id"].as_str().unwrap()
    ))
    .await;
    let stored = app
        .get(&format!("/api/v1/resources/{rid}/records?limit=100"))
        .await;
    assert_eq!(stored.body["meta"]["total"], 12, "read from the database");
    assert_eq!(
        app.get(&format!("/api/v1/resources/{rid}")).await.body["data"]["data_initialized"],
        true
    );

    let server = app.start_server(&project, json!({ "port": 0 })).await;
    let base = server["url"].as_str().unwrap().to_owned();
    assert_eq!(
        c.get(format!("{base}/users/13"))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    assert_eq!(
        c.get(format!("{base}/users/1"))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );

    // Reset restores the seed on the running server.
    assert_eq!(
        app.post(&format!("/api/v1/resources/{rid}/reset"), json!({}))
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        c.get(format!("{base}/users/1"))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    assert_eq!(
        c.get(format!("{base}/users/13"))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );

    // Changing the definition reloads the server.
    app.patch(
        &format!("/api/v1/resources/{rid}"),
        json!({ "pagination": { "default_limit": 5, "max_limit": 10 } }),
    )
    .await;
    let page: Value = c
        .get(format!("{base}/users"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(page["meta"]["limit"], 5);
    app.state.runtime.stop_all().await;
}

#[tokio::test]
async fn request_logs_are_persisted_and_filterable() {
    let app = app().await;
    let project = app.project().await;
    app.post(
        &format!("/api/v1/projects/{project}/endpoints"),
        json!({ "name": "Ping", "method": "GET", "path": "/ping" }),
    )
    .await;
    let server = app.start_server(&project, json!({ "port": 0 })).await;
    let base = server["url"].as_str().unwrap().to_owned();
    let c = http();
    c.get(format!("{base}/ping"))
        .header("authorization", "Bearer secret-token")
        .send()
        .await
        .unwrap();
    c.get(format!("{base}/missing")).send().await.unwrap();

    let logs_url = format!("/api/v1/projects/{project}/request-logs");
    eventually(|| async {
        app.get(&logs_url).await.body["data"]
            .as_array()
            .unwrap()
            .len()
            == 2
    })
    .await;

    let logs = app.get(&logs_url).await.body["data"].clone();
    assert_eq!(logs[0]["status"], 404, "newest first");
    let auth = logs[1]["request_headers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h[0] == "authorization")
        .unwrap();
    assert_eq!(auth[1], "[REDACTED]");

    assert_eq!(
        app.get(&format!("{logs_url}?status=200")).await.body["data"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let by_server = app
        .get(&format!(
            "/api/v1/mock-servers/{}/logs",
            server["id"].as_str().unwrap()
        ))
        .await;
    assert_eq!(by_server.body["data"].as_array().unwrap().len(), 2);
    assert_eq!(
        app.get(&format!("{logs_url}?nope=1")).await.status,
        StatusCode::UNPROCESSABLE_ENTITY
    );

    let cleared = app.delete(&logs_url).await;
    assert_eq!(cleared.body["data"]["deleted"], 2);
    app.state.runtime.stop_all().await;
}

#[tokio::test]
async fn secrets_are_write_only_and_encrypted() {
    let app = app().await;
    let project = app.project().await;
    let env = app
        .post(
            &format!("/api/v1/projects/{project}/environments"),
            json!({ "name": "Staging" }),
        )
        .await;
    let env_id = env.body["data"]["id"].as_str().unwrap().to_owned();
    let url = format!("/api/v1/environments/{env_id}/secrets");

    let put = app
        .send(
            Method::PUT,
            &format!("{url}/API_TOKEN"),
            Some(json!({ "value": "tok-123" })),
            &[],
        )
        .await;
    assert_eq!(put.status, StatusCode::NO_CONTENT);
    let listed = app.get(&url).await;
    assert_eq!(listed.body["data"][0]["name"], "API_TOKEN");
    assert!(
        listed.body["data"][0].get("value").is_none(),
        "values are never returned"
    );
    assert!(!listed.body.to_string().contains("tok-123"));
    assert!(
        !app.get(&format!("/api/v1/environments/{env_id}"))
            .await
            .body
            .to_string()
            .contains("tok-123")
    );

    let env_uuid: workbench_core::EnvironmentId = env_id.parse().unwrap();
    let sealed = app
        .state
        .store
        .get_secret(env_uuid, "API_TOKEN")
        .await
        .unwrap();
    assert!(
        !sealed.ciphertext.windows(7).any(|w| w == b"tok-123"),
        "stored encrypted"
    );
    assert_eq!(
        app.state
            .secret_key
            .decrypt(env_uuid, "API_TOKEN", &sealed)
            .as_deref(),
        Some("tok-123")
    );

    let bad = app
        .send(
            Method::PUT,
            &format!("{url}/1bad"),
            Some(json!({ "value": "x" })),
            &[],
        )
        .await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        app.delete(&format!("{url}/API_TOKEN")).await.status,
        StatusCode::NO_CONTENT
    );
    let gone = app.delete(&format!("{url}/API_TOKEN")).await;
    assert_eq!(gone.body["error"]["code"], "SECRET_NOT_FOUND");
}

#[tokio::test]
async fn datasets_preview_and_export() {
    let app = app().await;
    let project = app.project().await;
    let schema = json!({ "fields": [
        { "name": "id", "type": "sequence" },
        { "name": "name", "type": "full_name" },
        { "name": "city", "type": "city" }
    ] });
    let unsaved = app
        .post(
            &format!("/api/v1/projects/{project}/datasets/preview"),
            json!({ "schema": schema, "count": 500, "seed": 3, "locale": "id" }),
        )
        .await;
    assert_eq!(unsaved.status, StatusCode::OK, "{:?}", unsaved.body);
    assert_eq!(unsaved.body["data"].as_array().unwrap().len(), 20);

    let ds = app
        .post(&format!("/api/v1/projects/{project}/datasets"), json!({ "name": "Customers", "schema": schema, "count": 50, "seed": 3, "locale": "id" }))
        .await;
    assert_eq!(ds.status, StatusCode::CREATED, "{:?}", ds.body);
    let id = ds.body["data"]["id"].as_str().unwrap().to_owned();

    let preview = app.get(&format!("/api/v1/datasets/{id}/preview")).await;
    assert_eq!(
        preview.body["data"],
        json!(unsaved.body["data"]),
        "same seed, same data"
    );

    let csv = app
        .get(&format!("/api/v1/datasets/{id}/export?format=csv"))
        .await;
    assert_eq!(csv.headers["content-type"], "text/csv; charset=utf-8");
    assert!(
        csv.headers["content-disposition"]
            .to_str()
            .unwrap()
            .contains("Customers.csv")
    );
    let text = csv.body.as_str().unwrap();
    assert!(text.starts_with("id,name,city\n"));
    assert_eq!(text.lines().count(), 51);

    let json_export = app.get(&format!("/api/v1/datasets/{id}/export")).await;
    assert_eq!(json_export.body.as_array().unwrap().len(), 50);

    let bad = app
        .post(&format!("/api/v1/projects/{project}/datasets/preview"), json!({ "schema": { "fields": [{ "name": "n", "type": "integer", "min": 5, "max": 1 }] } }))
        .await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn token_protects_the_api() {
    let app = app_with(ApiConfig {
        api_token: Some("let-me-in".into()),
        ..ApiConfig::default()
    })
    .await;
    assert_eq!(
        app.get("/health").await.status,
        StatusCode::OK,
        "health stays open"
    );
    let denied = app.get("/api/v1/workspaces").await;
    assert_eq!(denied.status, StatusCode::UNAUTHORIZED);
    assert_eq!(denied.body["error"]["code"], "UNAUTHORIZED");
    let wrong = app
        .send(
            Method::GET,
            "/api/v1/workspaces",
            None,
            &[("authorization", "Bearer nope")],
        )
        .await;
    assert_eq!(wrong.status, StatusCode::UNAUTHORIZED);
    let ok = app
        .send(
            Method::GET,
            "/api/v1/workspaces",
            None,
            &[("authorization", "Bearer let-me-in")],
        )
        .await;
    assert_eq!(ok.status, StatusCode::OK);
}

#[tokio::test]
async fn cors_allows_only_listed_origins() {
    let app = app().await;
    let preflight = |origin: &'static str| {
        Request::builder()
            .method(Method::OPTIONS)
            .uri("/api/v1/workspaces")
            .header("origin", origin)
            .header("access-control-request-method", "POST")
            .body(Body::empty())
            .unwrap()
    };
    let ok = app
        .router
        .clone()
        .oneshot(preflight("http://localhost:5173"))
        .await
        .unwrap();
    assert_eq!(
        ok.headers()["access-control-allow-origin"],
        "http://localhost:5173"
    );
    let other = app
        .router
        .clone()
        .oneshot(preflight("https://evil.example"))
        .await
        .unwrap();
    assert!(!other.headers().contains_key("access-control-allow-origin"));
}

const PETS: &str = r#"
openapi: 3.0.3
info: { title: Pets, version: 1.0.0 }
servers: [{ url: "https://api.example.test/v1" }]
paths:
  /pets:
    get:
      operationId: listPets
      responses:
        200:
          description: OK
          content:
            application/json:
              schema:
                type: array
                items: { $ref: '#/components/schemas/Pet' }
    post:
      operationId: createPet
      requestBody:
        content:
          application/json:
            schema: { $ref: '#/components/schemas/Pet' }
      responses:
        '201': { description: Created, content: { application/json: { example: { id: 1 } } } }
  /pets/{petId}:
    get:
      operationId: getPet
      responses:
        '200':
          description: OK
          content:
            application/json:
              example: { id: "{{ path.petId }}", name: Rex }
    options:
      responses: { '200': { description: OK } }
components:
  schemas:
    Pet:
      type: object
      required: [name]
      properties:
        id: { type: integer }
        name: { type: string }
"#;

#[tokio::test]
async fn openapi_validate_import_and_export() {
    let app = app().await;
    let project = app.project().await;

    let (status, _, body) = app
        .send_raw(
            Method::POST,
            "/api/v1/openapi/validate",
            PETS.into(),
            "application/yaml",
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let v: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["data"]["valid"], true, "{v}");
    assert_eq!(v["data"]["operations"], 4);
    assert_eq!(v["data"]["importable"], 3);
    let (_, _, body) = app
        .send_raw(
            Method::POST,
            "/api/v1/openapi/validate",
            b"swagger: '2.0'".to_vec(),
            "text/plain",
        )
        .await;
    let v: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["data"]["valid"], false);
    assert!(
        v["data"]["issues"][0]["message"]
            .as_str()
            .unwrap()
            .contains("Swagger")
    );

    // An existing endpoint on the same route.
    let existing = app
        .post(
            &format!("/api/v1/projects/{project}/endpoints"),
            json!({ "name": "Old list", "method": "GET", "path": "/pets" }),
        )
        .await;
    assert_eq!(existing.status, StatusCode::CREATED);

    let uri = format!("/api/v1/projects/{project}/openapi/import");
    let preview = app
        .post(&uri, json!({ "document": PETS, "dry_run": true }))
        .await;
    assert_eq!(preview.status, StatusCode::OK, "{:?}", preview.body);
    let ops = preview.body["data"]["operations"].as_array().unwrap();
    assert_eq!(ops.len(), 4);
    assert_eq!(ops[0]["key"], "GET /pets");
    assert_eq!(ops[0]["conflicts_with_project"], true);
    assert_eq!(ops[3]["importable"], false);
    let listed = app
        .get(&format!("/api/v1/projects/{project}/endpoints"))
        .await;
    assert_eq!(listed.body["meta"]["total"], 1, "a dry run saves nothing");

    let bad = app
        .post(
            &uri,
            json!({ "document": PETS, "operations": ["OPTIONS /pets/{petId}"] }),
        )
        .await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        bad.body["error"]["details"][0]["message"]
            .as_str()
            .unwrap()
            .contains("cannot be imported")
    );

    let empty_doc =
        json!({ "openapi": "3.1.0", "info": { "title": "x", "version": "1" }, "paths": {} });
    let empty = app.post(&uri, json!({ "document": empty_doc })).await;
    assert_eq!(empty.status, StatusCode::UNPROCESSABLE_ENTITY);

    let imported = app
        .post(
            &uri,
            json!({ "document": PETS, "operations": ["listPets", "getPet", "POST /pets"] }),
        )
        .await;
    assert_eq!(imported.status, StatusCode::CREATED, "{:?}", imported.body);
    let d = &imported.body["data"];
    assert_eq!(d["imported"].as_array().unwrap().len(), 3);
    assert_eq!(d["disabled_due_to_conflicts"], json!(["listPets"]));
    assert_eq!(d["base_path_set"], "/v1");
    let p = app.get(&format!("/api/v1/projects/{project}")).await;
    assert_eq!(p.body["data"]["base_path"], "/v1");

    // The imported endpoints serve requests.
    let server = app.start_server(&project, json!({ "port": 0 })).await;
    let base = server["url"].as_str().unwrap().to_owned();
    assert!(base.ends_with("/v1"));
    let c = http();
    let pet: Value = c
        .get(format!("{base}/pets/7"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(pet, json!({ "id": "7", "name": "Rex" }));
    let invalid = c
        .post(format!("{base}/pets"))
        .json(&json!({ "id": 1 }))
        .send()
        .await
        .unwrap();
    assert_eq!(invalid.status(), 422, "imported schema validates bodies");
    let ok = c
        .post(format!("{base}/pets"))
        .json(&json!({ "name": "Tom" }))
        .send()
        .await
        .unwrap();
    assert_eq!(ok.status(), 201);

    // Export as YAML and JSON.
    let (status, headers, body) = app
        .send_raw(
            Method::GET,
            &format!("/api/v1/projects/{project}/openapi/export"),
            vec![],
            "text/plain",
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["content-type"], "application/yaml");
    assert!(
        headers["content-disposition"]
            .to_str()
            .unwrap()
            .contains("shop-api-openapi.yaml")
    );
    let exported = openapi_engine::parse_document(std::str::from_utf8(&body).unwrap()).unwrap();
    assert_eq!(exported["servers"][0]["url"], "/v1");
    let again = openapi_engine::analyze(&exported).unwrap();
    assert!(!again.has_errors());
    // The disabled duplicate of `GET /pets` gives way to the enabled one.
    assert_eq!(again.operations.len(), 3);
    assert_eq!(exported["paths"]["/pets"]["get"]["summary"], "Old list");
    let json_export = app
        .get(&format!(
            "/api/v1/projects/{project}/openapi/export?format=json&version=2.0.0"
        ))
        .await;
    assert_eq!(json_export.body["info"]["version"], "2.0.0");

    let swagger = app
        .post(&uri, json!({ "document": "swagger: '2.0'" }))
        .await;
    assert_eq!(swagger.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        swagger.body["error"]["details"][0]["location"],
        "document/swagger"
    );
}

#[tokio::test]
async fn curl_commands_become_endpoint_drafts() {
    let app = app().await;
    let project = app.project().await;
    app.patch(
        &format!("/api/v1/projects/{project}"),
        json!({ "base_path": "/api" }),
    )
    .await;
    let command = r#"curl -X POST https://h.test/api/users/42/notes -H 'Authorization: Bearer s3cr3t' -d '{"text":"hi","password":"pw"}'"#;
    let r = app
        .post(
            "/api/v1/curl/parse",
            json!({ "command": command, "project_id": project }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let d = &r.body["data"];
    assert_eq!(d["request"]["method"], "POST");
    assert_eq!(d["request"]["headers"][0]["value"], "Bearer [REDACTED]");
    assert_eq!(d["request"]["body"]["content"]["password"], "[REDACTED]");
    assert!(!r.body.to_string().contains("s3cr3t"));
    assert_eq!(d["endpoint"]["path"], "/users/:id/notes");
    assert_eq!(d["endpoint"]["auth"]["type"], "bearer");

    // The draft can be saved as-is.
    let saved = app
        .post(
            &format!("/api/v1/projects/{project}/endpoints"),
            d["endpoint"].clone(),
        )
        .await;
    assert_eq!(saved.status, StatusCode::CREATED, "{:?}", saved.body);

    let bad = app
        .post("/api/v1/curl/parse", json!({ "command": "rm -rf /" }))
        .await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(bad.body["error"]["details"][0]["location"], "body.command");
}

#[tokio::test]
async fn project_archives_round_trip() {
    let app = app().await;
    let project = app.project().await;
    let ws = app.get(&format!("/api/v1/projects/{project}")).await.body["data"]["workspace_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let env = app
        .post(
            &format!("/api/v1/projects/{project}/environments"),
            json!({ "name": "Development", "variables": { "GREETING": "hi" } }),
        )
        .await;
    let env_id = env.body["data"]["id"].as_str().unwrap().to_owned();
    let put = app
        .send(
            Method::PUT,
            &format!("/api/v1/environments/{env_id}/secrets/API_TOKEN"),
            Some(json!({ "value": "very-secret-value" })),
            &[],
        )
        .await;
    assert_eq!(put.status, StatusCode::NO_CONTENT);
    app.post(
        &format!("/api/v1/projects/{project}/endpoints"),
        json!({ "name": "Hello", "method": "GET", "path": "/hello",
                "response": { "body": { "type": "json", "content": { "hi": "{{ env.GREETING }}" } } } }),
    )
    .await;
    app.post(
        &format!("/api/v1/projects/{project}/resources"),
        json!({ "name": "users", "path": "/users", "key_strategy": "increment",
                "schema": { "fields": [{ "name": "name", "type": "full_name" }] },
                "seed": { "count": 3, "seed": 1 } }),
    )
    .await;
    let server = app.start_server(&project, json!({ "port": 0 })).await;
    let base = server["url"].as_str().unwrap().to_owned();
    let created = http()
        .post(format!("{base}/users"))
        .json(&json!({ "name": "Imported Ayu" }))
        .send()
        .await
        .unwrap();
    assert_eq!(created.status(), 201);

    let (status, headers, zip) = app
        .send_raw(
            Method::GET,
            &format!("/api/v1/projects/{project}/export?records=true"),
            vec![],
            "text/plain",
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["content-type"], "application/zip");
    assert!(
        !zip.windows(17).any(|w| w == b"very-secret-value"),
        "secret values are never exported"
    );

    let preview_uri = format!("/api/v1/projects/import?workspace_id={ws}&preview=true");
    let (status, _, body) = app
        .send_raw(Method::POST, &preview_uri, zip.clone(), "application/zip")
        .await;
    assert_eq!(status, StatusCode::OK);
    let preview: Value = serde_json::from_slice(&body).unwrap();
    let p = &preview["data"];
    assert_eq!(p["manifest"]["contents"]["records"], 4);
    assert_eq!(p["endpoints"][0]["name"], "Hello");
    assert_eq!(p["resources"][0]["records"], 4);
    assert_eq!(
        p["secrets_to_set"],
        json!([{ "environment": "Development", "names": ["API_TOKEN"] }])
    );
    let projects = app
        .get(&format!("/api/v1/projects?workspace_id={ws}"))
        .await;
    assert_eq!(projects.body["meta"]["total"], 1, "a preview saves nothing");

    let (status, _, body) = app
        .send_raw(
            Method::POST,
            &format!("/api/v1/projects/import?workspace_id={ws}&name=Restored"),
            zip,
            "application/zip",
        )
        .await;
    let imported: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(status, StatusCode::CREATED, "{imported}");
    let new_id = imported["data"]["project"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_ne!(new_id, project);
    assert_eq!(imported["data"]["project"]["name"], "Restored");

    let eps = app
        .get(&format!("/api/v1/projects/{new_id}/endpoints"))
        .await;
    assert_eq!(eps.body["data"][0]["name"], "Hello");
    let envs = app
        .get(&format!("/api/v1/projects/{new_id}/environments"))
        .await;
    let new_env = envs.body["data"][0]["id"].as_str().unwrap().to_owned();
    assert_eq!(envs.body["data"][0]["variables"]["GREETING"], "hi");
    let secrets = app
        .get(&format!("/api/v1/environments/{new_env}/secrets"))
        .await;
    assert_eq!(secrets.body["data"], json!([]));
    let res = app
        .get(&format!("/api/v1/projects/{new_id}/resources"))
        .await;
    let rid = res.body["data"][0]["id"].as_str().unwrap().to_owned();
    let records = app.get(&format!("/api/v1/resources/{rid}/records")).await;
    assert_eq!(records.body["meta"]["total"], 4);
    assert_eq!(records.body["data"][3]["name"], "Imported Ayu");

    let (status, _, body) = app
        .send_raw(
            Method::POST,
            &format!("/api/v1/projects/import?workspace_id={ws}"),
            b"definitely not a zip".to_vec(),
            "application/zip",
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let err: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(err["error"]["details"][0]["location"], "archive");
    let (status, _, _) = app
        .send_raw(
            Method::POST,
            "/api/v1/projects/import",
            vec![1],
            "application/zip",
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn management_api_describes_itself() {
    let app = app().await;
    let doc = app.get("/api/v1/openapi.json").await;
    assert_eq!(doc.status, StatusCode::OK);
    let analysis = openapi_engine::analyze(&doc.body).unwrap();
    assert!(
        !analysis.has_errors(),
        "{:#?}",
        analysis.all_issues().collect::<Vec<_>>()
    );

    // Every documented route exists in the router.
    let id = "00000000-0000-4000-8000-000000000000";
    let mut checked = 0;
    for (path, item) in doc.body["paths"].as_object().unwrap() {
        for method in item.as_object().unwrap().keys() {
            let uri = format!("/api/v1{}", path.replace("{id}", id).replace("{name}", "X"));
            let m = Method::from_bytes(method.to_uppercase().as_bytes()).unwrap();
            let r = app.send(m, &uri, Some(json!({})), &[]).await;
            assert_ne!(r.status, StatusCode::METHOD_NOT_ALLOWED, "{method} {path}");
            assert_ne!(
                r.body["error"]["code"], "ROUTE_NOT_FOUND",
                "{method} {path}"
            );
            checked += 1;
        }
    }
    assert!(checked >= 59, "{checked}");
}

#[tokio::test]
async fn put_replaces_definitions_and_keeps_nulls() {
    let app = app().await;
    let project = app.project().await;
    let ep = app
        .post(
            &format!("/api/v1/projects/{project}/endpoints"),
            json!({ "name": "Item", "method": "GET", "path": "/item",
                    "response": { "body": { "type": "json", "content": { "a": 1, "b": 2 } } } }),
        )
        .await;
    let id = ep.body["data"]["id"].as_str().unwrap().to_owned();
    let mut def = ep.body["data"].clone();
    def["response"]["body"]["content"] = json!({ "deleted_at": null });
    def["id"] = json!("ignored");
    let put = app
        .send(
            Method::PUT,
            &format!("/api/v1/endpoints/{id}"),
            Some(def),
            &[],
        )
        .await;
    assert_eq!(put.status, StatusCode::OK, "{:?}", put.body);
    assert_eq!(put.body["data"]["id"], id.as_str());
    assert_eq!(
        put.body["data"]["response"]["body"]["content"],
        json!({ "deleted_at": null })
    );
    let bad = app
        .send(
            Method::PUT,
            &format!("/api/v1/endpoints/{id}"),
            Some(json!({ "name": "x" })),
            &[],
        )
        .await;
    assert_eq!(
        bad.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "method and path are required"
    );
}
