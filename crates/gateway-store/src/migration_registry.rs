#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BackendMigrationStep {
    Sql(&'static str),
    Compatibility { reason: &'static str },
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct MigrationManifest {
    pub version: u32,
    pub name: &'static str,
    pub checksum: &'static str,
    pub libsql: BackendMigrationStep,
    pub postgres: BackendMigrationStep,
}

impl MigrationManifest {
    pub(crate) fn step_for(&self, backend: MigrationBackend) -> BackendMigrationStep {
        match backend {
            MigrationBackend::Libsql => self.libsql,
            MigrationBackend::Postgres => self.postgres,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MigrationBackend {
    Libsql,
    Postgres,
}

pub(crate) const MIGRATION_REGISTRY: &[MigrationManifest] = &[
    MigrationManifest {
        version: 1,
        name: "init",
        checksum: "V1__init.sql",
        libsql: BackendMigrationStep::Sql(include_str!("../migrations/V1__init.sql")),
        postgres: BackendMigrationStep::Sql(include_str!("../migrations/postgres/V1__init.sql")),
    },
    MigrationManifest {
        version: 2,
        name: "audit_baseline",
        checksum: "V2__audit_baseline.sql",
        libsql: BackendMigrationStep::Sql(include_str!("../migrations/V2__audit_baseline.sql")),
        postgres: BackendMigrationStep::Compatibility {
            reason: "PostgreSQL V1 already includes the audit baseline schema.",
        },
    },
    MigrationManifest {
        version: 3,
        name: "identity_foundation",
        checksum: "V3__identity_foundation.sql",
        libsql: BackendMigrationStep::Sql(include_str!(
            "../migrations/V3__identity_foundation.sql"
        )),
        postgres: BackendMigrationStep::Compatibility {
            reason: "PostgreSQL V1 already includes the identity foundation schema.",
        },
    },
    MigrationManifest {
        version: 4,
        name: "money_fixed_point",
        checksum: "V4__money_fixed_point.sql",
        libsql: BackendMigrationStep::Sql(include_str!("../migrations/V4__money_fixed_point.sql")),
        postgres: BackendMigrationStep::Compatibility {
            reason: "PostgreSQL V1 starts on the fixed-point money schema.",
        },
    },
    MigrationManifest {
        version: 5,
        name: "pricing_catalog_cache",
        checksum: "V5__pricing_catalog_cache.sql",
        libsql: BackendMigrationStep::Sql(include_str!(
            "../migrations/V5__pricing_catalog_cache.sql"
        )),
        postgres: BackendMigrationStep::Compatibility {
            reason: "PostgreSQL V1 already includes pricing catalog cache support.",
        },
    },
    MigrationManifest {
        version: 6,
        name: "identity_onboarding",
        checksum: "V6__identity_onboarding.sql",
        libsql: BackendMigrationStep::Sql(include_str!(
            "../migrations/V6__identity_onboarding.sql"
        )),
        postgres: BackendMigrationStep::Compatibility {
            reason: "PostgreSQL V1 already includes identity onboarding tables.",
        },
    },
    MigrationManifest {
        version: 7,
        name: "user_password_rotation",
        checksum: "V7__user_password_rotation.sql",
        libsql: BackendMigrationStep::Sql(include_str!(
            "../migrations/V7__user_password_rotation.sql"
        )),
        postgres: BackendMigrationStep::Compatibility {
            reason: "PostgreSQL V1 already includes user password rotation fields.",
        },
    },
    MigrationManifest {
        version: 8,
        name: "model_aliases",
        checksum: "V8__model_aliases.sql",
        libsql: BackendMigrationStep::Sql(include_str!("../migrations/V8__model_aliases.sql")),
        postgres: BackendMigrationStep::Sql(include_str!(
            "../migrations/postgres/V8__model_aliases.sql"
        )),
    },
];

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
