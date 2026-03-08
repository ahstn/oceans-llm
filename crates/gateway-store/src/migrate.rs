use std::{collections::HashSet, path::Path};

use anyhow::Context;

struct EmbeddedMigration {
    version: u32,
    name: &'static str,
    checksum: &'static str,
    sql: &'static str,
}

const EMBEDDED_MIGRATIONS: &[EmbeddedMigration] = &[
    EmbeddedMigration {
        version: 1,
        name: "init",
        checksum: "V1__init.sql",
        sql: include_str!("../migrations/V1__init.sql"),
    },
    EmbeddedMigration {
        version: 2,
        name: "audit_baseline",
        checksum: "V2__audit_baseline.sql",
        sql: include_str!("../migrations/V2__audit_baseline.sql"),
    },
    EmbeddedMigration {
        version: 3,
        name: "identity_foundation",
        checksum: "V3__identity_foundation.sql",
        sql: include_str!("../migrations/V3__identity_foundation.sql"),
    },
    EmbeddedMigration {
        version: 4,
        name: "money_fixed_point",
        checksum: "V4__money_fixed_point.sql",
        sql: include_str!("../migrations/V4__money_fixed_point.sql"),
    },
    EmbeddedMigration {
        version: 5,
        name: "pricing_catalog_cache",
        checksum: "V5__pricing_catalog_cache.sql",
        sql: include_str!("../migrations/V5__pricing_catalog_cache.sql"),
    },
    EmbeddedMigration {
        version: 6,
        name: "identity_onboarding",
        checksum: "V6__identity_onboarding.sql",
        sql: include_str!("../migrations/V6__identity_onboarding.sql"),
    },
    EmbeddedMigration {
        version: 7,
        name: "user_password_rotation",
        checksum: "V7__user_password_rotation.sql",
        sql: include_str!("../migrations/V7__user_password_rotation.sql"),
    },
    EmbeddedMigration {
        version: 8,
        name: "request_log_payloads",
        checksum: "V8__request_log_payloads.sql",
        sql: include_str!("../migrations/V8__request_log_payloads.sql"),
    },
];

pub async fn run_migrations(path: impl AsRef<Path>) -> anyhow::Result<()> {
    let path = path.as_ref();
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

    for migration in EMBEDDED_MIGRATIONS {
        if applied_versions.contains(&migration.version) {
            continue;
        }

        conn.execute_batch(migration.sql)
            .await
            .with_context(|| format!("failed applying migration {}", migration.version))?;

        conn.execute(
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
    }

    Ok(())
}
