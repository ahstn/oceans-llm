use serial_test::serial;
use tempfile::{TempDir, tempdir};

use super::*;
use crate::tests::{
    PostgresTestDatabase, create_postgres_test_database, drop_postgres_test_database,
};
#[path = "test_support.rs"]
mod test_support;

struct TestDatabase {
    options: StoreConnectionOptions,
    _directory: Option<TempDir>,
    postgres: Option<PostgresTestDatabase>,
}

impl TestDatabase {
    fn libsql() -> Self {
        let directory = tempdir().expect("tempdir");
        Self {
            options: StoreConnectionOptions::Libsql {
                path: directory.path().join("gateway.db"),
            },
            _directory: Some(directory),
            postgres: None,
        }
    }

    async fn postgres() -> Option<Self> {
        let Some(database) = create_postgres_test_database().await else {
            eprintln!(
                "skipping postgres migration compatibility test because TEST_POSTGRES_URL is not set"
            );
            return None;
        };
        Some(Self {
            options: StoreConnectionOptions::Postgres {
                url: database.database_url.clone(),
                max_connections: 2,
            },
            _directory: None,
            postgres: Some(database),
        })
    }

    async fn finish(self) {
        if let Some(database) = self.postgres {
            drop_postgres_test_database(&database).await;
        }
    }

    async fn execute(&self, sql: &str) {
        match &self.options {
            StoreConnectionOptions::Libsql { path } => {
                let database = libsql::Builder::new_local(path)
                    .build()
                    .await
                    .expect("libsql database");
                database
                    .connect()
                    .expect("libsql connection")
                    .execute_batch(sql)
                    .await
                    .expect("execute libsql SQL");
            }
            StoreConnectionOptions::Postgres { url, .. } => {
                let pool = sqlx::postgres::PgPoolOptions::new()
                    .max_connections(1)
                    .connect(url)
                    .await
                    .expect("postgres pool");
                sqlx::raw_sql(sql)
                    .execute(&pool)
                    .await
                    .expect("execute postgres SQL");
                pool.close().await;
            }
        }
    }

    async fn add_newer(&self, version: u32, minimum: Option<u32>) {
        self.execute(&test_support::newer_migration_sql(version, minimum))
            .await;
    }

    async fn compatibility(&self) -> HashMap<u32, u32> {
        match &self.options {
            StoreConnectionOptions::Libsql { path } => {
                let database = libsql::Builder::new_local(path)
                    .build()
                    .await
                    .expect("libsql database");
                load_libsql_compatibility(&database.connect().expect("libsql connection"))
                    .await
                    .expect("libsql compatibility")
            }
            StoreConnectionOptions::Postgres { url, .. } => {
                let pool = sqlx::postgres::PgPoolOptions::new()
                    .max_connections(1)
                    .connect(url)
                    .await
                    .expect("postgres pool");
                let mut conn = pool.acquire().await.expect("postgres connection");
                let compatibility = load_postgres_compatibility(&mut conn)
                    .await
                    .expect("postgres compatibility");
                drop(conn);
                pool.close().await;
                compatibility
            }
        }
    }
}

async fn assert_all_commands_reject_newer(database: &TestDatabase, minimum: &str) {
    let errors = [
        status_migrations_with_options(&database.options)
            .await
            .expect_err("reject status"),
        check_migrations_with_options(&database.options)
            .await
            .expect_err("reject check"),
        run_migrations_with_options(&database.options)
            .await
            .expect_err("reject apply"),
    ];
    for error in errors {
        let message = error.to_string();
        for expected in [
            "database schema is newer than this gateway",
            "Deploy the newer release again",
            "restore the database backup taken before the upgrade",
            minimum,
            &format!("this gateway knows migrations up to v{}", registry_head()),
            &format!(
                "v{} (`future_{}`)",
                registry_head() + 1,
                registry_head() + 1
            ),
        ] {
            assert!(
                message.contains(expected),
                "expected {expected:?}: {message}"
            );
        }
        assert!(!message.contains("reset"), "{message}");
        assert!(!message.contains("recreate"), "{message}");
    }
}

async fn exercise_compatible_newer(database: TestDatabase) {
    run_migrations_with_options(&database.options)
        .await
        .expect("initial migrations");
    let head = registry_head();
    // Simulate both Additive and RequiresReader(head), then an older required reader.
    for (version, minimum) in [(head + 1, 0), (head + 2, head), (head + 3, head - 1)] {
        database.add_newer(version, Some(minimum)).await;
    }
    let before = database.compatibility().await;
    let status = status_migrations_with_options(&database.options)
        .await
        .expect("compatible status");
    assert_eq!(status.pending_count(), 0);
    assert_eq!(
        status.newer_applied,
        vec![
            NewerAppliedMigration {
                version: head + 1,
                name: format!("future_{}", head + 1),
                min_reader_version: 0
            },
            NewerAppliedMigration {
                version: head + 2,
                name: format!("future_{}", head + 2),
                min_reader_version: head
            },
            NewerAppliedMigration {
                version: head + 3,
                name: format!("future_{}", head + 3),
                min_reader_version: head - 1
            },
        ]
    );
    let checked = check_migrations_with_options(&database.options)
        .await
        .expect("compatible check");
    assert_eq!(checked.newer_applied, status.newer_applied);
    run_migrations_with_options(&database.options)
        .await
        .expect("compatible apply is a no-op");
    assert_eq!(database.compatibility().await, before);
    let after = status_migrations_with_options(&database.options)
        .await
        .expect("status after no-op");
    assert_eq!(after.newer_applied, status.newer_applied);
    database.finish().await;
}

async fn exercise_incompatible_newer(database: TestDatabase) {
    run_migrations_with_options(&database.options)
        .await
        .expect("initial migrations");
    database
        .add_newer(registry_head() + 1, Some(registry_head() + 1))
        .await;
    let minimum = format!("migration v{} or later", registry_head() + 1);
    assert_all_commands_reject_newer(&database, &minimum).await;
    database.finish().await;
}

async fn exercise_missing_compatibility(database: TestDatabase) {
    run_migrations_with_options(&database.options)
        .await
        .expect("initial migrations");
    database.add_newer(registry_head() + 1, None).await;
    assert_all_commands_reject_newer(&database, "without recording which older gateways").await;
    // A database from a binary that did not create the runner table must fail clearly, too.
    database
        .execute("DROP TABLE gateway_migration_compatibility")
        .await;
    assert_all_commands_reject_newer(&database, "without recording which older gateways").await;
    database.finish().await;
}

async fn exercise_pending_below_newer(database: TestDatabase) {
    run_migrations_with_options(&database.options)
        .await
        .expect("initial migrations");
    database.add_newer(registry_head() + 1, Some(0)).await;
    let pending = MIGRATION_REGISTRY[0].version;
    database
        .execute(&format!(
            "DELETE FROM refinery_schema_history WHERE version = {pending}"
        ))
        .await;
    let before = database.compatibility().await;
    let status = status_migrations_with_options(&database.options)
        .await
        .expect("compatible status with a gap");
    assert_eq!(status.pending_count(), 1);
    assert_eq!(status.newer_applied.len(), 1);
    let error = run_migrations_with_options(&database.options)
        .await
        .expect_err("must not fill a gap below newer history");
    assert!(
        error
            .to_string()
            .contains("cannot apply pending migrations")
    );
    assert_eq!(database.compatibility().await, before);
    let error = check_migrations_with_options(&database.options)
        .await
        .expect_err("pending check");
    assert!(error.to_string().contains("pending migrations"));
    database.finish().await;
}

async fn exercise_unknown_below_head(database: TestDatabase) {
    run_migrations_with_options(&database.options)
        .await
        .expect("initial migrations");
    let version = MIGRATION_REGISTRY[0].version - 1;
    database.add_newer(version, Some(0)).await;
    for error in [
        status_migrations_with_options(&database.options)
            .await
            .expect_err("unknown historical status"),
        check_migrations_with_options(&database.options)
            .await
            .expect_err("unknown historical check"),
        run_migrations_with_options(&database.options)
            .await
            .expect_err("unknown historical apply"),
    ] {
        assert!(error.to_string().contains("database reset required"));
        assert!(error.to_string().contains("outside the active registry"));
        assert!(error.to_string().contains("recreate the database"));
    }
    database.finish().await;
}

async fn exercise_fresh_compatibility(database: TestDatabase) {
    run_migrations_with_options(&database.options)
        .await
        .expect("initial migrations");
    let compatibility = database.compatibility().await;
    assert_eq!(compatibility.len(), MIGRATION_REGISTRY.len());
    for migration in MIGRATION_REGISTRY {
        assert_eq!(
            compatibility.get(&migration.version),
            Some(&migration.min_reader_version())
        );
    }
    let status = status_migrations_with_options(&database.options)
        .await
        .expect("fresh migrated status");
    assert!(status.newer_applied.is_empty());
    database.finish().await;
}

#[tokio::test]
#[serial]
async fn libsql_compatible_newer_migrations_allow_status_check_and_apply() {
    exercise_compatible_newer(TestDatabase::libsql()).await;
}

#[tokio::test]
#[serial]
async fn postgres_compatible_newer_migrations_allow_status_check_and_apply() {
    if let Some(database) = TestDatabase::postgres().await {
        exercise_compatible_newer(database).await;
    }
}

#[tokio::test]
#[serial]
async fn libsql_incompatible_newer_migrations_require_newer_release_or_restore() {
    exercise_incompatible_newer(TestDatabase::libsql()).await;
}

#[tokio::test]
#[serial]
async fn postgres_incompatible_newer_migrations_require_newer_release_or_restore() {
    if let Some(database) = TestDatabase::postgres().await {
        exercise_incompatible_newer(database).await;
    }
}

#[tokio::test]
#[serial]
async fn libsql_newer_migrations_without_compatibility_are_rejected() {
    exercise_missing_compatibility(TestDatabase::libsql()).await;
}

#[tokio::test]
#[serial]
async fn postgres_newer_migrations_without_compatibility_are_rejected() {
    if let Some(database) = TestDatabase::postgres().await {
        exercise_missing_compatibility(database).await;
    }
}

#[tokio::test]
#[serial]
async fn libsql_pending_migrations_below_newer_history_are_not_applied() {
    exercise_pending_below_newer(TestDatabase::libsql()).await;
}

#[tokio::test]
#[serial]
async fn postgres_pending_migrations_below_newer_history_are_not_applied() {
    if let Some(database) = TestDatabase::postgres().await {
        exercise_pending_below_newer(database).await;
    }
}

#[tokio::test]
#[serial]
async fn libsql_unknown_migration_below_registry_head_requires_reset() {
    exercise_unknown_below_head(TestDatabase::libsql()).await;
}

#[tokio::test]
#[serial]
async fn postgres_unknown_migration_below_registry_head_requires_reset() {
    if let Some(database) = TestDatabase::postgres().await {
        exercise_unknown_below_head(database).await;
    }
}

#[tokio::test]
#[serial]
async fn libsql_fresh_migrations_record_every_reader_requirement() {
    exercise_fresh_compatibility(TestDatabase::libsql()).await;
}

#[tokio::test]
#[serial]
async fn postgres_fresh_migrations_record_every_reader_requirement() {
    if let Some(database) = TestDatabase::postgres().await {
        exercise_fresh_compatibility(database).await;
    }
}
