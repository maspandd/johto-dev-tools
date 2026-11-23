# Developer Workbench

**Build. Test. Simulate. Develop.**

An open-source developer workbench. The first module is **API Studio**: define
REST endpoints without writing a backend, then serve them from a real local
mock HTTP server with validation, response rules, dynamic templates and
request history.

> **Status: early development.** The Rust mock engine and runtime work today
> from the command line, and the storage layer persists projects to SQLite or
> PostgreSQL. The management API, visual editor and desktop app are on the
> [roadmap](#roadmap). The product spec is in
> [docs/developer_workbench_product_spec.md](docs/developer_workbench_product_spec.md).

## Quick start

Requirements: Rust (stable, 1.88+). Node 24+ and pnpm are only needed for the web UI.

```sh
cargo run --bin workbench-mock -- examples/demo-project.json
```

```text
Mock server listening on http://127.0.0.1:4010 (6 routes)
  GET    http://127.0.0.1:4010/api/users
  GET    http://127.0.0.1:4010/api/users/:id
  ...
```

Then call it from any client:

```sh
curl http://127.0.0.1:4010/api/users/42
curl -X POST http://127.0.0.1:4010/api/users \
  -H "content-type: application/json" -d '{"name":"Sari","email":"sari@example.test"}'
```

Options: `--port <n>`, `--host <ip>`, `--base-path </prefix>`, `--expose`.
The server binds to `127.0.0.1` only. Binding to any other interface requires `--expose`.

## What the mock engine does

| Capability | Details |
|---|---|
| Routing | `GET POST PUT PATCH DELETE`; `:id` or `{id}` path parameters; static segments beat parameters (`/users/me` before `/users/:id`); `405` with `Allow` header |
| Conflict detection | Two enabled endpoints with the same method and path shape are rejected with a clear error |
| Validation | Required headers and query parameters; JSON Schema for request bodies; errors list every problem with its location |
| Response rules | Ordered, declarative conditions on header, query, path or body (`equals`, `not_equals`, `contains`, `exists`, `absent`) |
| Templates | `{{ path.id }}`, `{{ query.page }}`, `{{ header.x-tenant }}`, `{{ body.user.name }}`, `{{ uuid }}`, `{{ now }}`, `{{ timestamp }}`, `{{ request.id }}` |
| Simulation | Status, headers, JSON or text body, delay (capped), bearer or API-key auth presence |
| Runtime | Start, stop, hot reload; port-conflict errors; graceful shutdown; body-size limit; several isolated servers at once |
| History | Captured off the response path; authorization headers and sensitive fields (`password`, `token`, …) redacted by default; retention limit; filters |

Errors use one JSON envelope everywhere:

```json
{ "error": { "code": "REQUEST_VALIDATION_FAILED", "message": "...", "details": [{ "location": "body/name", "message": "..." }], "request_id": "..." } }
```

### Endpoint format

```json
{
  "name": "Get user",
  "method": "GET",
  "path": "/api/users/:id",
  "auth": { "type": "bearer" },
  "request": { "query": [{ "name": "fields", "required": false }], "body_schema": null },
  "response": { "status": 200, "headers": {}, "delay_ms": 0,
                "body": { "type": "json", "content": { "id": "{{ path.id }}" } } },
  "rules": [{
    "name": "Unknown user",
    "when": [{ "source": { "from": "path", "name": "id" }, "operator": "equals", "value": "missing" }],
    "response": { "status": 404, "body": { "type": "json", "content": { "error": "not found" } } }
  }]
}
```

See [examples/demo-project.json](examples/demo-project.json) for a full project.

## Storage

`workbench-storage` persists workspaces, projects, environments, endpoints and
request logs behind one `Store` API:

| Backend | Used by | Connect with |
|---|---|---|
| SQLite | Desktop app, local use | `Store::open_sqlite(path)` or `sqlite://file.db` |
| PostgreSQL | Cloud and self-hosted | `postgres://user:pass@host/db` |

Both backends run the same SQL. Schemas live in [migrations/sqlite](migrations/sqlite)
and [migrations/postgres](migrations/postgres) and are applied on connect.
Queried fields are columns; flexible configuration (endpoint request, responses,
rules, headers, variables) is JSON (`JSONB` on PostgreSQL). Projects can be
archived, restored and duplicated; a duplicate copies environments and endpoints
under new IDs in one transaction.

## Repository layout

```text
crates/
  workbench-core      IDs, error codes and envelope, safe bind settings, timestamps
  workbench-domain    Workspace, Project, Environment, Endpoint, rules, request logs
  mock-engine         Pure request pipeline: route → auth → validate → rules → render
  mock-runtime        Axum/Tokio listeners, lifecycle, history; `workbench-mock` CLI
  workbench-storage   SQLite / PostgreSQL repositories and migrations
  workbench-api       (Phase 4) management API
  data-generator      Seeded synthetic data (en/id locales), JSON and CSV export
  openapi-engine      (Phase 5) OpenAPI 3.x import/export
  project-archive     (Phase 5) portable project archives
apps/
  web                 React + TypeScript + Vite + Tailwind (shell only for now)
  desktop             (Phase 7) Tauri 2 shell
```

## Development

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all

pnpm install
pnpm dev:web
pnpm lint && pnpm typecheck
```

Storage tests always run on SQLite. To also run them on PostgreSQL, point
`WORKBENCH_TEST_POSTGRES_URL` at a server where the user can create databases
(each test creates and drops its own):

```sh
docker run -d --name workbench-pg -e POSTGRES_PASSWORD=workbench -p 127.0.0.1:5432:5432 postgres:17-alpine
WORKBENCH_TEST_POSTGRES_URL=postgres://postgres:workbench@127.0.0.1:5432/postgres cargo test -p workbench-storage
```

## Roadmap

| Phase | Deliverable | Status |
|---|---|---|
| 0 | Repository, workspace, CI | Done |
| 1 | Mock engine and runtime | Done |
| 2 | Storage and persistence (SQLite, PostgreSQL) | Done |
| 3 | Data generator and CRUD simulation | Done |
| 4 | Management API, environments, history API | Next |
| 5 | OpenAPI, cURL import, project archives | Planned |
| 6 | Web UI (API Studio) | Planned |
| 7 | Desktop app and Docker self-hosting | Planned |

## Contributing and security

See [CONTRIBUTING.md](CONTRIBUTING.md) and [SECURITY.md](SECURITY.md).

## License

[Apache License 2.0](LICENSE)
