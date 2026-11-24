//! Developer Workbench desktop app.
//!
//! A Tauri 2 shell around the React UI from `apps/web`. The management API
//! runs in-process on a random loopback port, backed by SQLite in the
//! app's data folder, so the app works offline with no account. Each launch
//! gets a fresh random token that only the app's own pages receive, so
//! other local programs and web pages cannot use the API.

use std::error::Error;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::Path;
use std::sync::Mutex;

use tauri::{Manager, RunEvent, WebviewUrl, WebviewWindowBuilder};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tracing_subscriber::EnvFilter;
use workbench_api::{ApiConfig, AppState, DEFAULT_CORS_ORIGINS, SecretKey, router};
use workbench_storage::Store;

/// Origins of the app's own pages: the bundled UI on each platform, and
/// the Vite dev server during `tauri dev`.
const APP_ORIGINS: &[&str] = &[
    "http://tauri.localhost",
    "https://tauri.localhost",
    "tauri://localhost",
];

/// The embedded API, kept so it can be shut down cleanly on exit.
struct Host {
    state: AppState,
    store: Store,
    shutdown: Mutex<Option<oneshot::Sender<()>>>,
}

fn random_token() -> Result<String, Box<dyn Error>> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes)
        .map_err(|e| format!("system random number generator failed: {e}"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Opens the database and serves the API on `127.0.0.1:<free port>`.
async fn start_api(data_dir: &Path) -> Result<(Host, SocketAddr, String), Box<dyn Error>> {
    std::fs::create_dir_all(data_dir)?;
    let store = Store::open_sqlite(data_dir.join("workbench.db")).await?;
    let secret_key = SecretKey::load_or_create(&data_dir.join("secret.key"))?;
    let token = random_token()?;

    let cors_origins = APP_ORIGINS
        .iter()
        .chain(DEFAULT_CORS_ORIGINS)
        .map(|o| (*o).to_owned())
        .collect();
    let config = ApiConfig {
        api_token: Some(token.clone()),
        cors_origins,
        ..ApiConfig::default()
    };
    let state = AppState::new(store.clone(), secret_key, config);

    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
    let addr = listener.local_addr()?;
    let (tx, rx) = oneshot::channel::<()>();
    let app = router(state.clone());
    tauri::async_runtime::spawn(async move {
        let served = axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = rx.await;
            })
            .await;
        if let Err(e) = served {
            tracing::error!(error = %e, "embedded API stopped");
        }
    });
    tracing::info!(%addr, data_dir = %data_dir.display(), "embedded API listening");

    Ok((
        Host {
            state,
            store,
            shutdown: Mutex::new(Some(tx)),
        },
        addr,
        token,
    ))
}

/// Script run before the UI loads: tells the app's own pages where the API
/// is. Pages from any other origin get nothing.
fn connection_script(addr: SocketAddr, token: &str) -> String {
    let allowed: Vec<&str> = APP_ORIGINS
        .iter()
        .copied()
        .chain(std::iter::once("http://localhost:5173"))
        .collect();
    let connection = serde_json::json!({
        "baseUrl": format!("http://{addr}"),
        "token": token,
        "desktop": true,
    });
    format!(
        "if ({allowed}.includes(window.location.origin)) {{ \
           Object.defineProperty(window, '__WORKBENCH__', {{ value: Object.freeze({connection}) }}); \
         }}",
        allowed = serde_json::json!(allowed),
    )
}

pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stdout()))
        .init();

    let app = tauri::Builder::default()
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let (host, addr, token) = tauri::async_runtime::block_on(start_api(&data_dir))?;
            app.manage(host);
            WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("Developer Workbench")
                .inner_size(1440.0, 900.0)
                .min_inner_size(900.0, 600.0)
                .initialization_script(connection_script(addr, &token))
                .build()?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to build the Developer Workbench app");

    app.run(|handle, event| {
        if let RunEvent::Exit = event
            && let Some(host) = handle.try_state::<Host>()
        {
            // Stop mock servers and close the database before exiting.
            tauri::async_runtime::block_on(async {
                host.state.runtime.stop_all().await;
                if let Some(tx) = host.shutdown.lock().ok().and_then(|mut s| s.take()) {
                    let _ = tx.send(());
                }
                host.store.close().await;
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_only_serves_app_origins() {
        let script = connection_script("127.0.0.1:5000".parse().unwrap(), "abc");
        assert!(script.contains("\"http://tauri.localhost\""));
        assert!(script.contains("\"baseUrl\":\"http://127.0.0.1:5000\""));
        assert!(script.contains("window.location.origin"));
    }

    #[test]
    fn tokens_are_random_hex() {
        let a = random_token().unwrap();
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, random_token().unwrap());
    }
}
