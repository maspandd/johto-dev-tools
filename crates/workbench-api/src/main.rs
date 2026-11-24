//! `workbench-server`: the management API (default `127.0.0.1:8080`).
//!
//! ```text
//! workbench-server [--host <ip>] [--port <port>] [--data-dir <dir>]
//!                  [--database-url <url>] [--cors-origin <origin>]... [--no-cors]
//!                  [--history-retention <n>]
//! ```
//!
//! Environment variables:
//! - `WORKBENCH_DATABASE_URL` — `sqlite://...` or `postgres://...`
//!   (default: SQLite file `<data-dir>/workbench.db`)
//! - `WORKBENCH_API_TOKEN` — require `Authorization: Bearer <token>`;
//!   mandatory when binding to a non-loopback address
//! - `WORKBENCH_SECRET_KEY` — 64 hex characters; otherwise the key in
//!   `<data-dir>/secret.key` is used (created on first run)
//!
//! The token is read only from the environment so it never appears in the
//! process list.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::process::ExitCode;

use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;
use workbench_api::{ApiConfig, AppState, DEFAULT_CORS_ORIGINS, SecretKey, router};
use workbench_storage::Store;

struct Args {
    host: IpAddr,
    port: u16,
    data_dir: PathBuf,
    database_url: Option<String>,
    cors: Vec<String>,
    no_cors: bool,
    retention: usize,
}

const USAGE: &str = "usage: workbench-server [--host <ip>] [--port <port>] [--data-dir <dir>] \
                     [--database-url <url>] [--cors-origin <origin>]... [--no-cors] \
                     [--history-retention <n>]";

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        host: IpAddr::V4(Ipv4Addr::LOCALHOST),
        port: 8080,
        data_dir: PathBuf::from(".workbench"),
        database_url: std::env::var("WORKBENCH_DATABASE_URL").ok(),
        cors: Vec::new(),
        no_cors: false,
        retention: 1_000,
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

    let secret_key = match std::env::var("WORKBENCH_SECRET_KEY") {
        Ok(hex) => SecretKey::from_hex(&hex),
        Err(_) => SecretKey::load_or_create(&args.data_dir.join("secret.key")),
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
    let config = ApiConfig {
        api_token: token,
        cors_origins,
        history_retention: args.retention,
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
