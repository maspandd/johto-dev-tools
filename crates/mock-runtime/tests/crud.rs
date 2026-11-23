//! CRUD resources over real HTTP, with persistence across restarts.
//! Scenarios 5 and 6 refer to the spec's acceptance scenarios (§11).

use std::sync::Arc;
use std::time::Duration;

use mock_runtime::{
    HistoryFilter, JsonFileRecordStore, MemoryRecordStore, MockServerConfig, RecordStore,
    RunningServer, StartError,
};
use reqwest::{Client, StatusCode};
use serde_json::{Value, json};
use workbench_domain::{
    DataSchema, FieldKind, FieldSpec, KeyStrategy, Locale, PathTemplate, Resource, SeedSpec,
};

fn client() -> Client {
    Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap()
}

fn users(count: usize) -> Resource {
    let mut r = Resource::new("users", PathTemplate::parse("/users").unwrap());
    r.key_strategy = KeyStrategy::Increment;
    r.schema = DataSchema::new(vec![
        FieldSpec::new("name", FieldKind::FullName),
        FieldSpec::new("email", FieldKind::Email),
    ]);
    r.seed = Some(SeedSpec {
        count,
        seed: 1,
        locale: Locale::En,
    });
    r
}

async fn start(resources: &[Resource], store: Arc<dyn RecordStore>) -> RunningServer {
    RunningServer::start_project(MockServerConfig::loopback(0), &[], resources, store)
        .await
        .unwrap()
}

fn url(s: &RunningServer, path: &str) -> String {
    format!("http://{}{path}", s.local_addr())
}

async fn get_json(c: &Client, url: String) -> (StatusCode, Value) {
    let res = c.get(url).send().await.unwrap();
    let status = res.status();
    (status, res.json().await.unwrap_or(Value::Null))
}

#[tokio::test]
async fn scenario_5_generate_100_records_and_paginate() {
    let server = start(&[users(100)], Arc::new(MemoryRecordStore::new())).await;
    let c = client();

    let res = c
        .get(url(&server, "/users?_page=2&_limit=25"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    assert_eq!(res.headers()["x-total-count"], "100");
    let body: Value = res.json().await.unwrap();
    let ids: Vec<i64> = body["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|u| u["id"].as_i64().unwrap())
        .collect();
    assert_eq!(ids, (26..=50).collect::<Vec<_>>());
    assert_eq!(
        body["meta"],
        json!({ "page": 2, "limit": 25, "total": 100, "total_pages": 4 })
    );

    server.stop().await;
}

/// Runs create, read, update, patch and delete, restarts the server on the
/// same store, and checks every change survived and seeding did not rerun.
async fn crud_survives_restart(store: Arc<dyn RecordStore>) {
    let resource = users(3);
    let server = start(std::slice::from_ref(&resource), Arc::clone(&store)).await;
    let c = client();

    let res = c
        .post(url(&server, "/users"))
        .json(&json!({ "name": "Sari", "email": "sari@example.test" }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let location = res.headers()["location"].to_str().unwrap().to_owned();
    assert_eq!(location, "/users/4");

    let (status, body) = get_json(&c, url(&server, &location)).await;
    assert_eq!(status, 200);
    assert_eq!(body["data"]["name"], "Sari");

    let res = c
        .put(url(&server, "/users/1"))
        .json(&json!({ "name": "Replaced" }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let res = c
        .patch(url(&server, "/users/4"))
        .json(&json!({ "email": "sari.w@example.test" }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let res = c.delete(url(&server, "/users/2")).send().await.unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let (status, _) = get_json(&c, url(&server, "/users/2")).await;
    assert_eq!(status, 404);

    let before = server.resource_records(resource.id).unwrap();
    server.stop().await;

    // Restart on the same store: data is loaded, not reseeded.
    let server = start(std::slice::from_ref(&resource), Arc::clone(&store)).await;
    assert_eq!(server.resource_records(resource.id).unwrap(), before);
    let (_, list) = get_json(&c, url(&server, "/users")).await;
    let names: Vec<&str> = list["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|u| u["name"].as_str().unwrap())
        .collect();
    assert_eq!(names.len(), 3);
    assert_eq!(names[0], "Replaced");
    assert_eq!(names[2], "Sari");
    assert_eq!(list["data"][2]["email"], "sari.w@example.test");

    // Reset restores the seed data.
    server.reset_resource(resource.id).await.unwrap();
    let (_, list) = get_json(&c, url(&server, "/users")).await;
    assert_eq!(list["meta"]["total"], 3);
    assert_ne!(list["data"][0]["name"], "Replaced");
    server.stop().await;

    let server = start(std::slice::from_ref(&resource), store).await;
    let (_, list) = get_json(&c, url(&server, "/users")).await;
    assert_ne!(
        list["data"][0]["name"], "Replaced",
        "the reset itself was persisted"
    );
    server.stop().await;
}

#[tokio::test]
async fn scenario_6_crud_persists_in_memory_store() {
    crud_survives_restart(Arc::new(MemoryRecordStore::new())).await;
}

#[tokio::test]
async fn scenario_6_crud_persists_in_json_files() {
    let dir = std::env::temp_dir().join(format!("wb-crud-{}", uuid::Uuid::new_v4()));
    crud_survives_restart(Arc::new(JsonFileRecordStore::open(&dir).unwrap())).await;
    // A brand-new store instance on the same directory sees the same data.
    let reopened = JsonFileRecordStore::open(&dir).unwrap();
    let files = std::fs::read_dir(&dir).unwrap().count();
    assert_eq!(files, 1, "one file per resource");
    drop(reopened);
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn concurrent_writes_persist_in_the_order_applied() {
    let store = Arc::new(MemoryRecordStore::new());
    let resource = users(1);
    let server = start(
        std::slice::from_ref(&resource),
        Arc::clone(&store) as Arc<dyn RecordStore>,
    )
    .await;
    let c = client();

    let mut tasks = Vec::new();
    for i in 0..40 {
        let c = c.clone();
        let base = url(&server, "/users");
        tasks.push(tokio::spawn(async move {
            if i % 2 == 0 {
                c.patch(format!("{base}/1"))
                    .json(&json!({ "n": i }))
                    .send()
                    .await
                    .unwrap();
            } else {
                c.post(&base).json(&json!({ "n": i })).send().await.unwrap();
            }
        }));
    }
    for t in tasks {
        t.await.unwrap();
    }
    server.flush_records().await;

    let in_memory = server.resource_records(resource.id).unwrap();
    let persisted: Vec<Value> = store
        .load(resource.id)
        .await
        .unwrap()
        .unwrap()
        .into_iter()
        .map(|r| r.data)
        .collect();
    assert_eq!(in_memory.len(), 21);
    assert_eq!(persisted, in_memory, "the store matches memory exactly");
    server.stop().await;
}

#[tokio::test]
async fn reload_project_adds_resources_and_keeps_data() {
    let resource = users(2);
    let server = start(
        std::slice::from_ref(&resource),
        Arc::new(MemoryRecordStore::new()),
    )
    .await;
    let c = client();
    c.post(url(&server, "/users"))
        .json(&json!({ "name": "Kept" }))
        .send()
        .await
        .unwrap();

    let mut tags = Resource::new("tags", PathTemplate::parse("/tags").unwrap());
    tags.seed = Some(SeedSpec {
        count: 4,
        seed: 9,
        locale: Locale::Id,
    });
    tags.schema = DataSchema::new(vec![FieldSpec::new(
        "label",
        FieldKind::Words { min: 1, max: 2 },
    )]);
    server
        .reload_project(&[], &[resource.clone(), tags])
        .await
        .unwrap();

    let (_, list) = get_json(&c, url(&server, "/tags")).await;
    assert_eq!(list["meta"]["total"], 4);
    let (_, list) = get_json(&c, url(&server, "/users")).await;
    assert_eq!(list["meta"]["total"], 3, "existing data was not reseeded");

    // Endpoint-only reload keeps resources.
    server.reload(&[]).unwrap();
    assert_eq!(get_json(&c, url(&server, "/tags")).await.0, 200);
    server.stop().await;
}

#[tokio::test]
async fn history_records_the_resource() {
    let resource = users(1);
    let server = start(
        std::slice::from_ref(&resource),
        Arc::new(MemoryRecordStore::new()),
    )
    .await;
    client().get(url(&server, "/users/1")).send().await.unwrap();
    let mut records = Vec::new();
    for _ in 0..200 {
        records = server.history().list(&HistoryFilter::default());
        if !records.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(records[0].matched_resource, Some(resource.id));
    assert_eq!(records[0].matched_endpoint, None);
    server.stop().await;
}

#[tokio::test]
async fn seed_failures_stop_startup() {
    let mut orders = Resource::new("orders", PathTemplate::parse("/orders").unwrap());
    orders.schema = DataSchema::new(vec![FieldSpec::new(
        "user_id",
        FieldKind::Reference {
            resource: "users".into(),
            field: "id".into(),
        },
    )]);
    orders.seed = Some(SeedSpec {
        count: 5,
        seed: 0,
        locale: Locale::En,
    });
    // users has no seed, so orders has nothing to reference.
    let mut empty_users = users(0);
    empty_users.seed = None;
    let err = RunningServer::start_project(
        MockServerConfig::loopback(0),
        &[],
        &[orders, empty_users],
        Arc::new(MemoryRecordStore::new()),
    )
    .await
    .unwrap_err();
    assert!(matches!(err, StartError::Seed(_)), "{err}");
}
