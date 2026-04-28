use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

use anyhow::{Context, bail};
use sqlx::Row;
use time::OffsetDateTime;

use crate::{
    StoreConnectionOptions,
    migration_registry::{MIGRATION_REGISTRY, MigrationBackend},
};

#[derive(Debug, Clone)]
pub struct MigrationStatusEntry {
    pub version: u32,
    pub name: &'static str,
    pub checksum: &'static str,
    pub applied: bool,
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct AppliedMigrationRecord {
    version: u32,
    name: String,
    checksum: String,
}

#[derive(Debug, Clone)]
struct LoadedMigrationState {
    applied_history: Vec<AppliedMigrationRecord>,
    has_existing_app_tables: bool,
}

const ACTIVE_APPLICATION_TABLES: &[&str] = &[
    "providers",
    "gateway_models",
    "teams",
    "users",
    "team_memberships",
    "oidc_providers",
    "user_password_auth",
    "user_oidc_auth",
    "user_oauth_auth",
    "user_model_allowlist",
    "team_model_allowlist",
    "api_keys",
    "api_key_model_grants",
    "audit_logs",
    "model_routes",
    "pricing_catalog_cache",
    "password_invitations",
    "user_oidc_links",
    "user_sessions",
    "model_pricing",
    "request_logs",
    "request_log_payloads",
    "request_log_tags",
    "request_log_attempts",
    "user_budgets",
    "team_budgets",
    "budget_alerts",
    "budget_alert_deliveries",
    "usage_cost_event_duplicates_archive",
    "usage_cost_events",
];

#[derive(Debug, Clone, Default)]
pub(crate) struct MigrationTestHook {
    #[cfg(test)]
    pub fail_after_apply_version: Option<u32>,
    #[cfg(test)]
    pub fail_history_insert_version: Option<u32>,
}

impl MigrationTestHook {
    #[cfg(test)]
    fn maybe_fail_after_apply(&self, version: u32) -> anyhow::Result<()> {
        if self.fail_after_apply_version == Some(version) {
            anyhow::bail!("forced migration failure after applying version {version}");
        }
        Ok(())
    }

    #[cfg(not(test))]
    fn maybe_fail_after_apply(&self, _version: u32) -> anyhow::Result<()> {
        Ok(())
    }

    #[cfg(test)]
    fn should_fail_history_insert(&self, version: u32) -> bool {
        self.fail_history_insert_version == Some(version)
    }

    #[cfg(not(test))]
    fn should_fail_history_insert(&self, _version: u32) -> bool {
        false
    }
}

pub async fn run_migrations(path: impl AsRef<Path>) -> anyhow::Result<()> {
    run_migrations_with_options(&StoreConnectionOptions::Libsql {
        path: path.as_ref().to_path_buf(),
    })
    .await
}

pub async fn run_migrations_with_options(options: &StoreConnectionOptions) -> anyhow::Result<()> {
    run_migrations_with_hook(options, MigrationTestHook::default()).await
}

#[cfg(test)]
pub(crate) async fn run_migrations_with_options_for_test(
    options: &StoreConnectionOptions,
    hook: MigrationTestHook,
) -> anyhow::Result<()> {
    run_migrations_with_hook(options, hook).await
}

async fn run_migrations_with_hook(
    options: &StoreConnectionOptions,
    hook: MigrationTestHook,
) -> anyhow::Result<()> {
    match options {
        StoreConnectionOptions::Libsql { path } => run_libsql_migrations(path, hook).await,
        StoreConnectionOptions::Postgres { url, .. } => run_postgres_migrations(url, hook).await,
    }
}

pub async fn status_migrations_with_options(
    options: &StoreConnectionOptions,
) -> anyhow::Result<MigrationStatus> {
    let (backend, migration_state) = match options {
        StoreConnectionOptions::Libsql { path } => (
            "libsql",
            load_libsql_migration_state(path).await.with_context(|| {
                format!("failed loading migration status for `{}`", path.display())
            })?,
        ),
        StoreConnectionOptions::Postgres { url, .. } => (
            "postgres",
            load_postgres_migration_state(url)
                .await
                .context("failed loading postgres migration status")?,
        ),
    };

    validate_migration_history(
        backend,
        &migration_state.applied_history,
        migration_state.has_existing_app_tables,
    )?;
    let applied_versions = applied_versions(&migration_state.applied_history);

    let entries = MIGRATION_REGISTRY
        .iter()
        .map(|migration| MigrationStatusEntry {
            version: migration.version,
            name: migration.name,
            checksum: migration.checksum,
            applied: applied_versions.contains(&migration.version),
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

async fn run_libsql_migrations(path: &Path, hook: MigrationTestHook) -> anyhow::Result<()> {
    let db = libsql::Builder::new_local(path)
        .build()
        .await
        .with_context(|| format!("failed opening local libsql database `{}`", path.display()))?;
    let conn = db.connect().context("failed opening libsql connection")?;

    ensure_libsql_history_table(&conn).await?;
    let applied_history = load_libsql_history_from_connection(&conn).await?;
    let has_existing_app_tables = libsql_has_existing_application_tables(&conn).await?;
    validate_migration_history("libsql", &applied_history, has_existing_app_tables)?;
    let applied_versions = applied_versions(&applied_history);

    for migration in MIGRATION_REGISTRY {
        if applied_versions.contains(&migration.version) {
            continue;
        }

        tracing::info!(
            backend = "libsql",
            migration_version = migration.version,
            migration_name = migration.name,
            migration_event = "begin",
            "starting migration transaction"
        );

        let tx = conn.transaction().await.with_context(|| {
            format!(
                "failed starting transaction for migration {}",
                migration.version
            )
        })?;

        let migration_result = async {
            tracing::debug!(
                backend = "libsql",
                migration_version = migration.version,
                migration_name = migration.name,
                migration_event = "apply",
                "applying migration SQL"
            );
            tx.execute_batch(migration.sql_for(MigrationBackend::Libsql))
                .await
                .with_context(|| format!("failed applying migration {}", migration.version))?;
            hook.maybe_fail_after_apply(migration.version)?;

            tracing::debug!(
                backend = "libsql",
                migration_version = migration.version,
                migration_name = migration.name,
                migration_event = "history_insert",
                "recording migration in schema history"
            );

            if hook.should_fail_history_insert(migration.version) {
                tx.execute(
                    r#"
                    INSERT INTO refinery_schema_history_missing (version, name, applied_on, checksum)
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
            } else {
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
            }
            Ok::<(), anyhow::Error>(())
        }
        .await;

        match migration_result {
            Ok(()) => {
                tx.commit().await.with_context(|| {
                    format!("failed committing migration {}", migration.version)
                })?;
                tracing::info!(
                    backend = "libsql",
                    migration_version = migration.version,
                    migration_name = migration.name,
                    migration_event = "commit",
                    "migration transaction committed"
                );
            }
            Err(error) => {
                tracing::warn!(
                    backend = "libsql",
                    migration_version = migration.version,
                    migration_name = migration.name,
                    migration_event = "rollback",
                    error = %error,
                    "migration failed, rolling back transaction"
                );
                let rollback_error = tx.rollback().await.err();
                if let Some(rollback_error) = rollback_error {
                    tracing::warn!(
                        backend = "libsql",
                        migration_version = migration.version,
                        migration_name = migration.name,
                        migration_event = "rollback",
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

async fn run_postgres_migrations(url: &str, hook: MigrationTestHook) -> anyhow::Result<()> {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(url)
        .await
        .context("failed opening postgres connection pool for migrations")?;

    ensure_postgres_history_table(&pool).await?;
    let applied_history = load_postgres_history_from_pool(&pool).await?;
    let has_existing_app_tables = postgres_has_existing_application_tables(&pool).await?;
    validate_migration_history("postgres", &applied_history, has_existing_app_tables)?;
    let applied_versions = applied_versions(&applied_history);

    for migration in MIGRATION_REGISTRY {
        if applied_versions.contains(&migration.version) {
            continue;
        }

        tracing::info!(
            backend = "postgres",
            migration_version = migration.version,
            migration_name = migration.name,
            migration_event = "begin",
            "starting migration transaction"
        );

        let mut tx = pool.begin().await.with_context(|| {
            format!(
                "failed starting postgres transaction for migration {}",
                migration.version
            )
        })?;

        let migration_result = async {
            tracing::debug!(
                backend = "postgres",
                migration_version = migration.version,
                migration_name = migration.name,
                migration_event = "apply",
                "applying migration SQL"
            );
            sqlx::raw_sql(migration.sql_for(MigrationBackend::Postgres))
                .execute(&mut *tx)
                .await
                .with_context(|| {
                    format!("failed applying postgres migration {}", migration.version)
                })?;
            hook.maybe_fail_after_apply(migration.version)?;

            tracing::debug!(
                backend = "postgres",
                migration_version = migration.version,
                migration_name = migration.name,
                migration_event = "history_insert",
                "recording migration in schema history"
            );

            if hook.should_fail_history_insert(migration.version) {
                sqlx::query(
                    r#"
                    INSERT INTO refinery_schema_history_missing (version, name, applied_on, checksum)
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
            } else {
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
            }
            Ok::<(), anyhow::Error>(())
        }
        .await;

        match migration_result {
            Ok(()) => {
                tx.commit().await.with_context(|| {
                    format!("failed committing postgres migration {}", migration.version)
                })?;
                tracing::info!(
                    backend = "postgres",
                    migration_version = migration.version,
                    migration_name = migration.name,
                    migration_event = "commit",
                    "migration transaction committed"
                );
            }
            Err(error) => {
                tracing::warn!(
                    backend = "postgres",
                    migration_version = migration.version,
                    migration_name = migration.name,
                    migration_event = "rollback",
                    error = %error,
                    "migration failed, rolling back transaction"
                );
                let rollback_error = tx.rollback().await.err();
                if let Some(rollback_error) = rollback_error {
                    tracing::warn!(
                        backend = "postgres",
                        migration_version = migration.version,
                        migration_name = migration.name,
                        migration_event = "rollback",
                        error = %rollback_error,
                        "failed rolling back postgres migration transaction"
                    );
                }
                return Err(error);
            }
        }
    }

    pool.close().await;
    Ok(())
}

fn validate_migration_history(
    backend: &'static str,
    applied_history: &[AppliedMigrationRecord],
    has_existing_app_tables: bool,
) -> anyhow::Result<()> {
    if applied_history.is_empty() && has_existing_app_tables {
        bail!(
            "database reset required for {backend}: refinery_schema_history is empty but existing application tables were found; recreate the database and rerun migrations"
        );
    }

    let registry_by_version: HashMap<u32, _> = MIGRATION_REGISTRY
        .iter()
        .map(|manifest| (manifest.version, manifest))
        .collect();

    for applied in applied_history {
        let Some(expected) = registry_by_version.get(&applied.version) else {
            bail!(
                "database reset required for {backend}: found historical migration v{} outside the active registry; recreate the database and rerun migrations",
                applied.version
            );
        };

        if applied.name != expected.name || applied.checksum != expected.checksum {
            bail!(
                "database reset required for {backend}: migration v{} identity mismatch (expected name=`{}` checksum=`{}`, found name=`{}` checksum=`{}`); recreate the database and rerun migrations",
                applied.version,
                expected.name,
                expected.checksum,
                applied.name,
                applied.checksum
            );
        }
    }

    Ok(())
}

fn applied_versions(applied_history: &[AppliedMigrationRecord]) -> HashSet<u32> {
    applied_history.iter().map(|entry| entry.version).collect()
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

async fn load_libsql_migration_state(path: &Path) -> anyhow::Result<LoadedMigrationState> {
    let db = libsql::Builder::new_local(path)
        .build()
        .await
        .with_context(|| format!("failed opening local libsql database `{}`", path.display()))?;
    let conn = db.connect().context("failed opening libsql connection")?;

    let applied_history = if libsql_history_table_exists(&conn).await? {
        load_libsql_history_from_connection(&conn).await?
    } else {
        Vec::new()
    };
    let has_existing_app_tables = libsql_has_existing_application_tables(&conn).await?;

    Ok(LoadedMigrationState {
        applied_history,
        has_existing_app_tables,
    })
}

async fn load_libsql_history_from_connection(
    conn: &libsql::Connection,
) -> anyhow::Result<Vec<AppliedMigrationRecord>> {
    let mut rows = conn
        .query(
            "SELECT version, name, checksum FROM refinery_schema_history ORDER BY version",
            (),
        )
        .await
        .context("failed reading applied migration history")?;

    let mut applied_history = Vec::new();
    while let Some(row) = rows
        .next()
        .await
        .context("failed iterating applied migration history")?
    {
        let version: i64 = row
            .get(0)
            .map_err(|error| anyhow::anyhow!("failed decoding migration version: {error}"))?;
        let name: String = row
            .get(1)
            .map_err(|error| anyhow::anyhow!("failed decoding migration name: {error}"))?;
        let checksum: String = row
            .get(2)
            .map_err(|error| anyhow::anyhow!("failed decoding migration checksum: {error}"))?;
        applied_history.push(AppliedMigrationRecord {
            version: version as u32,
            name,
            checksum,
        });
    }

    Ok(applied_history)
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

async fn libsql_has_existing_application_tables(conn: &libsql::Connection) -> anyhow::Result<bool> {
    let mut rows = conn
        .query(
            r#"
            SELECT name
            FROM sqlite_master
            WHERE type = 'table'
            "#,
            (),
        )
        .await
        .context("failed checking libsql application tables")?;

    while let Some(row) = rows
        .next()
        .await
        .context("failed iterating libsql application tables")?
    {
        let table_name: String = row
            .get(0)
            .map_err(|error| anyhow::anyhow!("failed decoding libsql table name: {error}"))?;
        if ACTIVE_APPLICATION_TABLES.contains(&table_name.as_str()) {
            return Ok(true);
        }
    }

    Ok(false)
}

async fn load_postgres_migration_state(url: &str) -> anyhow::Result<LoadedMigrationState> {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(url)
        .await
        .context("failed opening postgres connection pool for migration status")?;

    let applied_history = if postgres_history_table_exists(&pool).await? {
        load_postgres_history_from_pool(&pool).await?
    } else {
        Vec::new()
    };
    let has_existing_app_tables = postgres_has_existing_application_tables(&pool).await?;

    pool.close().await;
    Ok(LoadedMigrationState {
        applied_history,
        has_existing_app_tables,
    })
}

async fn load_postgres_history_from_pool(
    pool: &sqlx::PgPool,
) -> anyhow::Result<Vec<AppliedMigrationRecord>> {
    let rows =
        sqlx::query("SELECT version, name, checksum FROM refinery_schema_history ORDER BY version")
            .fetch_all(pool)
            .await
            .context("failed reading applied postgres migration history")?;

    rows.into_iter()
        .map(|row| {
            Ok(AppliedMigrationRecord {
                version: row
                    .try_get::<i64, _>(0)
                    .context("failed decoding applied postgres migration version")?
                    as u32,
                name: row
                    .try_get::<String, _>(1)
                    .context("failed decoding applied postgres migration name")?,
                checksum: row
                    .try_get::<String, _>(2)
                    .context("failed decoding applied postgres migration checksum")?,
            })
        })
        .collect()
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

async fn postgres_has_existing_application_tables(pool: &sqlx::PgPool) -> anyhow::Result<bool> {
    let rows = sqlx::query_scalar::<_, String>(
        r#"
        SELECT table_name
        FROM information_schema.tables
        WHERE table_schema = 'public'
        "#,
    )
    .fetch_all(pool)
    .await
    .context("failed checking postgres application tables")?;

    Ok(rows
        .iter()
        .any(|table_name| ACTIVE_APPLICATION_TABLES.contains(&table_name.as_str())))
}
