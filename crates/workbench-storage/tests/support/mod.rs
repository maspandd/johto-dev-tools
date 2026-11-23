//! Test databases for both backends.
//!
//! SQLite tests use a fresh file in a temp directory. PostgreSQL tests run
//! only when `WORKBENCH_TEST_POSTGRES_URL` points at a server where the
//! user may create databases; each test gets its own database, dropped
//! afterwards.

#![allow(dead_code)]

use sqlx::Connection;
use sqlx::postgres::PgConnection;
use tempfile::TempDir;
use workbench_storage::Store;

pub const POSTGRES_ENV: &str = "WORKBENCH_TEST_POSTGRES_URL";

enum Kind {
    Sqlite {
        dir: TempDir,
    },
    Postgres {
        admin_url: String,
        url: String,
        name: String,
    },
}

pub struct TestDb {
    pub store: Store,
    kind: Kind,
}

/// Replaces the database name in a `postgres://.../<db>?params` URL.
fn with_database(url: &str, name: &str) -> String {
    let (base, query) = url
        .split_once('?')
        .map_or((url, None), |(b, q)| (b, Some(q)));
    let (prefix, _) = base
        .rsplit_once('/')
        .expect("postgres URL has a database path");
    match query {
        Some(q) => format!("{prefix}/{name}?{q}"),
        None => format!("{prefix}/{name}"),
    }
}

impl TestDb {
    pub async fn sqlite() -> Self {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = Store::open_sqlite(dir.path().join("workbench.db"))
            .await
            .expect("open sqlite");
        Self {
            store,
            kind: Kind::Sqlite { dir },
        }
    }

    /// `None` when no PostgreSQL server is configured.
    pub async fn postgres() -> Option<Self> {
        let admin_url = std::env::var(POSTGRES_ENV).ok()?;
        let name = format!("wb_test_{}", uuid::Uuid::new_v4().simple());
        let mut admin = PgConnection::connect(&admin_url)
            .await
            .expect("connect to WORKBENCH_TEST_POSTGRES_URL");
        sqlx::query(&format!("CREATE DATABASE {name}"))
            .execute(&mut admin)
            .await
            .expect("create test database");
        admin.close().await.ok();
        let url = with_database(&admin_url, &name);
        let store = Store::connect(&url).await.expect("connect test database");
        Some(Self {
            store,
            kind: Kind::Postgres {
                admin_url,
                url,
                name,
            },
        })
    }

    /// Closes every connection and opens the same database again, as an
    /// application restart would. Migrations run again on open.
    pub async fn reopen(&mut self) -> Store {
        self.store.close().await;
        self.store = match &self.kind {
            Kind::Sqlite { dir } => Store::open_sqlite(dir.path().join("workbench.db"))
                .await
                .expect("reopen sqlite"),
            Kind::Postgres { url, .. } => Store::connect(url).await.expect("reopen postgres"),
        };
        self.store.clone()
    }

    pub async fn cleanup(self) {
        self.store.close().await;
        if let Kind::Postgres {
            admin_url, name, ..
        } = self.kind
            && let Ok(mut admin) = PgConnection::connect(&admin_url).await
        {
            let _ = sqlx::query(&format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"))
                .execute(&mut admin)
                .await;
        }
    }
}

/// Generates each test twice: `sqlite::name` and `postgres::name`.
/// The body gets `$db: &mut TestDb`.
#[macro_export]
macro_rules! backend_tests {
    ($(async fn $name:ident($db:ident) $body:block)*) => {
        mod sqlite {
            #[allow(unused_imports)]
            use super::*;
            $(
                #[tokio::test]
                async fn $name() {
                    let mut db = $crate::support::TestDb::sqlite().await;
                    {
                        let $db = &mut db;
                        $body
                    }
                    db.cleanup().await;
                }
            )*
        }
        mod postgres {
            #[allow(unused_imports)]
            use super::*;
            $(
                #[tokio::test]
                async fn $name() {
                    let Some(mut db) = $crate::support::TestDb::postgres().await else {
                        eprintln!("skipped: set {} to run PostgreSQL tests", $crate::support::POSTGRES_ENV);
                        return;
                    };
                    {
                        let $db = &mut db;
                        $body
                    }
                    db.cleanup().await;
                }
            )*
        }
    };
}
