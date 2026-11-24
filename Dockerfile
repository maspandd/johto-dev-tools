# syntax=docker/dockerfile:1.7
#
# Developer Workbench: management API, web UI and mock servers in one image.
#
#   docker build -t developer-workbench .
#   docker run --rm -p 127.0.0.1:8080:8080 -p 127.0.0.1:4010-4019:4010-4019 \
#     -e WORKBENCH_API_TOKEN=change-me -v workbench-data:/data developer-workbench
#
# See docs/self-hosting.md and compose.yaml.

# --- Web UI ------------------------------------------------------------------
FROM node:24-bookworm-slim AS web
WORKDIR /src
RUN corepack enable
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
COPY apps/web/package.json apps/web/
RUN --mount=type=cache,id=pnpm,target=/root/.local/share/pnpm/store \
    pnpm install --frozen-lockfile --filter @workbench/web
COPY apps/web apps/web
# The demo project is bundled into the UI.
COPY examples examples
RUN pnpm --filter @workbench/web build

# --- Server ------------------------------------------------------------------
FROM rust:1-bookworm AS server
WORKDIR /src
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked --bin workbench-server \
    && cp target/release/workbench-server /workbench-server \
    && mkdir /data-empty

# --- Runtime -----------------------------------------------------------------
# Distroless: no shell or package manager, runs as an unprivileged user.
FROM gcr.io/distroless/cc-debian12:nonroot

COPY --from=server /workbench-server /usr/local/bin/workbench-server
COPY --from=web /src/apps/web/dist /srv/web
COPY --from=server --chown=nonroot:nonroot /data-empty /data

# Mock servers listen on all interfaces inside the container so published
# ports reach them, and their URLs are shown as localhost. The UI is served
# from the same origin, so the API needs no CORS.
ENV WORKBENCH_WEB_DIR=/srv/web \
    WORKBENCH_MOCK_HOST=0.0.0.0 \
    WORKBENCH_MOCK_PUBLIC_HOST=localhost \
    WORKBENCH_CORS_ORIGINS=

VOLUME ["/data"]
EXPOSE 8080 4010-4019
USER nonroot

HEALTHCHECK --interval=15s --timeout=5s --start-period=10s --retries=3 \
    CMD ["/usr/local/bin/workbench-server", "--health-check", "8080"]

# Listening on all interfaces requires WORKBENCH_API_TOKEN; the server
# refuses to start without it.
ENTRYPOINT ["/usr/local/bin/workbench-server"]
CMD ["--host", "0.0.0.0", "--port", "8080", "--data-dir", "/data"]
