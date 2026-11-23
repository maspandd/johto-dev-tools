//! End-to-end tests over real TCP with an external HTTP client.
//! Numbered scenarios refer to the spec's "Critical acceptance scenarios" (§11).

use std::net::{IpAddr, Ipv4Addr};
use std::time::{Duration, Instant};

use mock_runtime::{
    HistoryFilter, HistoryRecord, HistoryStore, MockRuntimeManager, MockServerConfig,
    RunningServer, StartError,
};
use reqwest::{Client, StatusCode};
use serde_json::{Value, json};
use workbench_core::{BindAddress, BindError};
use workbench_domain::{Endpoint, Method, MockResponse, PathTemplate};

fn client() -> Client {
    Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap()
}

fn endpoint(method: Method, path: &str, response: MockResponse) -> Endpoint {
    let mut e = Endpoint::new(path, method, PathTemplate::parse(path).unwrap());
    e.response = response;
    e
}

async fn start(endpoints: &[Endpoint]) -> RunningServer {
    RunningServer::start(MockServerConfig::loopback(0), endpoints)
        .await
        .unwrap()
}

fn url(server: &RunningServer, path: &str) -> String {
    format!("http://{}{path}", server.local_addr())
}

/// History is written asynchronously; wait until `n` records exist.
async fn wait_for_history(store: &HistoryStore, n: usize) -> Vec<HistoryRecord> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let records = store.list(&HistoryFilter::default());
        if records.len() >= n || Instant::now() > deadline {
            return records;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test]
async fn scenario_2_get_endpoint_returns_configured_json() {
    let mut resp = MockResponse::json(
        200,
        json!({ "data": [{ "id": "b7e7f6c4-01e5-4d65-95d7-8f54f4f2c201", "name": "Alex Morgan", "email": "alex@example.test" }] }),
    );
    resp.headers
        .insert("X-Mock-Source".into(), "workbench".into());
    let server = start(&[endpoint(Method::Get, "/api/users", resp)]).await;

    let res = client()
        .get(url(&server, "/api/users"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()["x-mock-source"], "workbench");
    assert_eq!(res.headers()["content-type"], "application/json");
    let body: Value = res.json().await.unwrap();
    assert_eq!(body["data"][0]["name"], "Alex Morgan");

    server.stop().await;
}

#[tokio::test]
async fn scenario_3_dynamic_route_matches_path_parameter() {
    let server = start(&[
        endpoint(
            Method::Get,
            "/api/users/:id",
            MockResponse::json(200, json!({ "id": "{{ path.id }}" })),
        ),
        endpoint(
            Method::Get,
            "/api/users/me",
            MockResponse::json(200, json!({ "id": "me", "static": true })),
        ),
    ])
    .await;
    let c = client();

    let body: Value = c
        .get(url(&server, "/api/users/42"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(body, json!({ "id": "42" }));

    let body: Value = c
        .get(url(&server, "/api/users/a%20b"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        body,
        json!({ "id": "a b" }),
        "path params are percent-decoded"
    );

    let body: Value = c
        .get(url(&server, "/api/users/me"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(body["static"], true, "static route wins over parameter");

    let res = c
        .get(url(&server, "/api/users/42/extra"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    server.stop().await;
}

#[tokio::test]
async fn scenario_4_post_endpoint_validates_request_body() {
    let mut create = endpoint(
        Method::Post,
        "/api/users",
        MockResponse::json(201, json!({ "name": "{{ body.name }}" })),
    );
    create.request.body_schema = Some(json!({
        "type": "object",
        "required": ["name", "email"],
        "properties": { "name": { "type": "string", "minLength": 1 }, "email": { "type": "string" } }
    }));
    let server = start(&[create]).await;
    let c = client();

    let ok = c
        .post(url(&server, "/api/users"))
        .json(&json!({ "name": "Alex", "email": "alex@example.test" }))
        .send()
        .await
        .unwrap();
    assert_eq!(ok.status(), StatusCode::CREATED);
    assert_eq!(ok.json::<Value>().await.unwrap(), json!({ "name": "Alex" }));

    let bad = c
        .post(url(&server, "/api/users"))
        .json(&json!({ "name": "" }))
        .send()
        .await
        .unwrap();
    assert_eq!(bad.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let err: Value = bad.json().await.unwrap();
    assert_eq!(err["error"]["code"], "REQUEST_VALIDATION_FAILED");
    assert!(err["error"]["details"].as_array().unwrap().len() >= 2);
    assert!(err["error"]["request_id"].is_string());

    let malformed = c
        .post(url(&server, "/api/users"))
        .header("content-type", "application/json")
        .body("{oops")
        .send()
        .await
        .unwrap();
    assert_eq!(malformed.status(), StatusCode::BAD_REQUEST);

    server.stop().await;
}

#[tokio::test]
async fn scenario_9_two_mock_environments_run_without_collisions() {
    let manager = MockRuntimeManager::new();
    let dev = manager
        .start(
            MockServerConfig::loopback(0),
            &[endpoint(
                Method::Get,
                "/api/env",
                MockResponse::json(200, json!({ "env": "development" })),
            )],
        )
        .await
        .unwrap();
    let test = manager
        .start(
            MockServerConfig::loopback(0),
            &[endpoint(
                Method::Get,
                "/api/env",
                MockResponse::json(200, json!({ "env": "testing" })),
            )],
        )
        .await
        .unwrap();
    assert_ne!(dev.addr, test.addr);
    assert_eq!(manager.status().await.len(), 2);

    let c = client();
    let a: Value = c
        .get(format!("http://{}/api/env", dev.addr))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let b: Value = c
        .get(format!("http://{}/api/env", test.addr))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(a["env"], "development");
    assert_eq!(b["env"], "testing");

    // History is isolated per server.
    let dev_history = manager.history(dev.id).await.unwrap();
    let test_history = manager.history(test.id).await.unwrap();
    assert_eq!(wait_for_history(&dev_history, 1).await.len(), 1);
    assert_eq!(wait_for_history(&test_history, 1).await.len(), 1);

    assert!(manager.stop(dev.id).await);
    assert!(
        !manager.stop(dev.id).await,
        "stopping twice reports unknown id"
    );
    assert_eq!(manager.status().await.len(), 1);
    manager.stop_all().await;
    assert!(manager.status().await.is_empty());
}

#[tokio::test]
async fn scenario_10_stopping_releases_the_listener() {
    let server = start(&[endpoint(Method::Get, "/ping", MockResponse::default())]).await;
    let addr = server.local_addr();
    let c = client();
    assert_eq!(
        c.get(format!("http://{addr}/ping"))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );

    server.stop().await;

    assert!(
        c.get(format!("http://{addr}/ping")).send().await.is_err(),
        "no longer accepting connections"
    );
    // The exact port can be bound again.
    let again = RunningServer::start(MockServerConfig::loopback(addr.port()), &[])
        .await
        .expect("port is free after stop");
    again.stop().await;
}

#[tokio::test]
async fn stop_all_shuts_down_every_listener() {
    let manager = MockRuntimeManager::new();
    let a = manager
        .start(MockServerConfig::loopback(0), &[])
        .await
        .unwrap();
    let b = manager
        .start(MockServerConfig::loopback(0), &[])
        .await
        .unwrap();
    manager.stop_all().await;
    for addr in [a.addr, b.addr] {
        tokio::net::TcpListener::bind(addr)
            .await
            .expect("listener released");
    }
}

#[tokio::test]
async fn port_conflict_is_reported_without_crashing() {
    let first = start(&[]).await;
    let port = first.local_addr().port();

    let err = RunningServer::start(MockServerConfig::loopback(port), &[])
        .await
        .unwrap_err();
    assert!(
        matches!(err, StartError::PortInUse { addr } if addr.port() == port),
        "{err}"
    );
    assert!(err.to_string().contains(&port.to_string()));

    // The first server is unaffected.
    let res = client().get(url(&first, "/x")).send().await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    first.stop().await;
}

#[tokio::test]
async fn non_loopback_bind_requires_explicit_opt_in() {
    let config = MockServerConfig {
        bind: BindAddress {
            host: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            port: 0,
            allow_network_exposure: false,
        },
        ..MockServerConfig::default()
    };
    let err = RunningServer::start(config, &[]).await.unwrap_err();
    assert!(matches!(
        err,
        StartError::Bind(BindError::NetworkExposureNotAllowed { .. })
    ));
}

#[tokio::test]
async fn invalid_configuration_fails_to_start() {
    let a = endpoint(Method::Get, "/users/:id", MockResponse::default());
    let b = endpoint(Method::Get, "/users/:userId", MockResponse::default());
    let err = RunningServer::start(MockServerConfig::loopback(0), &[a, b])
        .await
        .unwrap_err();
    assert!(matches!(err, StartError::Build(_)));
    assert!(err.to_string().contains("route conflict"), "{err}");
}

#[tokio::test]
async fn disabled_endpoints_do_not_respond() {
    let mut legacy = endpoint(Method::Get, "/legacy", MockResponse::default());
    legacy.enabled = false;
    let server = start(&[legacy]).await;
    let res = client().get(url(&server, "/legacy")).send().await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    server.stop().await;
}

#[tokio::test]
async fn reload_swaps_routes_without_restarting() {
    let server = start(&[endpoint(Method::Get, "/v1", MockResponse::default())]).await;
    let addr = server.local_addr();
    let c = client();
    assert_eq!(
        c.get(url(&server, "/v2")).send().await.unwrap().status(),
        StatusCode::NOT_FOUND
    );

    server
        .reload(&[endpoint(
            Method::Get,
            "/v2",
            MockResponse::json(200, json!({ "v": 2 })),
        )])
        .unwrap();
    assert_eq!(server.local_addr(), addr);
    assert_eq!(
        c.get(url(&server, "/v2")).send().await.unwrap().status(),
        StatusCode::OK
    );
    assert_eq!(
        c.get(url(&server, "/v1")).send().await.unwrap().status(),
        StatusCode::NOT_FOUND
    );

    // A bad reload keeps the current routes.
    let dup = endpoint(Method::Get, "/x/:a", MockResponse::default());
    let dup2 = endpoint(Method::Get, "/x/:b", MockResponse::default());
    assert!(server.reload(&[dup, dup2]).is_err());
    assert_eq!(
        c.get(url(&server, "/v2")).send().await.unwrap().status(),
        StatusCode::OK
    );

    server.stop().await;
}

#[tokio::test]
async fn simulated_delay_is_applied_and_capped() {
    let slow = MockResponse {
        delay_ms: 300,
        ..MockResponse::default()
    };
    let very_slow = MockResponse {
        delay_ms: 60_000,
        ..MockResponse::default()
    };
    let config = MockServerConfig {
        max_delay_ms: 100,
        ..MockServerConfig::loopback(0)
    };
    let server = RunningServer::start(
        config,
        &[
            endpoint(Method::Get, "/slow", slow),
            endpoint(Method::Get, "/capped", very_slow),
        ],
    )
    .await
    .unwrap();
    let c = client();

    let t = Instant::now();
    c.get(url(&server, "/slow")).send().await.unwrap();
    assert!(
        t.elapsed() >= Duration::from_millis(100),
        "delay is capped at max_delay_ms but still applied"
    );

    let t = Instant::now();
    c.get(url(&server, "/capped")).send().await.unwrap();
    assert!(
        t.elapsed() < Duration::from_secs(5),
        "60s delay capped to 100ms"
    );

    server.stop().await;
}

#[tokio::test]
async fn oversized_bodies_are_rejected() {
    let config = MockServerConfig {
        max_body_bytes: 16,
        ..MockServerConfig::loopback(0)
    };
    let server = RunningServer::start(
        config,
        &[endpoint(Method::Post, "/upload", MockResponse::default())],
    )
    .await
    .unwrap();
    let res = client()
        .post(url(&server, "/upload"))
        .body("x".repeat(1024))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let body: Value = res.json().await.unwrap();
    assert_eq!(body["error"]["code"], "PAYLOAD_TOO_LARGE");
    server.stop().await;
}

#[tokio::test]
async fn history_captures_requests_with_redaction() {
    let e = endpoint(
        Method::Post,
        "/login",
        MockResponse::json(200, json!({ "token": "abc", "user": "alex" })),
    );
    let id = e.id;
    let server = start(&[e]).await;
    let c = client();

    c.post(url(&server, "/login?api_key=k&page=1"))
        .header("Authorization", "Bearer super-secret")
        .json(&json!({ "username": "alex", "password": "hunter2" }))
        .send()
        .await
        .unwrap();
    c.get(url(&server, "/missing")).send().await.unwrap();

    let records = wait_for_history(server.history(), 2).await;
    assert_eq!(records.len(), 2);

    // Newest first.
    assert_eq!(records[0].status, 404);
    assert_eq!(records[0].matched_endpoint, None);

    let login = &records[1];
    assert_eq!(login.method, "POST");
    assert_eq!(login.path, "/login");
    assert_eq!(login.status, 200);
    assert_eq!(login.matched_endpoint, Some(id));
    let auth = login
        .request_headers
        .iter()
        .find(|(k, _)| k == "authorization")
        .unwrap();
    assert_eq!(auth.1, "[REDACTED]");
    assert!(
        login
            .query
            .contains(&("api_key".into(), "[REDACTED]".into()))
    );
    assert!(login.query.contains(&("page".into(), "1".into())));
    let req_body: Value =
        serde_json::from_str(&login.request_body.as_ref().unwrap().content).unwrap();
    assert_eq!(req_body["password"], "[REDACTED]");
    assert_eq!(req_body["username"], "alex");
    let resp_body: Value =
        serde_json::from_str(&login.response_body.as_ref().unwrap().content).unwrap();
    assert_eq!(resp_body["token"], "[REDACTED]");

    let only_404 = server.history().list(&HistoryFilter {
        status: Some(404),
        ..HistoryFilter::default()
    });
    assert_eq!(only_404.len(), 1);

    server.history().clear();
    assert!(server.history().is_empty());
    server.stop().await;
}

#[tokio::test]
async fn history_can_be_disabled() {
    let mut config = MockServerConfig::loopback(0);
    config.history.enabled = false;
    let server = RunningServer::start(config, &[]).await.unwrap();
    client().get(url(&server, "/x")).send().await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(server.history().is_empty());
    server.stop().await;
}

#[tokio::test]
async fn method_not_allowed_sets_allow_header() {
    let server = start(&[
        endpoint(Method::Get, "/items", MockResponse::default()),
        endpoint(Method::Post, "/items", MockResponse::default()),
    ])
    .await;
    let res = client()
        .delete(url(&server, "/items"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(res.headers()["allow"], "GET, POST");
    server.stop().await;
}
