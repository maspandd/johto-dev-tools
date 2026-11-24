//! Whole-project inserts (imports) and bulk endpoint inserts, on both backends.

mod support;

use serde_json::json;
use workbench_core::ProjectId;
use workbench_domain::{
    DataSchema, Dataset, Endpoint, Environment, FieldKind, FieldSpec, Method, PathTemplate,
    Project, Resource, StoredRecord,
};
use workbench_storage::{ProjectTree, StorageError, Store};

async fn tree(store: &Store) -> ProjectTree {
    let ws = store.create_workspace("Team").await.unwrap();
    let mut project = Project::new(ws.id, "Imported");
    project.base_path = Some("/api".into());
    let env = Environment::new(project.id, "Development");
    let endpoints = vec![
        Endpoint::new("A", Method::Get, PathTemplate::parse("/a").unwrap()),
        Endpoint::new("B", Method::Post, PathTemplate::parse("/b").unwrap()),
    ];
    let seeded = Resource::new("users", PathTemplate::parse("/users").unwrap());
    let fresh = Resource::new("orders", PathTemplate::parse("/orders").unwrap());
    let records = vec![StoredRecord {
        key: "u1".into(),
        position: 0,
        data: json!({ "id": "u1" }),
    }];
    let dataset = Dataset::new(
        project.id,
        "People",
        DataSchema::new(vec![FieldSpec::new("name", FieldKind::FullName)]),
        5,
    );
    ProjectTree {
        project,
        environments: vec![env],
        endpoints,
        resources: vec![(seeded, Some(records)), (fresh, None)],
        datasets: vec![dataset],
    }
}

async fn project_count(store: &Store, tree: &ProjectTree) -> usize {
    store
        .list_projects(tree.project.workspace_id, true)
        .await
        .unwrap()
        .len()
}

backend_tests! {
    async fn inserts_a_whole_project(db) {
        let store = &db.store;
        let t = tree(store).await;
        store.insert_project_tree(&t).await.unwrap();

        let id = t.project.id;
        assert_eq!(store.get_project(id).await.unwrap().base_path.as_deref(), Some("/api"));
        assert_eq!(store.list_environments(id).await.unwrap().len(), 1);
        let names: Vec<_> = store.list_endpoints(id).await.unwrap().into_iter().map(|e| e.name).collect();
        assert_eq!(names, ["A", "B"]);
        let records = store.load_resource_records(t.resources[0].0.id).await.unwrap().unwrap();
        assert_eq!(records[0].data, json!({ "id": "u1" }));
        assert_eq!(store.load_resource_records(t.resources[1].0.id).await.unwrap(), None, "not seeded yet");
        assert_eq!(store.list_datasets(id).await.unwrap().len(), 1);
    }

    async fn invalid_items_save_nothing(db) {
        let store = &db.store;
        let mut t = tree(store).await;
        t.endpoints[1].name = " ".into();
        t.datasets[0].count = 0;
        let Err(StorageError::Invalid { errors, .. }) = store.insert_project_tree(&t).await else {
            panic!("expected validation errors");
        };
        assert_eq!(errors.len(), 2, "{errors:?}");
        assert_eq!(project_count(store, &t).await, 0);

        let mut t = tree(store).await;
        t.environments[0].project_id = ProjectId::new();
        assert!(matches!(store.insert_project_tree(&t).await, Err(StorageError::Invalid { .. })));
    }

    async fn constraint_failure_rolls_back(db) {
        let store = &db.store;
        let mut t = tree(store).await;
        let mut twin = t.environments[0].clone();
        twin.id = workbench_core::EnvironmentId::new();
        t.environments.push(twin);
        assert!(matches!(store.insert_project_tree(&t).await, Err(StorageError::Conflict(_))));
        assert_eq!(project_count(store, &t).await, 0);
        assert!(matches!(
            store.get_project(t.project.id).await,
            Err(StorageError::NotFound { .. })
        ));
    }

    async fn bulk_endpoint_insert_is_all_or_nothing(db) {
        let store = &db.store;
        let ws = store.create_workspace("Team").await.unwrap();
        let p = store.create_project(ws.id, "P").await.unwrap();
        let first = Endpoint::new("First", Method::Get, PathTemplate::parse("/first").unwrap());
        store.create_endpoint(p.id, &first).await.unwrap();

        let batch = vec![
            Endpoint::new("Second", Method::Get, PathTemplate::parse("/second").unwrap()),
            Endpoint::new("Third", Method::Get, PathTemplate::parse("/third").unwrap()),
        ];
        store.create_endpoints(p.id, &batch).await.unwrap();
        let records = store.list_endpoint_records(p.id).await.unwrap();
        let order: Vec<_> = records.iter().map(|r| (r.endpoint.name.as_str(), r.position)).collect();
        assert_eq!(order, [("First", 0), ("Second", 1), ("Third", 2)]);

        // A repeated ID fails the whole batch.
        let fourth = Endpoint::new("Fourth", Method::Get, PathTemplate::parse("/fourth").unwrap());
        let again = vec![fourth, batch[0].clone()];
        assert!(matches!(store.create_endpoints(p.id, &again).await, Err(StorageError::Conflict(_))));
        assert_eq!(store.list_endpoints(p.id).await.unwrap().len(), 3);
    }
}
