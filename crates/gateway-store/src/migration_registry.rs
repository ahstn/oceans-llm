#[derive(Debug, Clone, Copy)]
pub(crate) struct MigrationManifest {
    pub version: u32,
    pub name: &'static str,
    pub checksum: &'static str,
    pub libsql_sql: &'static str,
    pub postgres_sql: &'static str,
}

impl MigrationManifest {
    pub(crate) fn sql_for(&self, backend: MigrationBackend) -> &'static str {
        match backend {
            MigrationBackend::Libsql => self.libsql_sql,
            MigrationBackend::Postgres => self.postgres_sql,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MigrationBackend {
    Libsql,
    Postgres,
}

pub(crate) const MIGRATION_REGISTRY: &[MigrationManifest] = &[MigrationManifest {
    version: 17,
    name: "baseline",
    checksum: "V17__baseline.sql",
    libsql_sql: include_str!("../migrations/V17__baseline.sql"),
    postgres_sql: include_str!("../migrations/postgres/V17__baseline.sql"),
}];

#[cfg(test)]
mod tests {
    use super::{MIGRATION_REGISTRY, MigrationManifest};

    #[test]
    fn migration_registry_versions_are_unique_and_sorted() {
        let mut previous_version = 0;
        for migration in MIGRATION_REGISTRY {
            assert!(migration.version > previous_version);
            previous_version = migration.version;
        }
    }

    #[test]
    fn migration_registry_names_are_non_empty() {
        for MigrationManifest { name, checksum, .. } in MIGRATION_REGISTRY {
            assert!(!name.is_empty());
            assert!(!checksum.is_empty());
        }
    }
}
