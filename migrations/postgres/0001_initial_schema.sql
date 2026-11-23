-- Initial schema for the cloud / self-hosted store.
-- Mirrors migrations/sqlite/0001_initial_schema.sql with native types.

CREATE TABLE workspaces (
    id          UUID        PRIMARY KEY,
    name        TEXT        NOT NULL,
    archived_at TIMESTAMPTZ,
    created_at  TIMESTAMPTZ NOT NULL,
    updated_at  TIMESTAMPTZ NOT NULL
);

CREATE TABLE projects (
    id              UUID        PRIMARY KEY,
    workspace_id    UUID        NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    name            TEXT        NOT NULL,
    description     TEXT        NOT NULL DEFAULT '',
    base_path       TEXT,
    default_headers JSONB       NOT NULL DEFAULT '{}',
    format_version  INTEGER     NOT NULL,
    archived_at     TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL,
    updated_at      TIMESTAMPTZ NOT NULL
);
CREATE INDEX projects_workspace_idx ON projects (workspace_id, created_at);

CREATE TABLE environments (
    id                     UUID        PRIMARY KEY,
    project_id             UUID        NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    name                   TEXT        NOT NULL,
    base_url               TEXT,
    default_headers        JSONB       NOT NULL DEFAULT '{}',
    variables              JSONB       NOT NULL DEFAULT '{}',
    bind_host              TEXT        NOT NULL,
    bind_port              INTEGER     NOT NULL,
    allow_network_exposure BOOLEAN     NOT NULL DEFAULT FALSE,
    created_at             TIMESTAMPTZ NOT NULL,
    updated_at             TIMESTAMPTZ NOT NULL,
    UNIQUE (project_id, name)
);

-- Queried fields are columns; request/response/rules/auth live in `config`.
CREATE TABLE endpoints (
    id          UUID        PRIMARY KEY,
    project_id  UUID        NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    name        TEXT        NOT NULL,
    description TEXT        NOT NULL DEFAULT '',
    method      TEXT        NOT NULL,
    path        TEXT        NOT NULL,
    enabled     BOOLEAN     NOT NULL DEFAULT TRUE,
    position    INTEGER     NOT NULL,
    config      JSONB       NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL,
    updated_at  TIMESTAMPTZ NOT NULL
);
CREATE INDEX endpoints_project_idx ON endpoints (project_id, position);

-- endpoint_id is not a foreign key: logs outlive deleted endpoints.
CREATE TABLE request_logs (
    id           UUID        PRIMARY KEY,
    project_id   UUID        NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    server_id    UUID        NOT NULL,
    endpoint_id  UUID,
    timestamp    TIMESTAMPTZ NOT NULL,
    method       TEXT        NOT NULL,
    path         TEXT        NOT NULL,
    status       INTEGER     NOT NULL,
    duration_ms  BIGINT      NOT NULL,
    matched_rule TEXT,
    details      JSONB       NOT NULL
);
CREATE INDEX request_logs_project_time_idx ON request_logs (project_id, timestamp);
