use std::{collections::HashSet, path::Path};

use anyhow::{Context, bail};
use sqlx::Row;
use time::OffsetDateTime;

use crate::StoreConnectionOptions;

struct EmbeddedMigration {
    version: u32,
    name: &'static str,
    checksum: &'static str,
    libsql_sql: &'static str,
    postgres_sql: &'static str,
}

#[derive(Debug, Clone, Default)]
pub struct MigrationTestHook {
    pub fail_after_apply_version: Option<u32>,
}

impl MigrationTestHook {
    fn maybe_fail(&self, version: u32) -> anyhow::Result<()> {
        if self.fail_after_apply_version == Some(version) {
            bail!("forced migration failure after applying version {version}");
        }
        Ok(())
    }
}

const EMBEDDED_MIGRATIONS: &[EmbeddedMigration] = &[
    EmbeddedMigration {
        version: 1,
        name: "init",
        checksum: "V1__init.sql",
        libsql_sql: include_str!("../migrations/V1__init.sql"),
        postgres_sql: include_str!("../migrations/postgres/V1__init.sql"),
    },
    EmbeddedMigration {
        version: 2,
        name: "audit_baseline",
        checksum: "V2__audit_baseline.sql",
        libsql_sql: include_str!("../migrations/V2__audit_baseline.sql"),
        postgres_sql: include_str!("../migrations/postgres/V2__audit_baseline.sql"),
    },
    EmbeddedMigration {
        version: 3,
        name: "identity_foundation",
        checksum: "V3__identity_foundation.sql",
        libsql_sql: include_str!("../migrations/V3__identity_foundation.sql"),
        postgres_sql: include_str!("../migrations/postgres/V3__identity_foundation.sql"),
    },
    EmbeddedMigration {
        version: 4,
        name: "money_fixed_point",
        checksum: "V4__money_fixed_point.sql",
        libsql_sql: include_str!("../migrations/V4__money_fixed_point.sql"),
        postgres_sql: include_str!("../migrations/postgres/V4__money_fixed_point.sql"),
    },
    EmbeddedMigration {
        version: 5,
        name: "pricing_catalog_cache",
        checksum: "V5__pricing_catalog_cache.sql",
        libsql_sql: include_str!("../migrations/V5__pricing_catalog_cache.sql"),
        postgres_sql: include_str!("../migrations/postgres/V5__pricing_catalog_cache.sql"),
    },
    EmbeddedMigration {
        version: 6,
        name: "identity_onboarding",
        checksum: "V6__identity_onboarding.sql",
        libsql_sql: include_str!("../migrations/V6__identity_onboarding.sql"),
        postgres_sql: include_str!("../migrations/postgres/V6__identity_onboarding.sql"),
    },
    EmbeddedMigration {
        version: 7,
        name: "user_password_rotation",
        checksum: "V7__user_password_rotation.sql",
        libsql_sql: include_str!("../migrations/V7__user_password_rotation.sql"),
        postgres_sql: include_str!("../migrations/postgres/V7__user_password_rotation.sql"),
    },
];

pub async fn run_migrations(path: impl AsRef<Path>) -> anyhow::Result<()> {
    run_migrations_with_options(
        &StoreConnectionOptions::Libsql {
            path: path.as_ref().to_path_buf(),
        },
        MigrationTestHook::default(),
    )
    .await
}

pub async fn run_migrations_with_options(
    options: &StoreConnectionOptions,
    hook: MigrationTestHook,
) -> anyhow::Result<()> {
    match options {
        StoreConnectionOptions::Libsql { path } => run_libsql_migrations(path, &hook).await,
        StoreConnectionOptions::Postgres { url, .. } => run_postgres_migrations(url, &hook).await,
    }
}

async fn run_libsql_migrations(path: &Path, hook: &MigrationTestHook) -> anyhow::Result<()> {
    let db = libsql::Builder::new_local(path)
        .build()
        .await
        .with_context(|| format!("failed opening local libsql database `{}`", path.display()))?;
    let conn = db.connect().context("failed opening libsql connection")?;

    conn.execute(
        r#"
        CREATE TABLE IF NOT EXISTS refinery_schema_history (
            version INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            applied_on INTEGER NOT NULL,
            checksum TEXT NOT NULL
        )
        "#,
        (),
    )
    .await
    .context("failed ensuring refinery schema history table")?;

    let applied_versions = load_libsql_applied_versions(&conn).await?;

    for migration in EMBEDDED_MIGRATIONS {
        if applied_versions.contains(&migration.version) {
            continue;
        }

        let tx = conn
            .transaction()
            .await
            .with_context(|| format!("failed starting transaction for migration {}", migration.version))?;

        let migration_result = async {
            tx.execute_batch(migration.libsql_sql)
                .await
                .with_context(|| format!("failed applying migration {}", migration.version))?;
            hook.maybe_fail(migration.version)?;
            tx.execute(
                r#"
                INSERT INTO refinery_schema_history (version, name, applied_on, checksum)
                VALUES (?1, ?2, unixepoch(), ?3)
                "#,
                libsql::params![
                    migration.version as i64,
                    migration.name,
                    migration.checksum
                ],
            )
            .await
            .with_context(|| {
                format!(
                    "failed recording migration {} in refinery_schema_history",
                    migration.version
                )
            })?;
            Ok::<(), anyhow::Error>(())
        }
        .await;

        match migration_result {
            Ok(()) => tx
                .commit()
                .await
                .with_context(|| format!("failed committing migration {}", migration.version))?,
            Err(error) => {
                let rollback_error = tx.rollback().await.err();
                if let Some(rollback_error) = rollback_error {
                    tracing::warn!(
                        migration_version = migration.version,
                        error = %rollback_error,
                        "failed rolling back libsql migration transaction"
                    );
                }
                return Err(error);
            }
        }
    }

    Ok(())
}

async fn load_libsql_applied_versions(
    conn: &libsql::Connection,
) -> anyhow::Result<HashSet<u32>> {
    let mut applied_rows = conn
        .query("SELECT version FROM refinery_schema_history", ())
        .await
        .context("failed reading applied migration versions")?;

    let mut applied_versions = HashSet::new();
    while let Some(row) = applied_rows
        .next()
        .await
        .context("failed iterating applied migration versions")?
    {
        let version: i64 = row
            .get(0)
            .map_err(|error| anyhow::anyhow!("failed decoding migration version: {error}"))?;
        applied_versions.insert(version as u32);
    }

    Ok(applied_versions)
}

async fn run_postgres_migrations(url: &str, hook: &MigrationTestHook) -> anyhow::Result<()> {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(url)
        .await
        .context("failed opening postgres connection pool for migrations")?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS refinery_schema_history (
            version BIGINT PRIMARY KEY,
            name TEXT NOT NULL,
            applied_on BIGINT NOT NULL,
            checksum TEXT NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await
    .context("failed ensuring refinery schema history table")?;

    let rows = sqlx::query("SELECT version FROM refinery_schema_history")
        .fetch_all(&pool)
        .await
        .context("failed reading applied postgres migration versions")?;
    let applied_versions = rows
        .iter()
        .map(|row| row.try_get::<i64, _>(0).map(|value| value as u32))
        .collect::<Result<HashSet<_>, _>>()
        .context("failed decoding applied postgres migration versions")?;

    for migration in EMBEDDED_MIGRATIONS {
        if applied_versions.contains(&migration.version) {
            continue;
        }

        let mut tx = pool
            .begin()
            .await
            .with_context(|| format!("failed starting postgres transaction for migration {}", migration.version))?;

        let migration_result = async {
            sqlx::raw_sql(migration.postgres_sql)
                .execute(&mut *tx)
                .await
                .with_context(|| format!("failed applying postgres migration {}", migration.version))?;
            hook.maybe_fail(migration.version)?;
            sqlx::query(
                r#"
                INSERT INTO refinery_schema_history (version, name, applied_on, checksum)
                VALUES ($1, $2, $3, $4)
                "#,
            )
            .bind(migration.version as i64)
            .bind(migration.name)
            .bind(OffsetDateTime::now_utc().unix_timestamp())
            .bind(migration.checksum)
            .execute(&mut *tx)
            .await
            .with_context(|| {
                format!(
                    "failed recording postgres migration {} in refinery_schema_history",
                    migration.version
                )
            })?;
            Ok::<(), anyhow::Error>(())
        }
        .await;

        match migration_result {
            Ok(()) => tx
                .commit()
                .await
                .with_context(|| format!("failed committing postgres migration {}", migration.version))?,
            Err(error) => {
                let rollback_error = tx.rollback().await.err();
                if let Some(rollback_error) = rollback_error {
                    tracing::warn!(
                        migration_version = migration.version,
                        error = %rollback_error,
                        "failed rolling back postgres migration transaction"
                    );
                }
                return Err(error);
            }
        }
    }

    Ok(())
}
