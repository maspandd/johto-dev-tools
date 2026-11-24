//! Encrypted secret storage and request-log filters by server and resource.

mod support;

use workbench_core::{EnvironmentId, MockServerId, ProjectId, RequestId, ResourceId};
use workbench_domain::{Environment, HistoryFilter, HistoryRecord};
use workbench_storage::{EncryptedSecret, StorageError, Store};

async fn environment(store: &Store) -> (ProjectId, EnvironmentId) {
    let ws = store.create_workspace("Team").await.unwrap();
    let p = store.create_project(ws.id, "Shop").await.unwrap();
    let env = Environment::new(p.id, "Development");
    store.create_environment(&env).await.unwrap();
    (p.id, env.id)
}

fn secret(byte: u8) -> EncryptedSecret {
    EncryptedSecret {
        nonce: vec![byte; 24],
        ciphertext: vec![byte, 0, 255, 7],
    }
}

fn log(server: MockServerId, resource: Option<ResourceId>) -> HistoryRecord {
    HistoryRecord {
        id: RequestId::new(),
        server_id: server,
        timestamp: workbench_core::time::now(),
        method: "GET".into(),
        path: "/x".into(),
        query: vec![],
        request_headers: vec![],
        request_body: None,
        matched_endpoint: None,
        matched_rule: None,
        matched_resource: resource,
        status: 200,
        duration_ms: 1,
        response_headers: vec![],
        response_body: None,
    }
}

backend_tests! {
    async fn secrets_round_trip_without_plaintext_columns(db) {
        let store = &db.store;
        let (project_id, env) = environment(store).await;
        store.put_secret(env, "API_TOKEN", &secret(1)).await.unwrap();
        store.put_secret(env, "DB_PASSWORD", &secret(2)).await.unwrap();
        assert_eq!(store.get_secret(env, "API_TOKEN").await.unwrap(), secret(1));

        let first = store.list_secrets(env).await.unwrap();
        assert_eq!(first.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["API_TOKEN", "DB_PASSWORD"]);

        // Replacing keeps created_at and changes the value.
        store.put_secret(env, "API_TOKEN", &secret(3)).await.unwrap();
        assert_eq!(store.get_secret(env, "API_TOKEN").await.unwrap(), secret(3));
        let again = store.list_secrets(env).await.unwrap();
        assert_eq!(again[0].created_at, first[0].created_at);
        assert!(again[0].updated_at >= first[0].updated_at);

        assert!(matches!(store.put_secret(env, "1bad", &secret(1)).await, Err(StorageError::Invalid { .. })));
        assert!(matches!(store.put_secret(EnvironmentId::new(), "X", &secret(1)).await, Err(StorageError::NotFound { entity: "environment", .. })));
        assert!(matches!(store.get_secret(env, "NOPE").await, Err(StorageError::NotFound { entity: "secret", .. })));

        store.delete_secret(env, "DB_PASSWORD").await.unwrap();
        assert!(matches!(store.delete_secret(env, "DB_PASSWORD").await, Err(StorageError::NotFound { .. })));

        // Secrets are not copied with a project, and go away with their environment.
        let copy = store.duplicate_project(project_id, None).await.unwrap();
        let copied_env = store.list_environments(copy.id).await.unwrap().remove(0);
        assert!(store.list_secrets(copied_env.id).await.unwrap().is_empty());
        store.delete_environment(env).await.unwrap();
        assert!(store.list_secrets(env).await.unwrap().is_empty());
    }

    async fn request_logs_filter_by_server_and_resource(db) {
        let store = &db.store;
        let (project_id, _) = environment(store).await;
        let (a, b) = (MockServerId::new(), MockServerId::new());
        let users = ResourceId::new();
        let first = log(a, Some(users));
        let second = log(b, None);
        store.insert_request_log(project_id, &first).await.unwrap();
        store.insert_request_log(project_id, &second).await.unwrap();

        let by = |f: HistoryFilter| async move {
            store.list_request_logs(project_id, &f).await.unwrap().into_iter().map(|r| r.id).collect::<Vec<_>>()
        };
        assert_eq!(by(HistoryFilter { server: Some(a), ..Default::default() }).await, [first.id]);
        assert_eq!(by(HistoryFilter { server: Some(b), ..Default::default() }).await, [second.id]);
        assert_eq!(by(HistoryFilter { resource: Some(users), ..Default::default() }).await, [first.id]);
        let stored = store.list_request_logs(project_id, &HistoryFilter::default()).await.unwrap();
        assert!(stored.contains(&first));
    }
}
