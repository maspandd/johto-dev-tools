//! Resources, resource records and datasets on SQLite and PostgreSQL,
//! plus spec scenario 6 (CRUD data persists) through a real database.

mod support;

use std::sync::Arc;
use std::time::Duration;

use mock_runtime::{MockServerConfig, RecordFuture, RecordStore, RunningServer};
use serde_json::{Value, json};
use workbench_core::{ProjectId, ResourceId};
use workbench_domain::{
    DataSchema, Dataset, DeleteBehavior, FieldKind, FieldSpec, KeyStrategy, Locale, PathTemplate,
    RecordChange, Resource, SeedSpec, StoredRecord,
};
use workbench_storage::{StorageError, Store};

/// Bridges the runtime's record store to the database — what the host
/// application does in production.
struct DbRecords(Store);

impl RecordStore for DbRecords {
    fn load(&self, id: ResourceId) -> RecordFuture<'_, Option<Vec<StoredRecord>>> {
        Box::pin(async move { Ok(self.0.load_resource_records(id).await?) })
    }

    fn apply(&self, id: ResourceId, change: RecordChange) -> RecordFuture<'_, ()> {
        Box::pin(async move { Ok(self.0.apply_record_change(id, &change).await?) })
    }
}

fn users() -> Resource {
    let mut r = Resource::new("users", PathTemplate::parse("/users").unwrap());
    r.key_strategy = KeyStrategy::Increment;
    r.schema = DataSchema::new(vec![
        FieldSpec::new("name", FieldKind::FullName),
        FieldSpec::new("city", FieldKind::City),
    ]);
    r.seed = Some(SeedSpec {
        count: 25,
        seed: 11,
        locale: Locale::Id,
    });
    r.searchable = vec!["name".into()];
    r.delete = DeleteBehavior::Soft;
    r.body_schema = Some(json!({ "type": "object", "required": ["name"] }));
    r
}

async fn project(store: &Store) -> ProjectId {
    let ws = store.create_workspace("Team").await.unwrap();
    store.create_project(ws.id, "Shop").await.unwrap().id
}

fn rec(key: &str, position: i64, data: Value) -> StoredRecord {
    StoredRecord {
        key: key.into(),
        position,
        data,
    }
}

backend_tests! {
    async fn resource_definition_round_trip(db) {
        let store = &db.store;
        let project_id = project(store).await;
        let r = users();
        store.create_resource(project_id, &r).await.unwrap();

        let record = store.get_resource(r.id).await.unwrap();
        assert_eq!(record.resource, r, "every setting survives");
        assert_eq!(record.project_id, project_id);
        assert!(record.data_initialized_at.is_none());

        assert!(matches!(store.create_resource(project_id, &Resource { id: ResourceId::new(), ..users() }).await, Err(StorageError::Conflict(_))));
        assert!(matches!(store.create_resource(ProjectId::new(), &users()).await, Err(StorageError::NotFound { .. })));
        let mut bad = users();
        bad.name = "1bad".into();
        assert!(matches!(store.create_resource(project_id, &bad).await, Err(StorageError::Invalid { .. })));

        let mut changed = r.clone();
        changed.path = PathTemplate::parse("/members").unwrap();
        changed.enabled = false;
        changed.pagination.default_limit = 5;
        assert_eq!(store.update_resource(&changed).await.unwrap().resource, changed);
        assert_eq!(store.list_resources(project_id).await.unwrap(), vec![changed]);

        store.delete_resource(r.id).await.unwrap();
        assert!(matches!(store.get_resource(r.id).await, Err(StorageError::NotFound { .. })));
    }

    async fn record_changes_apply_in_order(db) {
        let store = &db.store;
        let project_id = project(store).await;
        let r = users();
        store.create_resource(project_id, &r).await.unwrap();
        assert_eq!(store.load_resource_records(r.id).await.unwrap(), None, "not initialized yet");

        store.apply_record_change(r.id, &RecordChange::Replace { records: vec![
            rec("1", 0, json!({ "id": 1, "name": "A" })),
            rec("2", 1, json!({ "id": 2, "name": "B" })),
        ] }).await.unwrap();
        assert!(store.get_resource(r.id).await.unwrap().data_initialized_at.is_some());

        store.apply_record_change(r.id, &RecordChange::Upsert { record: rec("3", 2, json!({ "id": 3, "nested": { "x": [1, 2] } })) }).await.unwrap();
        store.apply_record_change(r.id, &RecordChange::Upsert { record: rec("1", 0, json!({ "id": 1, "name": "A2" })) }).await.unwrap();
        store.apply_record_change(r.id, &RecordChange::Delete { key: "2".into() }).await.unwrap();

        let loaded = store.load_resource_records(r.id).await.unwrap().unwrap();
        assert_eq!(loaded, vec![
            rec("1", 0, json!({ "id": 1, "name": "A2" })),
            rec("3", 2, json!({ "id": 3, "nested": { "x": [1, 2] } })),
        ]);

        // Empty but initialized is different from never initialized.
        store.apply_record_change(r.id, &RecordChange::Replace { records: vec![] }).await.unwrap();
        assert_eq!(store.load_resource_records(r.id).await.unwrap(), Some(vec![]));

        store.reset_resource_records(r.id).await.unwrap();
        assert_eq!(store.load_resource_records(r.id).await.unwrap(), None, "reset means reseed");

        assert!(matches!(
            store.apply_record_change(ResourceId::new(), &RecordChange::Replace { records: vec![] }).await,
            Err(StorageError::NotFound { .. })
        ));
    }

    async fn dataset_lifecycle(db) {
        let store = &db.store;
        let project_id = project(store).await;
        let mut d = Dataset::new(project_id, "Customers", DataSchema::new(vec![
            FieldSpec { unique: true, ..FieldSpec::new("email", FieldKind::Email) },
            FieldSpec { null_rate: 0.25, ..FieldSpec::new("phone", FieldKind::Phone) },
        ]), 500);
        d.seed = u64::MAX;
        d.locale = Locale::Id;
        store.create_dataset(&d).await.unwrap();
        assert_eq!(store.get_dataset(d.id).await.unwrap(), d);

        assert!(matches!(
            store.create_dataset(&Dataset { id: workbench_core::DatasetId::new(), ..d.clone() }).await,
            Err(StorageError::Conflict(_))
        ));
        d.count = 0;
        assert!(matches!(store.update_dataset(&d).await, Err(StorageError::Invalid { .. })));
        d.count = 10;
        d.description = "Synthetic customers".into();
        assert_eq!(store.update_dataset(&d).await.unwrap().description, "Synthetic customers");
        assert_eq!(store.list_datasets(project_id).await.unwrap().len(), 1);

        // Stored config regenerates the same data.
        let saved = store.get_dataset(d.id).await.unwrap();
        let refs = data_generator::References::new();
        assert_eq!(
            data_generator::generate(&saved.schema, saved.count, saved.seed, saved.locale, &refs).unwrap(),
            data_generator::generate(&d.schema, d.count, d.seed, d.locale, &refs).unwrap()
        );

        store.delete_dataset(d.id).await.unwrap();
        assert!(store.list_datasets(project_id).await.unwrap().is_empty());
    }

    async fn duplicate_copies_resources_records_and_datasets(db) {
        let store = &db.store;
        let project_id = project(store).await;
        let r = users();
        store.create_resource(project_id, &r).await.unwrap();
        store.apply_record_change(r.id, &RecordChange::Replace { records: vec![rec("1", 0, json!({ "id": 1 }))] }).await.unwrap();
        let d = Dataset::new(project_id, "Sample", DataSchema::new(vec![FieldSpec::new("id", FieldKind::Uuid)]), 3);
        store.create_dataset(&d).await.unwrap();

        let copy = store.duplicate_project(project_id, None).await.unwrap();
        let copied = store.list_resources(copy.id).await.unwrap();
        assert_eq!(copied.len(), 1);
        assert_ne!(copied[0].id, r.id);
        assert_eq!(Resource { id: r.id, ..copied[0].clone() }, r);
        assert_eq!(store.load_resource_records(copied[0].id).await.unwrap(), Some(vec![rec("1", 0, json!({ "id": 1 }))]));
        assert_eq!(store.list_datasets(copy.id).await.unwrap()[0].name, "Sample");

        // Independent data.
        store.apply_record_change(copied[0].id, &RecordChange::Delete { key: "1".into() }).await.unwrap();
        assert_eq!(store.load_resource_records(r.id).await.unwrap().unwrap().len(), 1);
    }

    async fn scenario_6_crud_data_survives_restart_in_database(db) {
        let project_id = project(&db.store).await;
        let resource = users();
        db.store.create_resource(project_id, &resource).await.unwrap();

        let serve = |store: Store| async move {
            let resources = store.list_resources(project_id).await.unwrap();
            RunningServer::start_project(MockServerConfig::loopback(0), &[], &resources, Arc::new(DbRecords(store)))
                .await
                .unwrap()
        };
        let c = reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(10)).build().unwrap();

        // First run seeds 25 records into the database.
        let server = serve(db.store.clone()).await;
        let base = format!("http://{}/users", server.local_addr());
        let created: Value = c.post(&base).json(&json!({ "name": "Wulan Sari", "city": "Bandung" })).send().await.unwrap().json().await.unwrap();
        assert_eq!(created["data"]["id"], 26);
        assert_eq!(c.patch(format!("{base}/1")).json(&json!({ "city": "Medan" })).send().await.unwrap().status(), 200);
        assert_eq!(c.delete(format!("{base}/2")).send().await.unwrap().status(), 204);
        assert_eq!(c.post(&base).json(&json!({ "city": "x" })).send().await.unwrap().status(), 422);
        server.stop().await;

        // Restart the application: close the database and open it again.
        let store = db.reopen().await;
        let stored = store.load_resource_records(resource.id).await.unwrap().unwrap();
        assert_eq!(stored.len(), 26, "25 seeded + 1 created; soft delete keeps the row");
        assert!(stored.iter().any(|r| r.data["deleted_at"].is_string()));

        let server = serve(store).await;
        let base = format!("http://{}/users", server.local_addr());
        let list: Value = c.get(format!("{base}?_limit=100")).send().await.unwrap().json().await.unwrap();
        assert_eq!(list["meta"]["total"], 25, "the soft-deleted record stays hidden");
        let first: Value = c.get(format!("{base}/1")).send().await.unwrap().json().await.unwrap();
        assert_eq!(first["data"]["city"], "Medan");
        let found: Value = c.get(format!("{base}?_q=wulan")).send().await.unwrap().json().await.unwrap();
        assert!(found["data"].as_array().unwrap().iter().any(|u| u["id"] == 26));
        assert_eq!(c.get(format!("{base}/2")).send().await.unwrap().status(), 404);
        server.stop().await;
    }
}
