-- CRUD resources, their records, and saved dataset definitions.

CREATE TABLE resources (
    id                  BLOB    PRIMARY KEY NOT NULL,
    project_id          BLOB    NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    name                TEXT    NOT NULL,
    path                TEXT    NOT NULL,
    enabled             BOOLEAN NOT NULL DEFAULT 1,
    config              TEXT    NOT NULL,
    -- NULL until records are first written; the runtime seeds uninitialized resources.
    data_initialized_at TEXT,
    created_at          TEXT    NOT NULL,
    updated_at          TEXT    NOT NULL,
    UNIQUE (project_id, name)
);

CREATE TABLE resource_records (
    resource_id BLOB    NOT NULL REFERENCES resources (id) ON DELETE CASCADE,
    record_key  TEXT    NOT NULL,
    position    INTEGER NOT NULL,
    data        TEXT    NOT NULL,
    PRIMARY KEY (resource_id, record_key)
);
CREATE INDEX resource_records_order_idx ON resource_records (resource_id, position);

CREATE TABLE datasets (
    id          BLOB PRIMARY KEY NOT NULL,
    project_id  BLOB NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    config      TEXT NOT NULL,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL,
    UNIQUE (project_id, name)
);
