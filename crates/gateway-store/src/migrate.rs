use std::{collections::HashSet, path::Path};

use anyhow::{Context, bail};
use sqlx::Row;
use time::OffsetDateTime;

use crate::{
    StoreConnectionOptions,
    migration_registry::{BackendMigrationStep, MIGRATION_REGISTRY, MigrationBackend},
};

#[derive(Debug, Clone)]
pub struct MigrationStatusEntry {
    pub version: u32,
    pub name: &'static str,
    pub checksum: &'static str,
    pub applied: bool,
    pub backend_note: Option<&'static str>,
}

#[derive(Debug, Clone)]
pub struct MigrationStatus {
    pub backend: &'static str,
    pub entries: Vec<MigrationStatusEntry>,
}

impl MigrationStatus {
    #[must_use]
    pub fn pending_count(&self) -> usize {
        self.entries.iter().filter(|entry| !entry.applied).count()
    }
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

pub async fn status_migrations_with_options(
    options: &StoreConnectionOptions,
) -> anyhow::Result<MigrationStatus> {
    let (backend, applied_versions) = match options {
        StoreConnectionOptions::Libsql { path } => (
            "libsql",
            load_libsql_applied_versions(path).await.with_context(|| {
                format!("failed loading migration status for `{}`", path.display())
            })?,
        ),
        StoreConnectionOptions::Postgres { url, .. } => (
            "postgres",
            load_postgres_applied_versions(url)
                .await
                .context("failed loading postgres migration status")?,
        ),
    };

    let entries = MIGRATION_REGISTRY
        .iter()
        .map(|migration| {
            let step = migration.step_for(match backend {
                "libsql" => MigrationBackend::Libsql,
                _ => MigrationBackend::Postgres,
            });
            MigrationStatusEntry {
                version: migration.version,
                name: migration.name,
                checksum: migration.checksum,
                applied: applied_versions.contains(&migration.version),
                backend_note: match step {
                    BackendMigrationStep::Sql(_) => None,
                    BackendMigrationStep::Compatibility { reason } => Some(reason),
                },
            }
        })
        .collect();

    Ok(MigrationStatus { backend, entries })
}

pub async fn check_migrations_with_options(
    options: &StoreConnectionOptions,
) -> anyhow::Result<MigrationStatus> {
    let status = status_migrations_with_options(options).await?;
    if status.pending_count() > 0 {
        bail!(
            "{} pending migrations remain for {}",
            status.pending_count(),
            status.backend
        );
    }
    Ok(status)
}

async fn run_libsql_migrations(path: &Path, hook: &MigrationTestHook) -> anyhow::Result<()> {
    let db = libsql::Builder::new_local(path)
        .build()
        .await
        .with_context(|| format!("failed opening local libsql database `{}`", path.display()))?;
    let conn = db.connect().context("failed opening libsql connection")?;

    ensure_libsql_history_table(&conn).await?;
    let applied_versions = load_libsql_versions_from_connection(&conn).await?;

    for migration in MIGRATION_REGISTRY {
        if applied_versions.contains(&migration.version) {
            continue;
        }

        let tx = conn.transaction().await.with_context(|| {
            format!(
                "failed starting transaction for migration {}",
                migration.version
            )
        })?;

        let migration_result = async {
            if let BackendMigrationStep::Sql(sql) = migration.step_for(MigrationBackend::Libsql) {
                tx.execute_batch(sql)
                    .await
                    .with_context(|| format!("failed applying migration {}", migration.version))?;
            }
            hook.maybe_fail(migration.version)?;
            tx.execute(
                r#"
                INSERT INTO refinery_schema_history (version, name, applied_on, checksum)
                VALUES (?1, ?2, unixepoch(), ?3)
                "#,
                libsql::params![migration.version as i64, migration.name, migration.checksum],
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

async fn run_postgres_migrations(url: &str, hook: &MigrationTestHook) -> anyhow::Result<()> {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(url)
        .await
        .context("failed opening postgres connection pool for migrations")?;

    ensure_postgres_history_table(&pool).await?;
    let applied_versions = load_postgres_versions_from_pool(&pool).await?;

    for migration in MIGRATION_REGISTRY {
        if applied_versions.contains(&migration.version) {
            continue;
        }

        let mut tx = pool.begin().await.with_context(|| {
            format!(
                "failed starting postgres transaction for migration {}",
                migration.version
            )
        })?;

        let migration_result = async {
            if let BackendMigrationStep::Sql(sql) = migration.step_for(MigrationBackend::Postgres) {
                sqlx::raw_sql(sql)
                    .execute(&mut *tx)
                    .await
                    .with_context(|| {
                        format!("failed applying postgres migration {}", migration.version)
                    })?;
            }
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
            Ok(()) => tx.commit().await.with_context(|| {
                format!("failed committing postgres migration {}", migration.version)
            })?,
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

async fn ensure_libsql_history_table(conn: &libsql::Connection) -> anyhow::Result<()> {
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
    Ok(())
}

async fn ensure_postgres_history_table(pool: &sqlx::PgPool) -> anyhow::Result<()> {
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
    .execute(pool)
    .await
    .context("failed ensuring refinery schema history table")?;
    Ok(())
}

async fn load_libsql_applied_versions(path: &Path) -> anyhow::Result<HashSet<u32>> {
    let db = libsql::Builder::new_local(path)
        .build()
        .await
        .with_context(|| format!("failed opening local libsql database `{}`", path.display()))?;
    let conn = db.connect().context("failed opening libsql connection")?;

    if !libsql_history_table_exists(&conn).await? {
        return Ok(HashSet::new());
    }

    load_libsql_versions_from_connection(&conn).await
}

async fn load_libsql_versions_from_connection(
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

async fn libsql_history_table_exists(conn: &libsql::Connection) -> anyhow::Result<bool> {
    let mut rows = conn
        .query(
            r#"
            SELECT COUNT(*)
            FROM sqlite_master
            WHERE type = 'table'
              AND name = 'refinery_schema_history'
            "#,
            (),
        )
        .await
        .context("failed checking libsql migration history table")?;

    let row = rows
        .next()
        .await
        .context("failed loading libsql migration history table row")?
        .ok_or_else(|| anyhow::anyhow!("missing libsql migration history table row"))?;
    let count: i64 = row.get(0).map_err(|error| {
        anyhow::anyhow!("failed decoding libsql migration history count: {error}")
    })?;
    Ok(count > 0)
}

async fn load_postgres_applied_versions(url: &str) -> anyhow::Result<HashSet<u32>> {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(url)
        .await
        .context("failed opening postgres connection pool for migration status")?;

    if !postgres_history_table_exists(&pool).await? {
        pool.close().await;
        return Ok(HashSet::new());
    }

    let versions = load_postgres_versions_from_pool(&pool).await;
    pool.close().await;
    versions
}

async fn load_postgres_versions_from_pool(pool: &sqlx::PgPool) -> anyhow::Result<HashSet<u32>> {
    let rows = sqlx::query("SELECT version FROM refinery_schema_history")
        .fetch_all(pool)
        .await
        .context("failed reading applied postgres migration versions")?;
    rows.iter()
        .map(|row| row.try_get::<i64, _>(0).map(|value| value as u32))
        .collect::<Result<HashSet<_>, _>>()
        .context("failed decoding applied postgres migration versions")
}

async fn postgres_history_table_exists(pool: &sqlx::PgPool) -> anyhow::Result<bool> {
    let exists: bool = sqlx::query_scalar(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM information_schema.tables
            WHERE table_schema = 'public'
              AND table_name = 'refinery_schema_history'
        )
        "#,
    )
    .fetch_one(pool)
        .await
        .context("failed checking postgres migration history table")?;
    Ok(exists)
}
