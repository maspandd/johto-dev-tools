//! Standalone mock server: serve the endpoints and CRUD resources from a
//! project JSON file.
//!
//! ```text
//! workbench-mock <project.json> [--host <ip>] [--port <port>] [--expose]
//!                [--base-path <path>] [--data-dir <dir> | --no-persist]
//! ```
//!
//! Binds to 127.0.0.1:4010 by default. Binding to any other interface
//! requires `--expose`. Resource data is saved under
//! `.workbench-data/<project file name>/` next to the project file unless
//! `--data-dir` or `--no-persist` is given. Stop with Ctrl+C; listeners are
//! shut down gracefully and pending data is written.

use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use mock_runtime::{
    HistoryFilter, JsonFileRecordStore, MemoryRecordStore, MockServerConfig, RecordStore,
    RunningServer,
};
use serde::Deserialize;
use serde_json::Value;
use tracing_subscriber::EnvFilter;
use uuid::Uuid;
use workbench_core::ResourceId;
use workbench_domain::{Endpoint, Resource};

/// Size cap for project files read from disk.
const MAX_PROJECT_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Debug, Deserialize)]
struct ProjectFile {
    name: String,
    #[serde(default)]
    base_path: Option<String>,
    #[serde(default)]
    endpoints: Vec<Endpoint>,
    /// Parsed separately so resources without an `id` get a stable one.
    #[serde(default)]
    resources: Vec<Value>,
}

struct Args {
    project: PathBuf,
    host: Option<IpAddr>,
    port: Option<u16>,
    expose: bool,
    base_path: Option<String>,
    data_dir: Option<PathBuf>,
    persist: bool,
}

const USAGE: &str = "usage: workbench-mock <project.json> [--host <ip>] [--port <port>] [--expose] \
                     [--base-path <path>] [--data-dir <dir> | --no-persist]";

fn parse_args() -> Result<Args, String> {
    let mut project = None;
    let mut args = Args {
        project: PathBuf::new(),
        host: None,
        port: None,
        expose: false,
        base_path: None,
        data_dir: None,
        persist: true,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut value = |flag: &str| it.next().ok_or(format!("{flag} needs a value"));
        match arg.as_str() {
            "--host" => {
                let v = value("--host")?;
                args.host = Some(v.parse().map_err(|_| format!("invalid --host `{v}`"))?);
            }
            "--port" => {
                let v = value("--port")?;
                args.port = Some(v.parse().map_err(|_| format!("invalid --port `{v}`"))?);
            }
            "--base-path" => args.base_path = Some(value("--base-path")?),
            "--data-dir" => args.data_dir = Some(PathBuf::from(value("--data-dir")?)),
            "--no-persist" => args.persist = false,
            "--expose" => args.expose = true,
            "-h" | "--help" => return Err(USAGE.to_owned()),
            s if s.starts_with('-') => return Err(format!("unknown option `{s}`\n{USAGE}")),
            _ if project.is_none() => project = Some(PathBuf::from(arg)),
            _ => return Err(format!("unexpected argument `{arg}`\n{USAGE}")),
        }
    }
    args.project = project.ok_or_else(|| USAGE.to_owned())?;
    if !args.persist && args.data_dir.is_some() {
        return Err("--data-dir and --no-persist cannot be combined".to_owned());
    }
    Ok(args)
}

fn load_project(path: &Path) -> Result<(ProjectFile, Vec<Resource>), String> {
    let meta =
        std::fs::metadata(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    if meta.len() > MAX_PROJECT_BYTES {
        return Err(format!(
            "{} is larger than the {MAX_PROJECT_BYTES}-byte limit",
            path.display()
        ));
    }
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let mut project: ProjectFile = serde_json::from_str(&text)
        .map_err(|e| format!("invalid project file {}: {e}", path.display()))?;

    // Resources without an explicit id get one derived from their name, so
    // their saved data is found again on the next run.
    let resources = std::mem::take(&mut project.resources)
        .into_iter()
        .map(|mut raw| {
            if let Value::Object(map) = &mut raw
                && !map.contains_key("id")
            {
                let name = map.get("name").and_then(Value::as_str).unwrap_or_default();
                let stable = Uuid::new_v5(
                    &Uuid::NAMESPACE_URL,
                    format!("workbench-mock:{}:{name}", project.name).as_bytes(),
                );
                map.insert(
                    "id".to_owned(),
                    Value::String(ResourceId::from_uuid(stable).to_string()),
                );
            }
            serde_json::from_value::<Resource>(raw)
                .map_err(|e| format!("invalid resource in {}: {e}", path.display()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok((project, resources))
}

fn default_data_dir(project: &Path) -> PathBuf {
    let stem = project
        .file_stem()
        .map_or_else(|| "project".into(), |s| s.to_string_lossy().into_owned());
    project
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(".workbench-data")
        .join(stem)
}

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let args = match parse_args() {
        Ok(a) => a,
        Err(msg) => {
            eprintln!("{msg}");
            return ExitCode::from(2);
        }
    };
    let (project, resources) = match load_project(&args.project) {
        Ok(p) => p,
        Err(msg) => {
            eprintln!("error: {msg}");
            return ExitCode::FAILURE;
        }
    };

    let (store, data_note): (Arc<dyn RecordStore>, String) =
        if !args.persist || resources.is_empty() {
            (Arc::new(MemoryRecordStore::new()), "in memory".to_owned())
        } else {
            let dir = args
                .data_dir
                .unwrap_or_else(|| default_data_dir(&args.project));
            match JsonFileRecordStore::open(&dir) {
                Ok(store) => (Arc::new(store), format!("saved in {}", dir.display())),
                Err(e) => {
                    eprintln!("error: cannot use data directory {}: {e}", dir.display());
                    return ExitCode::FAILURE;
                }
            }
        };

    let mut config = MockServerConfig::default();
    if let Some(host) = args.host {
        config.bind.host = host;
    }
    if let Some(port) = args.port {
        config.bind.port = port;
    }
    config.bind.allow_network_exposure = args.expose;
    config.base_path = args.base_path.or(project.base_path);

    let server =
        match RunningServer::start_project(config, &project.endpoints, &resources, store).await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("error: {e}");
                return ExitCode::FAILURE;
            }
        };

    let base = format!(
        "http://{}{}",
        server.local_addr(),
        server
            .config()
            .base_path
            .as_deref()
            .unwrap_or("")
            .trim_end_matches('/')
    );
    println!("Project: {}", project.name);
    println!(
        "Mock server listening on {base} ({} endpoints, {} resources)",
        server.route_count(),
        resources.iter().filter(|r| r.enabled).count()
    );
    for ep in project.endpoints.iter().filter(|e| e.enabled) {
        println!("  {:<6} {base}{}", ep.method.as_str(), ep.path);
    }
    for r in resources.iter().filter(|r| r.enabled) {
        let count = server.resource_records(r.id).map_or(0, |v| v.len());
        println!("  CRUD   {base}{}  ({count} records, {data_note})", r.path);
    }
    println!("Press Ctrl+C to stop.");

    if let Err(e) = tokio::signal::ctrl_c().await {
        eprintln!("cannot listen for Ctrl+C: {e}");
    }
    let handled = server.history().list(&HistoryFilter::default()).len();
    server.stop().await;
    println!("Stopped. {handled} request(s) handled.");
    ExitCode::SUCCESS
}
