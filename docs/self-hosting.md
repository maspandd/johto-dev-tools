# Self-hosting with Docker

One image holds the management API, the web UI and the mock servers. It
runs as an unprivileged user on a distroless base (no shell), and refuses to
start without an API token.

## With PostgreSQL (recommended)

```sh
cp .env.example .env
# Set WORKBENCH_API_TOKEN and POSTGRES_PASSWORD, e.g. with `openssl rand -hex 32`
docker compose up -d --build
```

Open <http://localhost:8080>, go to **Settings**, and enter the token.
Data lives in the `postgres-data` volume; the secret-encryption key lives in
`workbench-data` unless you set `WORKBENCH_SECRET_KEY`.

## Single container with SQLite

```sh
docker build -t developer-workbench .
docker run -d --name workbench \
  -p 127.0.0.1:8080:8080 -p 127.0.0.1:4010-4019:4010-4019 \
  -e WORKBENCH_API_TOKEN=<token> \
  -v workbench-data:/data \
  developer-workbench
```

## Mock servers

Mock servers listen inside the container on all interfaces, and their URLs
are shown as `localhost`. Publish the ports you start them on: the examples
publish 4010-4019, so pick a port in that range (or set an environment's
port). Browser pages from the same machine may call them (CORS); add other
origins when starting a server.

## Settings

| Variable | Default in the image | Meaning |
|---|---|---|
| `WORKBENCH_API_TOKEN` | (required) | Clients send `Authorization: Bearer <token>` |
| `WORKBENCH_DATABASE_URL` | SQLite in `/data` | `postgres://user:pass@host/db` for PostgreSQL |
| `WORKBENCH_SECRET_KEY` | key file in `/data` | 64 hex characters that encrypt environment secrets |
| `WORKBENCH_WEB_DIR` | `/srv/web` | Built web UI served at `/` |
| `WORKBENCH_MOCK_HOST` | `0.0.0.0` | Where mock servers listen by default |
| `WORKBENCH_MOCK_PUBLIC_HOST` | `localhost` | Host name shown in mock server URLs; set it to the host people use |
| `WORKBENCH_CORS_ORIGINS` | empty | Extra browser origins for the API (the UI is same-origin) |
| `RUST_LOG` | `info` | Log level |

The container health check calls `workbench-server --health-check 8080`.

## Exposing it to a network

The examples publish ports on `127.0.0.1` only. Before publishing on other
interfaces:

- use a long random `WORKBENCH_API_TOKEN` and keep it out of shell history;
- put the UI and API behind HTTPS (a reverse proxy such as Caddy or nginx),
  because the token is sent with every request;
- remember that mock servers have no authentication of their own beyond what
  each endpoint simulates; publish their ports only to people who should
  reach them, and set `WORKBENCH_MOCK_PUBLIC_HOST` so their URLs are right;
- back up the `postgres-data` volume and keep `WORKBENCH_SECRET_KEY` (or the
  `workbench-data` volume) with it: without the key, stored secrets cannot be
  decrypted.

PostgreSQL connections are not encrypted, so keep the database on the
compose network or another private network.
