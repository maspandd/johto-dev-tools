//! Repository tests, run against SQLite and (when configured) PostgreSQL.

mod support;

use std::net::{IpAddr, Ipv4Addr};

use chrono::{Duration, TimeZone, Utc};
use serde_json::json;
use workbench_core::{EndpointId, EnvironmentId, MockServerId, ProjectId, RequestId, WorkspaceId};
use workbench_domain::{
    AuthSimulation, CapturedBody, Condition, Endpoint, Environment, HistoryFilter, HistoryRecord,
    Method, MockResponse, Operator, ParamSpec, PathTemplate, ResponseRule, ValueSource,
};
use workbench_storage::{StorageError, Store};

fn endpoint(method: Method, path: &str) -> Endpoint {
    Endpoint::new(path, method, PathTemplate::parse(path).unwrap())
}

/// An endpoint using every configurable part, to prove nothing is lost.
fn rich_endpoint() -> Endpoint {
    let mut e = endpoint(Method::Post, "/api/users/:id/orders");
    e.description = "Create an order".into();
    e.auth = AuthSimulation::ApiKey {
        header: "X-Api-Key".into(),
    };
    e.request.query.push(ParamSpec {
        name: "dry_run".into(),
        required: false,
        description: "Validate only".into(),
    });
    e.request.headers.push(ParamSpec {
        name: "X-Tenant".into(),
        required: true,
        description: String::new(),
    });
    e.request.body_schema = Some(json!({ "type": "object", "required": ["sku"] }));
    e.request.body_example = Some(json!({ "sku": "A-1" }));
    let mut response =
        MockResponse::json(201, json!({ "id": "{{ uuid }}", "user": "{{ path.id }}" }));
    response
        .headers
        .insert("Location".into(), "/orders/{{ uuid }}".into());
    response.delay_ms = 120;
    e.response = response;
    e.rules.push(ResponseRule {
        name: "out of stock".into(),
        when: vec![Condition {
            source: ValueSource::Body {
                pointer: "/sku".into(),
            },
            operator: Operator::Equals,
            value: Some("SOLD-OUT".into()),
        }],
        response: MockResponse::json(409, json!({ "error": "out of stock" })),
    });
    e
}

async fn project(store: &Store) -> (WorkspaceId, ProjectId) {
    let ws = store.create_workspace("Team").await.unwrap();
    let p = store.create_project(ws.id, "Shop API").await.unwrap();
    (ws.id, p.id)
}

fn record(endpoint: Option<EndpointId>, method: &str, status: u16, minutes: i64) -> HistoryRecord {
    HistoryRecord {
        id: RequestId::new(),
        server_id: MockServerId::new(),
        timestamp: Utc.with_ymd_and_hms(2026, 11, 23, 10, 0, 0).unwrap()
            + Duration::minutes(minutes),
        method: method.into(),
        path: "/api/users".into(),
        query: vec![("page".into(), "1".into())],
        request_headers: vec![("authorization".into(), "[REDACTED]".into())],
        request_body: Some(CapturedBody {
            content: "{\"name\":\"Alex\"}".into(),
            size: 15,
            truncated: false,
        }),
        matched_endpoint: endpoint,
        matched_rule: None,
        matched_resource: None,
        status,
        duration_ms: 7,
        response_headers: vec![("content-type".into(), "application/json".into())],
        response_body: None,
    }
}

backend_tests! {
    async fn workspace_lifecycle(db) {
        let store = &db.store;
        let ws = store.create_workspace("  Platform  ").await.unwrap();
        assert_eq!(ws.name, "Platform");
        assert_eq!(store.get_workspace(ws.id).await.unwrap(), ws, "timestamps survive a round trip");

        let renamed = store.rename_workspace(ws.id, "Platform Team").await.unwrap();
        assert_eq!(renamed.name, "Platform Team");
        assert!(renamed.updated_at >= ws.updated_at);

        let archived = store.set_workspace_archived(ws.id, true).await.unwrap();
        assert!(archived.is_archived());
        assert!(store.list_workspaces(false).await.unwrap().is_empty());
        assert_eq!(store.list_workspaces(true).await.unwrap().len(), 1);

        let restored = store.set_workspace_archived(ws.id, false).await.unwrap();
        assert!(!restored.is_archived());
        assert_eq!(store.list_workspaces(false).await.unwrap().len(), 1);

        assert!(matches!(
            store.create_workspace(" ").await,
            Err(StorageError::Invalid { .. })
        ));

        store.delete_workspace(ws.id).await.unwrap();
        assert!(matches!(
            store.get_workspace(ws.id).await,
            Err(StorageError::NotFound { .. })
        ));
        assert!(matches!(
            store.delete_workspace(ws.id).await,
            Err(StorageError::NotFound { .. })
        ));
    }

    async fn project_lifecycle(db) {
        let store = &db.store;
        let (ws, id) = project(store).await;
        let mut p = store.get_project(id).await.unwrap();
        assert_eq!(p.name, "Shop API");
        assert_eq!(p.format_version, workbench_domain::PROJECT_FORMAT_VERSION);

        p.description = "Mock storefront".into();
        p.base_path = Some("/api".into());
        p.default_headers.insert("X-Mock".into(), "true".into());
        let saved = store.update_project(&p).await.unwrap();
        assert_eq!(saved.description, "Mock storefront");
        assert_eq!(saved.base_path.as_deref(), Some("/api"));
        assert_eq!(saved.default_headers["X-Mock"], "true");

        p.base_path = Some("/api/:version".into());
        assert!(matches!(store.update_project(&p).await, Err(StorageError::Invalid { .. })));

        store.set_project_archived(id, true).await.unwrap();
        assert!(store.list_projects(ws, false).await.unwrap().is_empty());
        assert_eq!(store.list_projects(ws, true).await.unwrap().len(), 1);
        store.set_project_archived(id, false).await.unwrap();

        assert!(matches!(
            store.create_project(WorkspaceId::new(), "Orphan").await,
            Err(StorageError::NotFound { entity: "workspace", .. })
        ));

        // Deleting a workspace removes its projects.
        store.delete_workspace(ws).await.unwrap();
        assert!(matches!(store.get_project(id).await, Err(StorageError::NotFound { .. })));
    }

    async fn environment_lifecycle(db) {
        let store = &db.store;
        let (_, project_id) = project(store).await;
        let mut dev = Environment::new(project_id, "Development");
        dev.base_url = Some("http://127.0.0.1:4010".into());
        dev.variables.insert("API_VERSION".into(), "v1".into());
        dev.default_headers.insert("X-Env".into(), "dev".into());
        dev.bind.port = 4011;
        store.create_environment(&dev).await.unwrap();
        assert_eq!(store.get_environment(dev.id).await.unwrap(), dev);

        let duplicate = Environment::new(project_id, "Development");
        assert!(matches!(
            store.create_environment(&duplicate).await,
            Err(StorageError::Conflict(_))
        ));

        let testing = Environment::new(project_id, "Testing");
        store.create_environment(&testing).await.unwrap();
        let names: Vec<_> = store
            .list_environments(project_id)
            .await
            .unwrap()
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert_eq!(names, ["Development", "Testing"]);

        dev.variables.insert("TIMEOUT".into(), "30".into());
        dev.bind.host = IpAddr::V4(Ipv4Addr::UNSPECIFIED);
        assert!(matches!(store.update_environment(&dev).await, Err(StorageError::Invalid { .. })));
        dev.bind.allow_network_exposure = true;
        let saved = store.update_environment(&dev).await.unwrap();
        assert_eq!(saved.variables.len(), 2);
        assert_eq!(saved.bind.host, IpAddr::V4(Ipv4Addr::UNSPECIFIED));

        let mut renamed = testing.clone();
        renamed.name = "Development".into();
        assert!(matches!(store.update_environment(&renamed).await, Err(StorageError::Conflict(_))));

        store.delete_environment(testing.id).await.unwrap();
        assert!(matches!(
            store.get_environment(EnvironmentId::new()).await,
            Err(StorageError::NotFound { .. })
        ));
    }

    async fn endpoint_round_trip_preserves_every_field(db) {
        let store = &db.store;
        let (_, project_id) = project(store).await;
        let rich = rich_endpoint();
        store.create_endpoint(project_id, &rich).await.unwrap();

        let record = store.get_endpoint(rich.id).await.unwrap();
        assert_eq!(record.endpoint, rich);
        assert_eq!(record.project_id, project_id);
        assert_eq!(record.position, 0);

        let mut changed = rich.clone();
        changed.path = PathTemplate::parse("/api/v2/orders").unwrap();
        changed.method = Method::Put;
        changed.rules.clear();
        let saved = store.update_endpoint(&changed).await.unwrap();
        assert_eq!(saved.endpoint, changed);
        assert!(saved.updated_at >= record.updated_at);

        let mut invalid = changed.clone();
        invalid.response.status = 42;
        assert!(matches!(store.update_endpoint(&invalid).await, Err(StorageError::Invalid { .. })));
        assert!(matches!(
            store.create_endpoint(ProjectId::new(), &endpoint(Method::Get, "/x")).await,
            Err(StorageError::NotFound { entity: "project", .. })
        ));
    }

    async fn endpoint_ordering_enable_duplicate_delete(db) {
        let store = &db.store;
        let (_, project_id) = project(store).await;
        let a = endpoint(Method::Get, "/a");
        let b = endpoint(Method::Get, "/b");
        let c = endpoint(Method::Get, "/c");
        for e in [&a, &b, &c] {
            store.create_endpoint(project_id, e).await.unwrap();
        }
        let positions: Vec<_> = store
            .list_endpoint_records(project_id)
            .await
            .unwrap()
            .into_iter()
            .map(|r| (r.endpoint.id, r.position))
            .collect();
        assert_eq!(positions, [(a.id, 0), (b.id, 1), (c.id, 2)]);

        store.reorder_endpoints(project_id, &[c.id, a.id, b.id]).await.unwrap();
        let ids: Vec<_> = store.list_endpoints(project_id).await.unwrap().into_iter().map(|e| e.id).collect();
        assert_eq!(ids, [c.id, a.id, b.id]);
        assert!(matches!(
            store.reorder_endpoints(project_id, &[c.id, a.id]).await,
            Err(StorageError::Invalid { .. })
        ));
        assert!(matches!(
            store.reorder_endpoints(project_id, &[c.id, a.id, a.id]).await,
            Err(StorageError::Invalid { .. })
        ));

        let disabled = store.set_endpoint_enabled(a.id, false).await.unwrap();
        assert!(!disabled.endpoint.enabled);

        let copy = store.duplicate_endpoint(b.id).await.unwrap();
        assert_ne!(copy.endpoint.id, b.id);
        assert_eq!(copy.endpoint.name, "/b (copy)");
        assert!(!copy.endpoint.enabled, "copies start disabled to avoid route conflicts");
        assert_eq!(copy.endpoint.path, b.path);
        assert_eq!(copy.position, 3);

        store.delete_endpoint(c.id).await.unwrap();
        assert_eq!(store.list_endpoints(project_id).await.unwrap().len(), 3);
        assert!(matches!(store.delete_endpoint(c.id).await, Err(StorageError::NotFound { .. })));
    }

    async fn duplicated_project_is_independent(db) {
        let store = &db.store;
        let (ws, source_id) = project(store).await;
        let mut source = store.get_project(source_id).await.unwrap();
        source.base_path = Some("/api".into());
        store.update_project(&source).await.unwrap();
        store.create_environment(&Environment::new(source_id, "Development")).await.unwrap();
        let rich = rich_endpoint();
        let plain = endpoint(Method::Get, "/api/health");
        store.create_endpoint(source_id, &rich).await.unwrap();
        store.create_endpoint(source_id, &plain).await.unwrap();

        let copy = store.duplicate_project(source_id, None).await.unwrap();
        assert_ne!(copy.id, source_id);
        assert_eq!(copy.name, "Shop API (copy)");
        assert_eq!(copy.workspace_id, ws);
        assert_eq!(copy.base_path.as_deref(), Some("/api"));
        assert_eq!(store.get_project(copy.id).await.unwrap(), copy);

        let copied_envs = store.list_environments(copy.id).await.unwrap();
        assert_eq!(copied_envs.len(), 1);
        assert_eq!(copied_envs[0].project_id, copy.id);

        let copied = store.list_endpoints(copy.id).await.unwrap();
        assert_eq!(copied.len(), 2);
        assert_ne!(copied[0].id, rich.id);
        let strip = |e: &Endpoint| Endpoint { id: EndpointId::from_uuid(uuid::Uuid::nil()), ..e.clone() };
        assert_eq!(strip(&copied[0]), strip(&rich), "configuration is copied exactly, in order");
        assert_eq!(strip(&copied[1]), strip(&plain));

        // Changing the copy leaves the original untouched, and vice versa.
        let mut edited = copied[0].clone();
        edited.name = "Edited in copy".into();
        store.update_endpoint(&edited).await.unwrap();
        assert_eq!(store.get_endpoint(rich.id).await.unwrap().endpoint.name, rich.name);

        store.delete_project(source_id).await.unwrap();
        assert_eq!(store.list_endpoints(copy.id).await.unwrap().len(), 2);

        let named = store.duplicate_project(copy.id, Some("Staging mock")).await.unwrap();
        assert_eq!(named.name, "Staging mock");
    }

    async fn request_logs_round_trip_filter_and_prune(db) {
        let store = &db.store;
        let (_, project_id) = project(store).await;
        let ep = endpoint(Method::Get, "/api/users");
        store.create_endpoint(project_id, &ep).await.unwrap();

        let first = record(Some(ep.id), "GET", 200, 0);
        let second = record(None, "POST", 422, 1);
        let third = record(Some(ep.id), "GET", 404, 2);
        for r in [&first, &second, &third] {
            store.insert_request_log(project_id, r).await.unwrap();
        }

        let all = store.list_request_logs(project_id, &HistoryFilter::default()).await.unwrap();
        assert_eq!(all, vec![third.clone(), second.clone(), first.clone()], "newest first, nothing lost");

        let by = |f: HistoryFilter| async move {
            store.list_request_logs(project_id, &f).await.unwrap().into_iter().map(|r| r.id).collect::<Vec<_>>()
        };
        assert_eq!(by(HistoryFilter { method: Some("get".into()), ..Default::default() }).await, [third.id, first.id]);
        assert_eq!(by(HistoryFilter { status: Some(422), ..Default::default() }).await, [second.id]);
        assert_eq!(by(HistoryFilter { endpoint: Some(ep.id), ..Default::default() }).await, [third.id, first.id]);
        assert_eq!(by(HistoryFilter { since: Some(second.timestamp), ..Default::default() }).await, [third.id, second.id]);
        assert_eq!(by(HistoryFilter { until: Some(second.timestamp), ..Default::default() }).await, [second.id, first.id]);
        assert_eq!(by(HistoryFilter { limit: Some(1), ..Default::default() }).await, [third.id]);

        // Logs outlive a deleted endpoint.
        store.delete_endpoint(ep.id).await.unwrap();
        assert_eq!(store.list_request_logs(project_id, &HistoryFilter::default()).await.unwrap().len(), 3);

        assert_eq!(store.prune_request_logs(project_id, 2).await.unwrap(), 1);
        assert_eq!(by(HistoryFilter::default()).await, [third.id, second.id]);
        assert_eq!(store.clear_request_logs(project_id).await.unwrap(), 2);
        assert!(by(HistoryFilter::default()).await.is_empty());

        assert!(matches!(
            store.insert_request_log(ProjectId::new(), &first).await,
            Err(StorageError::NotFound { entity: "project", .. })
        ));
    }

    async fn deleting_a_project_cascades(db) {
        let store = &db.store;
        let (_, project_id) = project(store).await;
        let env = Environment::new(project_id, "Development");
        let ep = endpoint(Method::Get, "/a");
        store.create_environment(&env).await.unwrap();
        store.create_endpoint(project_id, &ep).await.unwrap();
        store.insert_request_log(project_id, &record(Some(ep.id), "GET", 200, 0)).await.unwrap();

        store.delete_project(project_id).await.unwrap();
        assert!(matches!(store.get_environment(env.id).await, Err(StorageError::NotFound { .. })));
        assert!(matches!(store.get_endpoint(ep.id).await, Err(StorageError::NotFound { .. })));
        assert!(store.list_request_logs(project_id, &HistoryFilter::default()).await.unwrap().is_empty());
    }

    async fn data_survives_reopen(db) {
        let (_, project_id) = project(&db.store).await;
        let rich = rich_endpoint();
        db.store.create_endpoint(project_id, &rich).await.unwrap();
        let before = db.store.get_project(project_id).await.unwrap();

        let store = db.reopen().await;
        assert_eq!(store.get_project(project_id).await.unwrap(), before);
        assert_eq!(store.list_endpoints(project_id).await.unwrap(), vec![rich]);
    }
}
