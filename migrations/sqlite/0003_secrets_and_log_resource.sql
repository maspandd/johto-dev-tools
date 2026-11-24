-- Request logs can be filtered by the CRUD resource that handled them.
ALTER TABLE request_logs ADD COLUMN resource_id BLOB;

-- Secret environment variables. Values are encrypted by the application
-- (XChaCha20-Poly1305); the database only ever sees ciphertext.
CREATE TABLE environment_secrets (
    environment_id BLOB NOT NULL REFERENCES environments (id) ON DELETE CASCADE,
    name           TEXT NOT NULL,
    nonce          BLOB NOT NULL,
    ciphertext     BLOB NOT NULL,
    created_at     TEXT NOT NULL,
    updated_at     TEXT NOT NULL,
    PRIMARY KEY (environment_id, name)
);
