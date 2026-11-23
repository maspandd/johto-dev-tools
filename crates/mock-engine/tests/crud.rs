//! CRUD simulation through the public engine API.

use std::sync::Arc;

use mock_engine::{BuildProblem, DataStore, Engine, EngineConfig, MockOutcome, MockRequest};
use serde_json::{Value, json};
use workbench_domain::{
    DataSchema, DeleteBehavior, Endpoint, FieldKind, FieldSpec, KeyStrategy, Locale, Method,
    PathTemplate, RecordChange, Resource, SeedSpec,
};

fn path(p: &str) -> PathTemplate {
    PathTemplate::parse(p).unwrap()
}

fn users(count: usize) -> Resource {
    let mut r = Resource::new("users", path("/users"));
    r.key_strategy = KeyStrategy::Increment;
    r.schema = DataSchema::new(vec![
        FieldSpec::new("name", FieldKind::FullName),
        FieldSpec::new(
            "role",
            FieldKind::Enum {
                values: vec![json!("admin"), json!("member")],
            },
        ),
        FieldSpec::new("age", FieldKind::Integer { min: 18, max: 80 }),
    ]);
    r.seed = Some(SeedSpec {
        count,
        seed: 42,
        locale: Locale::En,
    });
    r.searchable = vec!["name".into()];
    r
}

fn orders() -> Resource {
    let mut r = Resource::new("orders", path("/orders"));
    r.schema = DataSchema::new(vec![
        FieldSpec::new(
            "user_id",
            FieldKind::Reference {
                resource: "users".into(),
                field: "id".into(),
            },
        ),
        FieldSpec::new(
            "total",
            FieldKind::Decimal {
                min: 1.0,
                max: 500.0,
                scale: 2,
            },
        ),
    ]);
    r.seed = Some(SeedSpec {
        count: 30,
        seed: 7,
        locale: Locale::En,
    });
    r.body_schema = Some(json!({
        "type": "object",
        "required": ["user_id", "total"],
        "properties": { "total": { "type": "number", "minimum": 0 } }
    }));
    r
}

/// Builds an engine and seeds every resource in dependency order.
fn engine(resources: &[Resource], base: Option<&str>) -> Engine {
    let config = EngineConfig {
        base_path: base.map(str::to_owned),
        ..EngineConfig::default()
    };
    let e =
        Engine::build_with_resources(&config, &[], resources, Arc::new(DataStore::new())).unwrap();
    for id in e.seed_order().to_vec() {
        let records = e.seed_records(id).unwrap();
        e.data().load(id, records);
    }
    e
}

fn send(e: &Engine, method: &str, uri: &str, body: Option<Value>) -> (MockOutcome, Value) {
    let mut req = MockRequest::new(method, uri);
    if let Some(b) = body {
        req = req
            .with_header("content-type", "application/json")
            .with_body(b.to_string());
    }
    let out = e.handle(&req);
    let json = serde_json::from_slice(&out.body).unwrap_or(Value::Null);
    (out, json)
}

#[test]
fn scenario_5_hundred_records_paginate() {
    let e = engine(&[users(100)], None);

    let (out, body) = send(&e, "GET", "/users", None);
    assert_eq!(out.status, 200);
    assert_eq!(body["data"].as_array().unwrap().len(), 20, "default limit");
    assert_eq!(
        body["meta"],
        json!({ "page": 1, "limit": 20, "total": 100, "total_pages": 5 })
    );
    assert_eq!(out.header("x-total-count"), Some("100"));

    let (_, body) = send(&e, "GET", "/users?_page=3&_limit=10", None);
    let ids: Vec<i64> = body["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|u| u["id"].as_i64().unwrap())
        .collect();
    assert_eq!(ids, (21..=30).collect::<Vec<_>>());
    assert_eq!(body["meta"]["total_pages"], 10);

    let (_, body) = send(&e, "GET", "/users?_page=11&_limit=10", None);
    assert!(
        body["data"].as_array().unwrap().is_empty(),
        "past the end is empty, not an error"
    );

    let (_, body) = send(&e, "GET", "/users?_limit=5000", None);
    assert_eq!(body["meta"]["limit"], 100, "clamped to max_limit");

    let (out, body) = send(&e, "GET", "/users?_page=0&_limit=x", None);
    assert_eq!(out.status, 422);
    assert_eq!(body["error"]["details"].as_array().unwrap().len(), 2);
}

#[test]
fn seeding_is_deterministic_and_assigns_keys() {
    let a = engine(&[users(10)], None);
    let b = engine(&[users(10)], None);
    let records_a = a.data().records(a.seed_order()[0]).unwrap();
    let records_b = b.data().records(b.seed_order()[0]).unwrap();
    assert_eq!(records_a, records_b);
    let first = &records_a[0];
    assert_eq!(first["id"], 1, "increment keys come first");
    assert_eq!(first.as_object().unwrap().keys().next().unwrap(), "id");

    let mut uuid_users = users(3);
    uuid_users.key_strategy = KeyStrategy::Uuid;
    let c = engine(&[uuid_users], None);
    let rec = &c.data().records(c.seed_order()[0]).unwrap()[0];
    assert!(uuid::Uuid::parse_str(rec["id"].as_str().unwrap()).is_ok());
}

#[test]
fn filter_search_and_sort() {
    let e = engine(&[users(60)], None);

    let (_, body) = send(&e, "GET", "/users?role=admin&_limit=100", None);
    let admins = body["data"].as_array().unwrap();
    assert!(!admins.is_empty());
    assert!(admins.iter().all(|u| u["role"] == "admin"));

    let (_, both) = send(&e, "GET", "/users?role=admin&role=member&_limit=100", None);
    assert_eq!(
        both["meta"]["total"], 60,
        "repeated filter matches any value"
    );

    let name = admins[0]["name"].as_str().unwrap();
    let needle = name.split(' ').next_back().unwrap().to_lowercase();
    let (_, found) = send(&e, "GET", &format!("/users?_q={needle}&_limit=100"), None);
    assert!(
        found["data"].as_array().unwrap().iter().all(|u| u["name"]
            .as_str()
            .unwrap()
            .to_lowercase()
            .contains(&needle))
    );

    let (_, sorted) = send(&e, "GET", "/users?_sort=-age,id&_limit=100", None);
    let rows = sorted["data"].as_array().unwrap();
    for w in rows.windows(2) {
        let (a, b) = (w[0]["age"].as_i64().unwrap(), w[1]["age"].as_i64().unwrap());
        assert!(a > b || (a == b && w[0]["id"].as_i64() < w[1]["id"].as_i64()));
    }
    let (_, asc) = send(&e, "GET", "/users?_sort=age&_order=desc&_limit=1", None);
    assert_eq!(asc["data"][0]["age"], rows[0]["age"]);
}

#[test]
fn scenario_6_crud_operations() {
    let e = engine(&[users(3)], Some("/api"));
    let id = e.seed_order()[0];

    // Create: key assigned, Location set, change emitted.
    let (out, body) = send(
        &e,
        "POST",
        "/api/users",
        Some(json!({ "name": "Sari", "role": "admin", "age": 31 })),
    );
    assert_eq!(out.status, 201);
    assert_eq!(body["data"]["id"], 4);
    assert_eq!(out.header("location"), Some("/api/users/4"));
    assert!(
        matches!(&out.changes[..], [(rid, RecordChange::Upsert { record })] if *rid == id && record.key == "4")
    );
    assert_eq!(out.matched_resource, Some(id));

    // Read.
    let (out, body) = send(&e, "GET", "/api/users/4", None);
    assert_eq!(out.status, 200);
    assert_eq!(body["data"]["name"], "Sari");

    // Duplicate key.
    let (out, body) = send(
        &e,
        "POST",
        "/api/users",
        Some(json!({ "id": 4, "name": "Dup" })),
    );
    assert_eq!(out.status, 409);
    assert_eq!(body["error"]["code"], "RECORD_CONFLICT");

    // Replace keeps the key and position.
    let (out, body) = send(
        &e,
        "PUT",
        "/api/users/4",
        Some(json!({ "name": "Sari W", "role": "member" })),
    );
    assert_eq!(out.status, 200);
    assert_eq!(
        body["data"],
        json!({ "id": 4, "name": "Sari W", "role": "member" })
    );

    // Patch merges; null removes a field.
    let (out, body) = send(
        &e,
        "PATCH",
        "/api/users/4",
        Some(json!({ "age": 32, "role": null })),
    );
    assert_eq!(out.status, 200);
    assert_eq!(
        body["data"],
        json!({ "id": 4, "name": "Sari W", "age": 32 })
    );

    // The key cannot change.
    let (out, _) = send(&e, "PATCH", "/api/users/4", Some(json!({ "id": 5 })));
    assert_eq!(out.status, 422);

    // The updated record is still last in the list.
    let (_, list) = send(&e, "GET", "/api/users", None);
    assert_eq!(list["data"][3]["name"], "Sari W");

    // Delete.
    let (out, _) = send(&e, "DELETE", "/api/users/4", None);
    assert_eq!(out.status, 204);
    assert!(matches!(&out.changes[..], [(_, RecordChange::Delete { key })] if key == "4"));
    assert_eq!(send(&e, "GET", "/api/users/4", None).0.status, 404);
    assert_eq!(send(&e, "DELETE", "/api/users/4", None).0.status, 404);
    assert_eq!(
        send(&e, "PUT", "/api/users/4", Some(json!({}))).0.status,
        404
    );

    // Bad bodies.
    assert_eq!(
        send(&e, "POST", "/api/users", Some(json!([1]))).0.status,
        422
    );
    assert_eq!(send(&e, "POST", "/api/users", None).0.status, 422);
    let bad = e.handle(
        &MockRequest::new("POST", "/api/users")
            .with_header("content-type", "application/json")
            .with_body("{nope"),
    );
    assert_eq!(bad.status, 400);
    assert_eq!(
        send(&e, "POST", "/api/users", Some(json!({ "id": {} })))
            .0
            .status,
        422
    );
}

#[test]
fn soft_delete_hides_records() {
    let mut r = users(2);
    r.delete = DeleteBehavior::Soft;
    let e = engine(&[r], None);
    let (out, _) = send(&e, "DELETE", "/users/1", None);
    assert_eq!(out.status, 204);
    let [(_, RecordChange::Upsert { record })] = &out.changes[..] else {
        panic!("soft delete keeps the record");
    };
    assert!(record.data["deleted_at"].is_string());
    assert_eq!(send(&e, "GET", "/users/1", None).0.status, 404);
    assert_eq!(send(&e, "GET", "/users", None).1["meta"]["total"], 1);
    assert_eq!(
        send(&e, "POST", "/users", Some(json!({ "id": 1 })))
            .0
            .status,
        409,
        "the key stays taken"
    );
}

#[test]
fn references_are_seeded_and_enforced() {
    // Order in the list does not matter: users are seeded first.
    let e = engine(&[orders(), users(5)], None);
    let user_ids: Vec<Value> = (1..=5).map(Value::from).collect();
    let (_, list) = send(&e, "GET", "/orders?_limit=100", None);
    let rows = list["data"].as_array().unwrap();
    assert_eq!(rows.len(), 30);
    assert!(rows.iter().all(|o| user_ids.contains(&o["user_id"])));

    let (ok, _) = send(
        &e,
        "POST",
        "/orders",
        Some(json!({ "user_id": 3, "total": 10.5 })),
    );
    assert_eq!(ok.status, 201);
    let (out, body) = send(
        &e,
        "POST",
        "/orders",
        Some(json!({ "user_id": 99, "total": 10.5 })),
    );
    assert_eq!(out.status, 422);
    assert_eq!(body["error"]["details"][0]["location"], "body/user_id");
    let (out, body) = send(
        &e,
        "POST",
        "/orders",
        Some(json!({ "user_id": 3, "total": -1 })),
    );
    assert_eq!(out.status, 422, "body_schema applies");
    assert_eq!(body["error"]["details"][0]["location"], "body/total");
}

#[test]
fn build_rejects_conflicts_cycles_and_unknown_references() {
    let data = || Arc::new(DataStore::new());
    let endpoint = Endpoint::new("list", Method::Get, path("/users"));
    let err =
        Engine::build_with_resources(&EngineConfig::default(), &[endpoint], &[users(1)], data())
            .unwrap_err();
    assert!(
        err.0
            .iter()
            .any(|p| matches!(p, BuildProblem::Conflict { owners, .. } if owners.len() == 2)),
        "{err}"
    );

    let mut a = Resource::new("a", path("/a"));
    a.schema = DataSchema::new(vec![FieldSpec::new(
        "b",
        FieldKind::Reference {
            resource: "b".into(),
            field: "id".into(),
        },
    )]);
    let mut b = Resource::new("b", path("/b"));
    b.schema = DataSchema::new(vec![FieldSpec::new(
        "a",
        FieldKind::Reference {
            resource: "a".into(),
            field: "id".into(),
        },
    )]);
    let err = Engine::build_with_resources(&EngineConfig::default(), &[], &[a.clone(), b], data())
        .unwrap_err();
    assert!(
        matches!(&err.0[..], [BuildProblem::ReferenceCycle(_)]),
        "{err}"
    );

    let err =
        Engine::build_with_resources(&EngineConfig::default(), &[], &[a], data()).unwrap_err();
    assert!(
        err.to_string().contains("unknown or disabled resource `b`"),
        "{err}"
    );

    let mut disabled = users(1);
    disabled.enabled = false;
    let e =
        Engine::build_with_resources(&EngineConfig::default(), &[], &[disabled], data()).unwrap();
    assert_eq!(e.handle(&MockRequest::new("GET", "/users")).status, 404);
}
