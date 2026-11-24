# Developer Workbench

**Build. Test. Simulate. Develop.**

An open-source developer workbench. The first module is **API Studio**: define
REST endpoints without writing a backend, then serve them from a real local
mock HTTP server with validation, response rules, dynamic templates and
request history.

> **Status: early development.** The mock engine, CRUD simulation, data
> generator, SQLite/PostgreSQL storage and the management API work today. The
> visual editor and desktop app are on the [roadmap](#roadmap). The product spec is in
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

### CRUD resources

A resource such as `{ "name": "users", "path": "/users" }` answers six routes with
stateful data: `GET/POST /users` and `GET/PUT/PATCH/DELETE /users/:id`.

| Setting | Meaning |
|---|---|
| `primary_key`, `key_strategy` | Key field (default `id`) and how new keys are made: `uuid` or `increment` |
| `schema` | Generated fields (`full_name`, `email`, `phone`, `address`, `company`, `integer`, `decimal`, `date`, `enum`, `template`, `object`, `array`, `reference`, ...) with `null_rate` and `unique` |
| `seed` | `{ "count", "seed", "locale": "en" \| "id" }` — generated once, the first time the resource starts |
| `body_schema` | JSON Schema checked against the stored record on every write |
| `searchable` | Fields matched by `_q` |
| `pagination` | `default_limit` and `max_limit` |
| `delete` | `hard`, or `soft` (records get `deleted_at` and disappear from results) |

List queries: `?field=value` (repeat for any-of), `_q=text`, `_sort=name,-age`
(or `_order=desc`), `_page`, `_limit`. Responses are `{ "data": ... }`; lists add
`"meta": { page, limit, total, total_pages }` and an `X-Total-Count` header.
`reference` fields must point at an existing record of the referenced resource.
Data survives restarts: the CLI keeps it in `.workbench-data/`, the server in its database.

## Management API

```sh
cargo run --bin workbench-server            # http://127.0.0.1:8080/api/v1
```

Data lives in `.workbench/` (SQLite database and the secret-encryption key) unless
`WORKBENCH_DATABASE_URL` points at PostgreSQL. Set `WORKBENCH_API_TOKEN` to require
`Authorization: Bearer <token>` (mandatory with `--host` other than loopback).

| Area | Routes (under `/api/v1`) |
|---|---|
| Workspaces | `GET POST /workspaces`, `GET PATCH DELETE /workspaces/{id}` |
| Projects | `GET /projects?workspace_id=`, `POST /projects`, `GET PATCH DELETE /projects/{id}`, `POST /projects/{id}/duplicate` |
| Environments | `GET POST /projects/{id}/environments`, `GET PATCH DELETE /environments/{id}` |
| Secrets | `GET /environments/{id}/secrets` (names only), `PUT DELETE /environments/{id}/secrets/{name}` |
| Endpoints | `GET POST /projects/{id}/endpoints`, `POST /projects/{id}/endpoints/reorder`, `GET PATCH DELETE /endpoints/{id}`, `POST /endpoints/{id}/duplicate\|enable\|disable` |
| Resources | `GET POST /projects/{id}/resources`, `GET PATCH DELETE /resources/{id}`, `GET /resources/{id}/records`, `POST /resources/{id}/reset` |
| Datasets | `GET POST /projects/{id}/datasets`, `POST /projects/{id}/datasets/preview`, `GET PATCH DELETE /datasets/{id}`, `GET /datasets/{id}/preview`, `GET /datasets/{id}/export?format=json\|csv` |
| Mock servers | `GET POST /projects/{id}/mock-servers`, `GET /mock-servers`, `GET PATCH DELETE /mock-servers/{id}`, `GET /mock-servers/{id}/logs` |
| Request logs | `GET DELETE /projects/{id}/request-logs` (filters: `method`, `status`, `endpoint_id`, `resource_id`, `server_id`, `since`, `until`, `limit`) |
| Health | `GET /health` (no token needed) |

`PATCH` takes a JSON merge patch. Collections accept `?page=&limit=`. Every
response has an `x-request-id` header, and errors use the shared envelope.
Endpoint and resource changes apply to running mock servers immediately; a
change that fails to apply is reported in the server's `last_reload_error`
while the server keeps its previous routes. Environment variables are
available in responses as `{{ env.NAME }}`. Secret values are encrypted at
rest (XChaCha20-Poly1305) and never returned by the API.

```sh
curl -X POST localhost:8080/api/v1/workspaces -H 'content-type: application/json' -d '{"name":"Team"}'
curl -X POST localhost:8080/api/v1/projects -H 'content-type: application/json' \
  -d '{"workspace_id":"<id>","name":"Shop","base_path":"/api"}'
curl -X POST localhost:8080/api/v1/projects/<id>/endpoints -H 'content-type: application/json' \
  -d '{"name":"Hello","method":"GET","path":"/hello/:name","response":{"body":{"type":"json","content":{"hi":"{{ path.name }}"}}}}'
curl -X POST localhost:8080/api/v1/projects/<id>/mock-servers -H 'content-type: application/json' -d '{"port":4010}'
curl localhost:4010/api/hello/sari          # {"hi":"sari"}
```

## Storage

`workbench-storage` persists workspaces, projects, environments (with encrypted
secrets), endpoints, resources and their records, datasets and
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
  workbench-api       Management API (/api/v1) and `workbench-server`
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
| 4 | Management API, environments, history API | Done |
| 5 | OpenAPI, cURL import, project archives | Next |
| 6 | Web UI (API Studio) | Planned |
| 7 | Desktop app and Docker self-hosting | Planned |

## Contributing and security

See [CONTRIBUTING.md](CONTRIBUTING.md) and [SECURITY.md](SECURITY.md).

## License

[Apache License 2.0](LICENSE)
