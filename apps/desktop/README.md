# Developer Workbench desktop app

A Tauri 2 shell around the web UI from `apps/web`. The management API runs
inside the app on a random `127.0.0.1` port, with SQLite in the app's data
folder, so it works offline with no account and no separate server.

- Each launch creates a new random API token. Only the app's own pages
  receive it, so other programs and web pages on the machine cannot use the
  API.
- Mock servers started from the app listen on loopback, like the CLI and
  server defaults.
- Data, the database and the secret-encryption key live in the app data
  folder (`%APPDATA%\io.github.maspandd.workbench` on Windows,
  `~/Library/Application Support/io.github.maspandd.workbench` on macOS,
  `~/.local/share/io.github.maspandd.workbench` on Linux).

## Develop

Requirements: Rust 1.90+, Node 24+, pnpm, and the
[Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS
(WebView2 on Windows, which Windows 10/11 already have; `webkit2gtk-4.1` on
Linux).

```sh
pnpm install
pnpm dev:desktop      # Vite dev server + the app with hot reload
pnpm build:desktop    # installers in apps/desktop/src-tauri/target/release/bundle
```

The Rust crate in `src-tauri` is its own Cargo workspace: Tauri needs the
system WebView libraries, which the main workspace's CI and Docker builds do
not install. CI builds and tests it on Windows.

`app-icon.png` is the source for the icons in `src-tauri/icons`; regenerate
them with `pnpm --filter @workbench/desktop icons`.

## Limits

- Release builds are not code-signed yet, so Windows SmartScreen and macOS
  Gatekeeper warn on first launch.
- File downloads (exports) use the WebView's own download handling.
- Links to external sites (such as Documentation) do not open from the app.
