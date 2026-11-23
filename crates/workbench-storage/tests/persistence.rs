//! Spec §11 scenario 1: create a project, restart, verify it persisted —
//! extended end to end: the reloaded project is served by a real mock
//! server, and its request history is written back to storage.

mod support;

use std::time::{Duration, Instant};

use mock_runtime::{HistoryFilter, MockServerConfig, RunningServer};
use serde_json::{Value, json};
use workbench_domain::{Endpoint, Environment, Method, MockResponse, PathTemplate};

backend_tests! {
    async fn project_survives_restart_and_serves_over_http(db) {
        // Session 1: build a project.
        let ws = db.store.create_workspace("Team").await.unwrap();
        let mut project = db.store.create_project(ws.id, "Users API").await.unwrap();
        project.base_path = Some("/api".into());
        project.default_headers.insert("X-Mock-Project".into(), "users".into());
        db.store.update_project(&project).await.unwrap();

        let mut env = Environment::new(project.id, "Development");
        env.bind.port = 0; // any free port
        db.store.create_environment(&env).await.unwrap();

        let mut get_user = Endpoint::new("Get user", Method::Get, PathTemplate::parse("/users/:id").unwrap());
        get_user.response = MockResponse::json(200, json!({ "id": "{{ path.id }}", "name": "Alex Morgan" }));
        let mut create_user = Endpoint::new("Create user", Method::Post, PathTemplate::parse("/users").unwrap());
        create_user.request.body_schema = Some(json!({ "type": "object", "required": ["name"] }));
        create_user.response = MockResponse::json(201, json!({ "name": "{{ body.name }}" }));
        db.store.create_endpoint(project.id, &get_user).await.unwrap();
        db.store.create_endpoint(project.id, &create_user).await.unwrap();

        // Restart: close every connection and open the database again.
        let store = db.reopen().await;

        // Session 2: everything is still there.
        let workspaces = store.list_workspaces(false).await.unwrap();
        assert_eq!(workspaces.len(), 1);
        let projects = store.list_projects(workspaces[0].id, false).await.unwrap();
        assert_eq!(projects.len(), 1);
        let project = &projects[0];
        assert_eq!(project.name, "Users API");
        let env = store.list_environments(project.id).await.unwrap().remove(0);
        let endpoints = store.list_endpoints(project.id).await.unwrap();
        assert_eq!(endpoints, vec![get_user.clone(), create_user.clone()]);

        // Serve the reloaded project.
        let config = MockServerConfig {
            bind: env.bind,
            base_path: project.base_path.clone(),
            default_headers: project.default_headers.clone(),
            ..MockServerConfig::default()
        };
        let server = RunningServer::start(config, &endpoints).await.unwrap();
        let base = format!("http://{}/api", server.local_addr());
        let client = reqwest::Client::builder().no_proxy().build().unwrap();

        let res = client.get(format!("{base}/users/7")).send().await.unwrap();
        assert_eq!(res.status(), 200);
        assert_eq!(res.headers()["x-mock-project"], "users");
        assert_eq!(res.json::<Value>().await.unwrap(), json!({ "id": "7", "name": "Alex Morgan" }));

        let res = client.post(format!("{base}/users")).json(&json!({})).send().await.unwrap();
        assert_eq!(res.status(), 422);

        // Persist the captured history (what the host app does in the background).
        let deadline = Instant::now() + Duration::from_secs(5);
        let records = loop {
            let r = server.history().list(&HistoryFilter::default());
            if r.len() >= 2 || Instant::now() > deadline {
                break r;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        };
        server.stop().await;
        for r in records.iter().rev() {
            store.insert_request_log(project.id, r).await.unwrap();
        }

        let store = db.reopen().await;
        let logs = store.list_request_logs(project.id, &HistoryFilter::default()).await.unwrap();
        assert_eq!(logs, records, "history survives a restart unchanged");
        assert_eq!(logs[0].status, 422);
        assert_eq!(logs[1].matched_endpoint, Some(get_user.id));
    }
}
