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
