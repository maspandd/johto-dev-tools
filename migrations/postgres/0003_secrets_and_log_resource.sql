-- Request logs can be filtered by the CRUD resource that handled them.
ALTER TABLE request_logs ADD COLUMN resource_id UUID;

-- Secret environment variables. Values are encrypted by the application
-- (XChaCha20-Poly1305); the database only ever sees ciphertext.
CREATE TABLE environment_secrets (
    environment_id UUID        NOT NULL REFERENCES environments (id) ON DELETE CASCADE,
    name           TEXT        NOT NULL,
    nonce          BYTEA       NOT NULL,
    ciphertext     BYTEA       NOT NULL,
    created_at     TIMESTAMPTZ NOT NULL,
    updated_at     TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (environment_id, name)
);
