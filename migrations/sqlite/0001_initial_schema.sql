-- Initial schema for the local (desktop) store.
-- IDs are 16-byte UUID blobs; timestamps are RFC 3339 UTC text;
-- flexible configuration is JSON text.

CREATE TABLE workspaces (
    id          BLOB    PRIMARY KEY NOT NULL,
    name        TEXT    NOT NULL,
    archived_at TEXT,
    created_at  TEXT    NOT NULL,
    updated_at  TEXT    NOT NULL
);

CREATE TABLE projects (
    id              BLOB    PRIMARY KEY NOT NULL,
    workspace_id    BLOB    NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    name            TEXT    NOT NULL,
    description     TEXT    NOT NULL DEFAULT '',
    base_path       TEXT,
    default_headers TEXT    NOT NULL DEFAULT '{}',
    format_version  INTEGER NOT NULL,
    archived_at     TEXT,
    created_at      TEXT    NOT NULL,
    updated_at      TEXT    NOT NULL
);
CREATE INDEX projects_workspace_idx ON projects (workspace_id, created_at);

CREATE TABLE environments (
    id                     BLOB    PRIMARY KEY NOT NULL,
    project_id             BLOB    NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    name                   TEXT    NOT NULL,
    base_url               TEXT,
    default_headers        TEXT    NOT NULL DEFAULT '{}',
    variables              TEXT    NOT NULL DEFAULT '{}',
    bind_host              TEXT    NOT NULL,
    bind_port              INTEGER NOT NULL,
    allow_network_exposure BOOLEAN NOT NULL DEFAULT 0,
    created_at             TEXT    NOT NULL,
    updated_at             TEXT    NOT NULL,
    UNIQUE (project_id, name)
);

-- Queried fields are columns; request/response/rules/auth live in `config`.
CREATE TABLE endpoints (
    id          BLOB    PRIMARY KEY NOT NULL,
    project_id  BLOB    NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    name        TEXT    NOT NULL,
    description TEXT    NOT NULL DEFAULT '',
    method      TEXT    NOT NULL,
    path        TEXT    NOT NULL,
    enabled     BOOLEAN NOT NULL DEFAULT 1,
    position    INTEGER NOT NULL,
    config      TEXT    NOT NULL,
    created_at  TEXT    NOT NULL,
    updated_at  TEXT    NOT NULL
);
CREATE INDEX endpoints_project_idx ON endpoints (project_id, position);

-- endpoint_id is not a foreign key: logs outlive deleted endpoints.
CREATE TABLE request_logs (
    id           BLOB    PRIMARY KEY NOT NULL,
    project_id   BLOB    NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    server_id    BLOB    NOT NULL,
    endpoint_id  BLOB,
    timestamp    TEXT    NOT NULL,
    method       TEXT    NOT NULL,
    path         TEXT    NOT NULL,
    status       INTEGER NOT NULL,
    duration_ms  INTEGER NOT NULL,
    matched_rule TEXT,
    details      TEXT    NOT NULL
);
CREATE INDEX request_logs_project_time_idx ON request_logs (project_id, timestamp);
