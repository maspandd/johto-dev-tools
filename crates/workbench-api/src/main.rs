//! `workbench-server`: the management API (default `127.0.0.1:8080`).
//!
//! ```text
//! workbench-server [--host <ip>] [--port <port>] [--data-dir <dir>]
//!                  [--database-url <url>] [--cors-origin <origin>]... [--no-cors]
//!                  [--history-retention <n>] [--web-dir <dir>]
//!                  [--mock-host <ip>] [--mock-public-host <name>]
//! ```
//!
//! Environment variables:
//! - `WORKBENCH_DATABASE_URL` — `sqlite://...` or `postgres://...`
//!   (default: SQLite file `<data-dir>/workbench.db`)
//! - `WORKBENCH_API_TOKEN` — require `Authorization: Bearer <token>`;
//!   mandatory when binding to a non-loopback address
//! - `WORKBENCH_SECRET_KEY` — 64 hex characters; otherwise the key in
//!   `<data-dir>/secret.key` is used (created on first run)
//! - `WORKBENCH_WEB_DIR` — built web UI to serve at `/`
//! - `WORKBENCH_MOCK_HOST` — where mock servers listen by default
//!   (e.g. `0.0.0.0` in a container; loopback otherwise)
//! - `WORKBENCH_MOCK_PUBLIC_HOST` — host name shown in mock server URLs
//!
//! The token is read only from the environment so it never appears in the
//! process list.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::process::ExitCode;

use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;
use workbench_api::{ApiConfig, AppState, DEFAULT_CORS_ORIGINS, MockDefaults, SecretKey, router};
use workbench_storage::Store;

struct Args {
    host: IpAddr,
    port: u16,
    data_dir: PathBuf,
    database_url: Option<String>,
    cors: Vec<String>,
    no_cors: bool,
    retention: usize,
    web_dir: Option<PathBuf>,
    mock_host: Option<IpAddr>,
    mock_public_host: Option<String>,
}

const USAGE: &str = "usage: workbench-server [--host <ip>] [--port <port>] [--data-dir <dir>] \
                     [--database-url <url>] [--cors-origin <origin>]... [--no-cors] \
                     [--history-retention <n>] [--web-dir <dir>] \
                     [--mock-host <ip>] [--mock-public-host <name>]";

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

fn parse_args() -> Result<Args, String> {
    let mock_host = match env("WORKBENCH_MOCK_HOST") {
        Some(v) => Some(
            v.parse()
                .map_err(|_| format!("invalid WORKBENCH_MOCK_HOST `{v}`"))?,
        ),
        None => None,
    };
    let mut args = Args {
        host: IpAddr::V4(Ipv4Addr::LOCALHOST),
        port: 8080,
        data_dir: PathBuf::from(".workbench"),
        database_url: env("WORKBENCH_DATABASE_URL"),
        cors: Vec::new(),
        no_cors: false,
        retention: 1_000,
        web_dir: env("WORKBENCH_WEB_DIR").map(PathBuf::from),
        mock_host,
        mock_public_host: env("WORKBENCH_MOCK_PUBLIC_HOST"),
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut value = |flag: &str| it.next().ok_or(format!("{flag} needs a value"));
        match arg.as_str() {
            "--host" => {
                let v = value("--host")?;
                args.host = v.parse().map_err(|_| format!("invalid --host `{v}`"))?;
            }
            "--port" => {
                let v = value("--port")?;
                args.port = v.parse().map_err(|_| format!("invalid --port `{v}`"))?;
            }
            "--data-dir" => args.data_dir = PathBuf::from(value("--data-dir")?),
            "--database-url" => args.database_url = Some(value("--database-url")?),
            "--cors-origin" => args.cors.push(value("--cors-origin")?),
            "--no-cors" => args.no_cors = true,
            "--history-retention" => {
                let v = value("--history-retention")?;
                args.retention = v
                    .parse()
                    .map_err(|_| format!("invalid --history-retention `{v}`"))?;
            }
            "--web-dir" => args.web_dir = Some(PathBuf::from(value("--web-dir")?)),
            "--mock-host" => {
                let v = value("--mock-host")?;
                args.mock_host = Some(
                    v.parse()
                        .map_err(|_| format!("invalid --mock-host `{v}`"))?,
                );
            }
            "--mock-public-host" => args.mock_public_host = Some(value("--mock-public-host")?),
            "-h" | "--help" => return Err(USAGE.to_owned()),
            other => return Err(format!("unknown argument `{other}`\n{USAGE}")),
        }
    }
    Ok(args)
}

async fn shutdown_signal() {
    if let Err(e) = tokio::signal::ctrl_c().await {
        tracing::error!(error = %e, "cannot listen for Ctrl+C");
        std::future::pending::<()>().await;
    }
}

/// `workbench-server --health-check [port]`: exits 0 when `GET /health`
/// on loopback answers 200. For container health checks in images
/// without a shell or curl.
fn health_check(port: Option<String>) -> ExitCode {
    use std::io::{Read, Write};
    use std::time::Duration;

    let port: u16 = port.and_then(|p| p.parse().ok()).unwrap_or(8080);
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    let check = || -> std::io::Result<bool> {
        let mut stream = std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(3))?;
        stream.set_read_timeout(Some(Duration::from_secs(3)))?;
        stream.write_all(b"GET /health HTTP/1.0\r\nHost: localhost\r\n\r\n")?;
        let mut head = [0u8; 16];
        let n = stream.read(&mut head)?;
        Ok(head[..n].starts_with(b"HTTP/1.0 200") || head[..n].starts_with(b"HTTP/1.1 200"))
    };
    match check() {
        Ok(true) => ExitCode::SUCCESS,
        _ => ExitCode::FAILURE,
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let mut raw = std::env::args().skip(1);
    if raw.next().as_deref() == Some("--health-check") {
        return health_check(raw.next());
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        // Plain text when logs go to a file or a container log.
        .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stdout()))
        .init();

    let args = match parse_args() {
        Ok(a) => a,
        Err(msg) => {
            eprintln!("{msg}");
            return ExitCode::from(2);
        }
    };
    let token = std::env::var("WORKBENCH_API_TOKEN")
        .ok()
        .filter(|t| !t.trim().is_empty());
    if !args.host.is_loopback() && token.is_none() {
        eprintln!(
            "error: refusing to expose the management API on {} without WORKBENCH_API_TOKEN",
            args.host
        );
        return ExitCode::FAILURE;
    }

    // Empty counts as unset (compose passes unset variables as "").
    let secret_key = match env("WORKBENCH_SECRET_KEY") {
        Some(hex) => SecretKey::from_hex(&hex),
        None => SecretKey::load_or_create(&args.data_dir.join("secret.key")),
    };
    let secret_key = match secret_key {
        Ok(k) => k,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };

    let store = match &args.database_url {
        Some(url) => Store::connect(url).await,
        None => {
            if let Err(e) = std::fs::create_dir_all(&args.data_dir) {
                eprintln!("error: cannot create {}: {e}", args.data_dir.display());
                return ExitCode::FAILURE;
            }
            Store::open_sqlite(args.data_dir.join("workbench.db")).await
        }
    };
    let store = match store {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: cannot open database: {e}");
            return ExitCode::FAILURE;
        }
    };

    let cors_origins = if args.no_cors {
        Vec::new()
    } else if !args.cors.is_empty() {
        args.cors
    } else if let Ok(list) = std::env::var("WORKBENCH_CORS_ORIGINS") {
        list.split(',')
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .collect()
    } else {
        DEFAULT_CORS_ORIGINS
            .iter()
            .map(|s| (*s).to_owned())
            .collect()
    };
    if let Some(dir) = &args.web_dir
        && !dir.join("index.html").is_file()
    {
        eprintln!(
            "error: {} has no index.html; build the web UI first",
            dir.display()
        );
        return ExitCode::FAILURE;
    }
    if let Some(host) = args.mock_host
        && !host.is_loopback()
    {
        eprintln!("warning: mock servers listen on {host} by default (network exposure)");
    }
    let config = ApiConfig {
        api_token: token,
        cors_origins,
        history_retention: args.retention,
        web_dir: args.web_dir.clone(),
        mock: MockDefaults {
            host: args.mock_host,
            public_host: args.mock_public_host.clone(),
        },
        ..ApiConfig::default()
    };

    let state = AppState::new(store.clone(), secret_key, config);
    let addr = SocketAddr::new(args.host, args.port);
    let listener = match TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("error: cannot listen on {addr}: {e}");
            return ExitCode::FAILURE;
        }
    };
    println!("Developer Workbench API listening on http://{addr}/api/v1");
    if args.web_dir.is_some() {
        println!("Web UI at http://{addr}/");
    }

    let runtime = state.runtime.clone();
    let served = axum::serve(listener, router(state))
        .with_graceful_shutdown(shutdown_signal())
        .await;
    runtime.stop_all().await;
    store.close().await;
    match served {
        Ok(()) => {
            println!("Stopped.");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
