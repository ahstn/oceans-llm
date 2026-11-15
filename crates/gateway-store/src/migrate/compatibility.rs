use std::collections::{HashMap, HashSet};

use anyhow::{Context, bail};
use sqlx::Row;

use super::{AppliedMigrationRecord, MIGRATION_REGISTRY};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewerAppliedMigration {
    pub version: u32,
    pub name: String,
    pub min_reader_version: u32,
}

pub(super) fn registry_head() -> u32 {
    MIGRATION_REGISTRY
        .last()
        .expect("nonempty migration registry")
        .version
}

pub(super) fn validate_newer_migrations(
    backend: &str,
    history: &[AppliedMigrationRecord],
    compatibility: &HashMap<u32, u32>,
) -> anyhow::Result<Vec<NewerAppliedMigration>> {
    let head = registry_head();
    let newer: Vec<_> = history
        .iter()
        .filter(|entry| entry.version > head)
        .collect();
    let missing: Vec<_> = newer
        .iter()
        .filter(|entry| !compatibility.contains_key(&entry.version))
        .map(|entry| entry.version.to_string())
        .collect();
    let required = newer
        .iter()
        .filter_map(|entry| compatibility.get(&entry.version))
        .copied()
        .max()
        .unwrap_or(0);
    if !missing.is_empty() || required > head {
        let versions = newer
            .iter()
            .map(|entry| format!("v{} (`{}`)", entry.version, entry.name))
            .collect::<Vec<_>>()
            .join(", ");
        let requirement = if missing.is_empty() {
            format!("which need a gateway that knows migration v{required} or later")
        } else {
            format!(
                "without recording which older gateways can read them (missing for v{})",
                missing.join(", v")
            )
        };
        bail!(
            "database schema is newer than this gateway ({backend}): a newer release applied {versions}, {requirement}; this gateway knows migrations up to v{head}. Deploy the newer release again, or restore the database backup taken before the upgrade"
        );
    }
    Ok(newer
        .into_iter()
        .map(|entry| NewerAppliedMigration {
            version: entry.version,
            name: entry.name.clone(),
            min_reader_version: compatibility[&entry.version],
        })
        .collect())
}

pub(super) fn ensure_no_pending_with_newer(
    backend: &str,
    applied: &HashSet<u32>,
    newer: &[NewerAppliedMigration],
) -> anyhow::Result<()> {
    if !newer.is_empty() {
        let pending: Vec<_> = MIGRATION_REGISTRY
            .iter()
            .filter(|entry| !applied.contains(&entry.version))
            .map(|entry| entry.version.to_string())
            .collect();
        if !pending.is_empty() {
            bail!(
                "cannot apply pending migrations v{} ({backend}): a newer release already applied later migrations, and this gateway knows migrations up to v{}. Deploy the newer release again, or restore the database backup taken before the upgrade",
                pending.join(", v"),
                registry_head()
            );
        }
    }
    Ok(())
}

pub(super) async fn load_libsql_compatibility(
    conn: &libsql::Connection,
) -> anyhow::Result<HashMap<u32, u32>> {
    let mut tables = conn
        .query(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'gateway_migration_compatibility'",
            (),
        )
        .await
        .context("failed checking libsql migration compatibility table")?;
    let table = tables
        .next()
        .await?
        .context("missing compatibility table count")?;
    if table.get::<i64>(0)? == 0 {
        return Ok(HashMap::new());
    }
    let mut rows = conn
        .query(
            "SELECT version, min_reader_version FROM gateway_migration_compatibility",
            (),
        )
        .await
        .context("failed reading libsql migration compatibility")?;
    let mut compatibility = HashMap::new();
    while let Some(row) = rows.next().await? {
        compatibility.insert(
            u32::try_from(row.get::<i64>(0)?)?,
            u32::try_from(row.get::<i64>(1)?)?,
        );
    }
    Ok(compatibility)
}

pub(super) async fn load_postgres_compatibility(
    pool: &sqlx::PgPool,
) -> anyhow::Result<HashMap<u32, u32>> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM information_schema.tables WHERE table_schema = 'public' AND table_name = 'gateway_migration_compatibility')",
    )
    .fetch_one(pool)
    .await
    .context("failed checking postgres migration compatibility table")?;
    if !exists {
        return Ok(HashMap::new());
    }
    let rows =
        sqlx::query("SELECT version, min_reader_version FROM gateway_migration_compatibility")
            .fetch_all(pool)
            .await
            .context("failed reading postgres migration compatibility")?;
    rows.into_iter()
        .map(|row| {
            Ok((
                u32::try_from(row.try_get::<i64, _>(0)?)?,
                u32::try_from(row.try_get::<i64, _>(1)?)?,
            ))
        })
        .collect()
}
