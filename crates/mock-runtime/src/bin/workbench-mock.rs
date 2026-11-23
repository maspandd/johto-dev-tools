//! Standalone mock server: serve the endpoints from a project JSON file.
//!
//! ```text
//! workbench-mock <project.json> [--host <ip>] [--port <port>] [--expose] [--base-path <path>]
//! ```
//!
//! Binds to 127.0.0.1:4010 by default. Binding to any other interface
//! requires `--expose`. Stop with Ctrl+C; listeners are shut down gracefully.

use std::net::IpAddr;
use std::path::PathBuf;
use std::process::ExitCode;

use mock_runtime::{HistoryFilter, MockServerConfig, RunningServer};
use serde::Deserialize;
use tracing_subscriber::EnvFilter;
use workbench_domain::Endpoint;

/// Size cap for project files read from disk.
const MAX_PROJECT_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Debug, Deserialize)]
struct ProjectFile {
    name: String,
    #[serde(default)]
    base_path: Option<String>,
    endpoints: Vec<Endpoint>,
}

struct Args {
    project: PathBuf,
    host: Option<IpAddr>,
    port: Option<u16>,
    expose: bool,
    base_path: Option<String>,
}

const USAGE: &str = "usage: workbench-mock <project.json> [--host <ip>] [--port <port>] [--expose] [--base-path <path>]";

fn parse_args() -> Result<Args, String> {
    let mut project = None;
    let mut args = Args {
        project: PathBuf::new(),
        host: None,
        port: None,
        expose: false,
        base_path: None,
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
            "--expose" => args.expose = true,
            "-h" | "--help" => return Err(USAGE.to_owned()),
            s if s.starts_with('-') => return Err(format!("unknown option `{s}`\n{USAGE}")),
            _ if project.is_none() => project = Some(PathBuf::from(arg)),
            _ => return Err(format!("unexpected argument `{arg}`\n{USAGE}")),
        }
    }
    args.project = project.ok_or_else(|| USAGE.to_owned())?;
    Ok(args)
}

fn load_project(path: &PathBuf) -> Result<ProjectFile, String> {
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
    serde_json::from_str(&text).map_err(|e| format!("invalid project file {}: {e}", path.display()))
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
    let project = match load_project(&args.project) {
        Ok(p) => p,
        Err(msg) => {
            eprintln!("error: {msg}");
            return ExitCode::FAILURE;
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

    let server = match RunningServer::start(config, &project.endpoints).await {
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
        "Mock server listening on {base} ({} routes)",
        server.route_count()
    );
    for ep in project.endpoints.iter().filter(|e| e.enabled) {
        println!("  {:<6} {base}{}", ep.method.as_str(), ep.path);
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
