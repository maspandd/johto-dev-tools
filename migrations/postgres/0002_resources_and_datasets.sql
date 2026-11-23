-- CRUD resources, their records, and saved dataset definitions.

CREATE TABLE resources (
    id                  UUID        PRIMARY KEY,
    project_id          UUID        NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    name                TEXT        NOT NULL,
    path                TEXT        NOT NULL,
    enabled             BOOLEAN     NOT NULL DEFAULT TRUE,
    config              JSONB       NOT NULL,
    -- NULL until records are first written; the runtime seeds uninitialized resources.
    data_initialized_at TIMESTAMPTZ,
    created_at          TIMESTAMPTZ NOT NULL,
    updated_at          TIMESTAMPTZ NOT NULL,
    UNIQUE (project_id, name)
);

CREATE TABLE resource_records (
    resource_id UUID   NOT NULL REFERENCES resources (id) ON DELETE CASCADE,
    record_key  TEXT   NOT NULL,
    position    BIGINT NOT NULL,
    data        JSONB  NOT NULL,
    PRIMARY KEY (resource_id, record_key)
);
CREATE INDEX resource_records_order_idx ON resource_records (resource_id, position);

CREATE TABLE datasets (
    id          UUID        PRIMARY KEY,
    project_id  UUID        NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    name        TEXT        NOT NULL,
    description TEXT        NOT NULL DEFAULT '',
    config      JSONB       NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL,
    updated_at  TIMESTAMPTZ NOT NULL,
    UNIQUE (project_id, name)
);
