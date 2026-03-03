use std::{collections::HashSet, path::Path};

use anyhow::Context;

mod embedded {
    use refinery::embed_migrations;

    embed_migrations!("migrations");
}

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

    for migration in embedded::migrations::runner().get_migrations() {
        if applied_versions.contains(&migration.version()) {
            continue;
        }

        let sql = migration.sql().ok_or_else(|| {
            anyhow::anyhow!("embedded migration {} has no SQL body", migration.version())
        })?;

        conn.execute_batch(sql)
            .await
            .with_context(|| format!("failed applying migration {}", migration.version()))?;

        conn.execute(
            r#"
            INSERT INTO refinery_schema_history (version, name, applied_on, checksum)
            VALUES (?1, ?2, unixepoch(), ?3)
            "#,
            libsql::params![
                migration.version() as i64,
                migration.name(),
                migration.checksum().to_string()
            ],
        )
        .await
        .with_context(|| {
            format!(
                "failed recording migration {} in refinery_schema_history",
                migration.version()
            )
        })?;
    }

    Ok(())
}
