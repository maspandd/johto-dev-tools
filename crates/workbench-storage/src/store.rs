//! Connection handling, migrations, and backend dispatch.

use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

use sqlx::migrate::Migrator;
use sqlx::postgres::{PgPool, PgPoolOptions};
use sqlx::sqlite::{
    SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions, SqliteSynchronous,
};

use crate::{StorageError, StorageResult};

static SQLITE_MIGRATIONS: Migrator = sqlx::migrate!("../../migrations/sqlite");
static POSTGRES_MIGRATIONS: Migrator = sqlx::migrate!("../../migrations/postgres");

/// Which database a [`Store`] talks to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Sqlite,
    Postgres,
}

#[derive(Debug, Clone)]
pub(crate) enum Pool {
    Sqlite(SqlitePool),
    Postgres(PgPool),
}

/// Runs `$body` with `$pool` bound to the concrete pool type.
///
/// The body is compiled once per backend, so the same SQL and bindings
/// serve both. SQL uses `$1`-style placeholders, which SQLite also accepts.
macro_rules! with_pool {
    ($store:expr, |$pool:ident| $body:expr) => {
        match &$store.pool {
            $crate::store::Pool::Sqlite($pool) => $body,
            $crate::store::Pool::Postgres($pool) => $body,
        }
    };
}
pub(crate) use with_pool;

/// Persistent storage for workspaces, projects, environments, endpoints
/// and request logs. Cheap to clone; clones share the connection pool.
#[derive(Debug, Clone)]
pub struct Store {
    pub(crate) pool: Pool,
}

impl Store {
    /// Connects from a URL: `sqlite://path/to/file.db`, `sqlite::memory:`,
    /// or `postgres://...`. Pending migrations are applied.
    pub async fn connect(url: &str) -> StorageResult<Self> {
        if url.starts_with("sqlite:") {
            let options = SqliteConnectOptions::from_str(url)?.create_if_missing(true);
            let in_memory = url.contains(":memory:") || url.contains("mode=memory");
            Self::open_sqlite_with(options, in_memory).await
        } else if url.starts_with("postgres://") || url.starts_with("postgresql://") {
            let pool = PgPoolOptions::new()
                .max_connections(10)
                .acquire_timeout(Duration::from_secs(10))
                .connect(url)
                .await?;
            POSTGRES_MIGRATIONS.run(&pool).await?;
            Ok(Self {
                pool: Pool::Postgres(pool),
            })
        } else {
            Err(StorageError::UnsupportedUrl)
        }
    }

    /// Opens (creating if needed) a SQLite database file. Used by the desktop app.
    pub async fn open_sqlite(path: impl AsRef<Path>) -> StorageResult<Self> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true);
        Self::open_sqlite_with(options, false).await
    }

    /// A private in-memory SQLite database, gone when the store is dropped.
    pub async fn in_memory() -> StorageResult<Self> {
        Self::open_sqlite_with(SqliteConnectOptions::from_str("sqlite::memory:")?, true).await
    }

    async fn open_sqlite_with(
        options: SqliteConnectOptions,
        in_memory: bool,
    ) -> StorageResult<Self> {
        let options = options
            .foreign_keys(true)
            .busy_timeout(Duration::from_secs(5))
            .journal_mode(if in_memory {
                SqliteJournalMode::Memory
            } else {
                SqliteJournalMode::Wal
            })
            .synchronous(SqliteSynchronous::Normal);
        // Each in-memory connection is a separate database, so keep exactly one alive.
        let pool_options = if in_memory {
            SqlitePoolOptions::new()
                .max_connections(1)
                .min_connections(1)
                .idle_timeout(None)
                .max_lifetime(None)
        } else {
            SqlitePoolOptions::new().max_connections(8)
        };
        let pool = pool_options.connect_with(options).await?;
        SQLITE_MIGRATIONS.run(&pool).await?;
        Ok(Self {
            pool: Pool::Sqlite(pool),
        })
    }

    #[must_use]
    pub const fn backend(&self) -> Backend {
        match self.pool {
            Pool::Sqlite(_) => Backend::Sqlite,
            Pool::Postgres(_) => Backend::Postgres,
        }
    }

    /// Closes every connection. Further calls fail.
    pub async fn close(&self) {
        with_pool!(self, |pool| pool.close().await);
    }
}
