#[derive(Debug, Clone, Copy)]
pub(crate) struct MigrationManifest {
    pub version: u32,
    pub name: &'static str,
    pub checksum: &'static str,
    pub libsql_sql: &'static str,
    pub postgres_sql: &'static str,
}

/// libsql migrations that rebuild a table other tables reference, with that
/// table. They run with foreign keys off, as SQLite's table-rebuild
/// procedure requires, and then check the rebuilt table's children.
const LIBSQL_REBUILT_PARENTS: &[(u32, &str)] = &[(66, "gateway_oauth_clients")];

impl MigrationManifest {
    pub(crate) fn libsql_rebuilt_parent(&self) -> Option<&'static str> {
        LIBSQL_REBUILT_PARENTS
            .iter()
            .find(|(version, _)| *version == self.version)
            .map(|(_, table)| *table)
    }

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

pub(crate) const MIGRATION_REGISTRY: &[MigrationManifest] = &[
    MigrationManifest {
        version: 17,
        name: "baseline",
        checksum: "V17__baseline.sql",
        libsql_sql: include_str!("../migrations/V17__baseline.sql"),
        postgres_sql: include_str!("../migrations/postgres/V17__baseline.sql"),
    },
    MigrationManifest {
        version: 18,
        name: "route_compatibility_profiles",
        checksum: "V18__route_compatibility_profiles.sql",
        libsql_sql: include_str!("../migrations/V18__route_compatibility_profiles.sql"),
        postgres_sql: include_str!("../migrations/postgres/V18__route_compatibility_profiles.sql"),
    },
    MigrationManifest {
        version: 19,
        name: "request_log_attempts",
        checksum: "V19__request_log_attempts.sql",
        libsql_sql: include_str!("../migrations/V19__request_log_attempts.sql"),
        postgres_sql: include_str!("../migrations/postgres/V19__request_log_attempts.sql"),
    },
    MigrationManifest {
        version: 20,
        name: "request_log_tool_cardinality",
        checksum: "V20__request_log_tool_cardinality.sql",
        libsql_sql: include_str!("../migrations/V20__request_log_tool_cardinality.sql"),
        postgres_sql: include_str!("../migrations/postgres/V20__request_log_tool_cardinality.sql"),
    },
    MigrationManifest {
        version: 21,
        name: "request_log_agent_harness",
        checksum: "V21__request_log_agent_harness.sql",
        libsql_sql: include_str!("../migrations/V21__request_log_agent_harness.sql"),
        postgres_sql: include_str!("../migrations/postgres/V21__request_log_agent_harness.sql"),
    },
    MigrationManifest {
        version: 22,
        name: "mcp_tool_invocations",
        checksum: "V22__mcp_tool_invocations.sql",
        libsql_sql: include_str!("../migrations/V22__mcp_tool_invocations.sql"),
        postgres_sql: include_str!("../migrations/postgres/V22__mcp_tool_invocations.sql"),
    },
    MigrationManifest {
        version: 23,
        name: "service_accounts",
        checksum: "V23__service_accounts.sql",
        libsql_sql: include_str!("../migrations/V23__service_accounts.sql"),
        postgres_sql: include_str!("../migrations/postgres/V23__service_accounts.sql"),
    },
    MigrationManifest {
        version: 24,
        name: "identity_entity_tags",
        checksum: "V24__identity_entity_tags.sql",
        libsql_sql: include_str!("../migrations/V24__identity_entity_tags.sql"),
        postgres_sql: include_str!("../migrations/postgres/V24__identity_entity_tags.sql"),
    },
    MigrationManifest {
        version: 25,
        name: "oidc_sso_hardening",
        checksum: "V25__oidc_sso_hardening.sql",
        libsql_sql: include_str!("../migrations/V25__oidc_sso_hardening.sql"),
        postgres_sql: include_str!("../migrations/postgres/V25__oidc_sso_hardening.sql"),
    },
    MigrationManifest {
        version: 26,
        name: "oauth_providers_and_github_sso",
        checksum: "V26__oauth_providers_and_github_sso.sql",
        libsql_sql: include_str!("../migrations/V26__oauth_providers_and_github_sso.sql"),
        postgres_sql: include_str!(
            "../migrations/postgres/V26__oauth_providers_and_github_sso.sql"
        ),
    },
    MigrationManifest {
        version: 27,
        name: "external_mcp_registry",
        checksum: "V27__external_mcp_registry.sql",
        libsql_sql: include_str!("../migrations/V27__external_mcp_registry.sql"),
        postgres_sql: include_str!("../migrations/postgres/V27__external_mcp_registry.sql"),
    },
    MigrationManifest {
        version: 28,
        name: "generic_budget_scopes",
        checksum: "V28__generic_budget_scopes.sql",
        libsql_sql: include_str!("../migrations/V28__generic_budget_scopes.sql"),
        postgres_sql: include_str!("../migrations/postgres/V28__generic_budget_scopes.sql"),
    },
    MigrationManifest {
        version: 29,
        name: "mcp_tool_access_and_token_overhead",
        checksum: "V29__mcp_tool_access_and_token_overhead.sql",
        libsql_sql: include_str!("../migrations/V29__mcp_tool_access_and_token_overhead.sql"),
        postgres_sql: include_str!(
            "../migrations/postgres/V29__mcp_tool_access_and_token_overhead.sql"
        ),
    },
    MigrationManifest {
        version: 30,
        name: "mcp_aggregate_sessions",
        checksum: "V30__mcp_aggregate_sessions.sql",
        libsql_sql: include_str!("../migrations/V30__mcp_aggregate_sessions.sql"),
        postgres_sql: include_str!("../migrations/postgres/V30__mcp_aggregate_sessions.sql"),
    },
    MigrationManifest {
        version: 31,
        name: "mcp_upstream_credential_bindings",
        checksum: "V31__mcp_upstream_credential_bindings.sql",
        libsql_sql: include_str!("../migrations/V31__mcp_upstream_credential_bindings.sql"),
        postgres_sql: include_str!(
            "../migrations/postgres/V31__mcp_upstream_credential_bindings.sql"
        ),
    },
    MigrationManifest {
        version: 32,
        name: "oauth_provider_allowed_email_domains",
        checksum: "V32__oauth_provider_allowed_email_domains.sql",
        libsql_sql: include_str!("../migrations/V32__oauth_provider_allowed_email_domains.sql"),
        postgres_sql: include_str!(
            "../migrations/postgres/V32__oauth_provider_allowed_email_domains.sql"
        ),
    },
    MigrationManifest {
        version: 33,
        name: "oauth_provider_sso_email_verification",
        checksum: "V33__oauth_provider_sso_email_verification.sql",
        libsql_sql: include_str!("../migrations/V33__oauth_provider_sso_email_verification.sql"),
        postgres_sql: include_str!(
            "../migrations/postgres/V33__oauth_provider_sso_email_verification.sql"
        ),
    },
    MigrationManifest {
        version: 34,
        name: "review_agent",
        checksum: "V34__review_agent.sql",
        libsql_sql: include_str!("../migrations/V34__review_agent.sql"),
        postgres_sql: include_str!("../migrations/postgres/V34__review_agent.sql"),
    },
    MigrationManifest {
        version: 35,
        name: "managed_api_key_secrets",
        checksum: "V35__managed_api_key_secrets.sql",
        libsql_sql: include_str!("../migrations/V35__managed_api_key_secrets.sql"),
        postgres_sql: include_str!("../migrations/postgres/V35__managed_api_key_secrets.sql"),
    },
    MigrationManifest {
        version: 36,
        name: "api_key_model_grant_mode",
        checksum: "V36__api_key_model_grant_mode.sql",
        libsql_sql: include_str!("../migrations/V36__api_key_model_grant_mode.sql"),
        postgres_sql: include_str!("../migrations/postgres/V36__api_key_model_grant_mode.sql"),
    },
    MigrationManifest {
        version: 37,
        name: "model_allowlists",
        checksum: "V37__model_allowlists.sql",
        libsql_sql: include_str!("../migrations/V37__model_allowlists.sql"),
        postgres_sql: include_str!("../migrations/postgres/V37__model_allowlists.sql"),
    },
    MigrationManifest {
        version: 38,
        name: "budget_sources",
        checksum: "V38__budget_sources.sql",
        libsql_sql: include_str!("../migrations/V38__budget_sources.sql"),
        postgres_sql: include_str!("../migrations/postgres/V38__budget_sources.sql"),
    },
    MigrationManifest {
        version: 39,
        name: "service_account_entity_tags",
        checksum: "V39__service_account_entity_tags.sql",
        libsql_sql: include_str!("../migrations/V39__service_account_entity_tags.sql"),
        postgres_sql: include_str!("../migrations/postgres/V39__service_account_entity_tags.sql"),
    },
    MigrationManifest {
        version: 40,
        name: "route_metadata_overrides",
        checksum: "V40__route_metadata_overrides.sql",
        libsql_sql: include_str!("../migrations/V40__route_metadata_overrides.sql"),
        postgres_sql: include_str!("../migrations/postgres/V40__route_metadata_overrides.sql"),
    },
    MigrationManifest {
        version: 41,
        name: "mcp_oauth_states",
        checksum: "V41__mcp_oauth_states.sql",
        libsql_sql: include_str!("../migrations/V41__mcp_oauth_states.sql"),
        postgres_sql: include_str!("../migrations/postgres/V41__mcp_oauth_states.sql"),
    },
    MigrationManifest {
        version: 42,
        name: "cache_usage_accounting",
        checksum: "V42__cache_usage_accounting.sql",
        libsql_sql: include_str!("../migrations/V42__cache_usage_accounting.sql"),
        postgres_sql: include_str!("../migrations/postgres/V42__cache_usage_accounting.sql"),
    },
    MigrationManifest {
        version: 43,
        name: "validate_cache_usage_accounting",
        checksum: "V43__validate_cache_usage_accounting.sql",
        libsql_sql: include_str!("../migrations/V43__validate_cache_usage_accounting.sql"),
        postgres_sql: include_str!(
            "../migrations/postgres/V43__validate_cache_usage_accounting.sql"
        ),
    },
    MigrationManifest {
        version: 44,
        name: "agent_session_analysis",
        checksum: "V44__agent_session_analysis.sql",
        libsql_sql: include_str!("../migrations/V44__agent_session_analysis.sql"),
        postgres_sql: include_str!("../migrations/postgres/V44__agent_session_analysis.sql"),
    },
    MigrationManifest {
        version: 45,
        name: "agent_analysis_configuration_version",
        checksum: "V45__agent_analysis_configuration_version.sql",
        libsql_sql: include_str!("../migrations/V45__agent_analysis_configuration_version.sql"),
        postgres_sql: include_str!(
            "../migrations/postgres/V45__agent_analysis_configuration_version.sql"
        ),
    },
    MigrationManifest {
        version: 46,
        name: "batch_jobs",
        checksum: "V46__batch_jobs.sql",
        libsql_sql: include_str!("../migrations/V46__batch_jobs.sql"),
        postgres_sql: include_str!("../migrations/postgres/V46__batch_jobs.sql"),
    },
    MigrationManifest {
        version: 47,
        name: "batch_pricing_snapshot",
        checksum: "V47__batch_pricing_snapshot.sql",
        libsql_sql: include_str!("../migrations/V47__batch_pricing_snapshot.sql"),
        postgres_sql: include_str!("../migrations/postgres/V47__batch_pricing_snapshot.sql"),
    },
    MigrationManifest {
        version: 48,
        name: "guardrail_decisions",
        checksum: "V48__guardrail_decisions.sql",
        libsql_sql: include_str!("../migrations/V48__guardrail_decisions.sql"),
        postgres_sql: include_str!("../migrations/postgres/V48__guardrail_decisions.sql"),
    },
    MigrationManifest {
        version: 49,
        name: "provider_user_credentials",
        checksum: "V49__provider_user_credentials.sql",
        libsql_sql: include_str!("../migrations/V49__provider_user_credentials.sql"),
        postgres_sql: include_str!("../migrations/postgres/V49__provider_user_credentials.sql"),
    },
    MigrationManifest {
        version: 50,
        name: "model_reasoning_effort_policy",
        checksum: "V50__model_reasoning_effort_policy.sql",
        libsql_sql: include_str!("../migrations/V50__model_reasoning_effort_policy.sql"),
        postgres_sql: include_str!("../migrations/postgres/V50__model_reasoning_effort_policy.sql"),
    },
    MigrationManifest {
        version: 51,
        name: "budget_service_account_source",
        checksum: "V51__budget_service_account_source.sql",
        libsql_sql: include_str!("../migrations/V51__budget_service_account_source.sql"),
        postgres_sql: include_str!("../migrations/postgres/V51__budget_service_account_source.sql"),
    },
    MigrationManifest {
        version: 52,
        name: "skills",
        checksum: "V52__skills.sql",
        libsql_sql: include_str!("../migrations/V52__skills.sql"),
        postgres_sql: include_str!("../migrations/postgres/V52__skills.sql"),
    },
    MigrationManifest {
        version: 53,
        name: "model_routing_policy",
        checksum: "V53__model_routing_policy.sql",
        libsql_sql: include_str!("../migrations/V53__model_routing_policy.sql"),
        postgres_sql: include_str!("../migrations/postgres/V53__model_routing_policy.sql"),
    },
    MigrationManifest {
        version: 54,
        name: "request_log_request_tool_count",
        checksum: "V54__request_log_request_tool_count.sql",
        libsql_sql: include_str!("../migrations/V54__request_log_request_tool_count.sql"),
        postgres_sql: include_str!(
            "../migrations/postgres/V54__request_log_request_tool_count.sql"
        ),
    },
    MigrationManifest {
        version: 55,
        name: "request_log_invoked_distinct_tool_count",
        checksum: "V55__request_log_invoked_distinct_tool_count.sql",
        libsql_sql: include_str!("../migrations/V55__request_log_invoked_distinct_tool_count.sql"),
        postgres_sql: include_str!(
            "../migrations/postgres/V55__request_log_invoked_distinct_tool_count.sql"
        ),
    },
    MigrationManifest {
        version: 56,
        name: "budget_increases",
        checksum: "V56__budget_increases.sql",
        libsql_sql: include_str!("../migrations/V56__budget_increases.sql"),
        postgres_sql: include_str!("../migrations/postgres/V56__budget_increases.sql"),
    },
    MigrationManifest {
        version: 57,
        name: "mcp_invocation_method",
        checksum: "V57__mcp_invocation_method.sql",
        libsql_sql: include_str!("../migrations/V57__mcp_invocation_method.sql"),
        postgres_sql: include_str!("../migrations/postgres/V57__mcp_invocation_method.sql"),
    },
    MigrationManifest {
        version: 58,
        name: "mcp_tool_metadata",
        checksum: "V58__mcp_tool_metadata.sql",
        libsql_sql: include_str!("../migrations/V58__mcp_tool_metadata.sql"),
        postgres_sql: include_str!("../migrations/postgres/V58__mcp_tool_metadata.sql"),
    },
    MigrationManifest {
        version: 59,
        name: "mcp_oauth_clients",
        checksum: "V59__mcp_oauth_clients.sql",
        libsql_sql: include_str!("../migrations/V59__mcp_oauth_clients.sql"),
        postgres_sql: include_str!("../migrations/postgres/V59__mcp_oauth_clients.sql"),
    },
    MigrationManifest {
        version: 60,
        name: "rate_limits",
        checksum: "V60__rate_limits.sql",
        libsql_sql: include_str!("../migrations/V60__rate_limits.sql"),
        postgres_sql: include_str!("../migrations/postgres/V60__rate_limits.sql"),
    },
    MigrationManifest {
        version: 61,
        name: "admin_audit_log",
        checksum: "V61__admin_audit_log.sql",
        libsql_sql: include_str!("../migrations/V61__admin_audit_log.sql"),
        postgres_sql: include_str!("../migrations/postgres/V61__admin_audit_log.sql"),
    },
    MigrationManifest {
        version: 62,
        name: "gateway_oauth_authorization",
        checksum: "V62__gateway_oauth_authorization.sql",
        libsql_sql: include_str!("../migrations/V62__gateway_oauth_authorization.sql"),
        postgres_sql: include_str!("../migrations/postgres/V62__gateway_oauth_authorization.sql"),
    },
    MigrationManifest {
        version: 63,
        name: "mcp_aggregate_session_oauth_grants",
        checksum: "V63__mcp_aggregate_session_oauth_grants.sql",
        libsql_sql: include_str!("../migrations/V63__mcp_aggregate_session_oauth_grants.sql"),
        postgres_sql: include_str!(
            "../migrations/postgres/V63__mcp_aggregate_session_oauth_grants.sql"
        ),
    },
    MigrationManifest {
        version: 64,
        name: "gateway_oauth_cleanup_indexes",
        checksum: "V64__gateway_oauth_cleanup_indexes.sql",
        libsql_sql: include_str!("../migrations/V64__gateway_oauth_cleanup_indexes.sql"),
        postgres_sql: include_str!("../migrations/postgres/V64__gateway_oauth_cleanup_indexes.sql"),
    },
    MigrationManifest {
        version: 65,
        name: "api_key_scopes",
        checksum: "V65__api_key_scopes.sql",
        libsql_sql: include_str!("../migrations/V65__api_key_scopes.sql"),
        postgres_sql: include_str!("../migrations/postgres/V65__api_key_scopes.sql"),
    },
    MigrationManifest {
        version: 66,
        name: "gateway_oauth_metadata_document_clients",
        checksum: "V66__gateway_oauth_metadata_document_clients.sql",
        libsql_sql: include_str!("../migrations/V66__gateway_oauth_metadata_document_clients.sql"),
        postgres_sql: include_str!(
            "../migrations/postgres/V66__gateway_oauth_metadata_document_clients.sql"
        ),
    },
    MigrationManifest {
        version: 67,
        name: "mcp_tool_tag_rules",
        checksum: "V67__mcp_tool_tag_rules.sql",
        libsql_sql: include_str!("../migrations/V67__mcp_tool_tag_rules.sql"),
        postgres_sql: include_str!("../migrations/postgres/V67__mcp_tool_tag_rules.sql"),
    },
    MigrationManifest {
        version: 68,
        name: "mcp_tool_versions",
        checksum: "V68__mcp_tool_versions.sql",
        libsql_sql: include_str!("../migrations/V68__mcp_tool_versions.sql"),
        postgres_sql: include_str!("../migrations/postgres/V68__mcp_tool_versions.sql"),
    },
    MigrationManifest {
        version: 69,
        name: "mcp_tool_effective_tags",
        checksum: "V69__mcp_tool_effective_tags.sql",
        libsql_sql: include_str!("../migrations/V69__mcp_tool_effective_tags.sql"),
        postgres_sql: include_str!("../migrations/postgres/V69__mcp_tool_effective_tags.sql"),
    },
    MigrationManifest {
        version: 70,
        name: "mcp_server_ownership",
        checksum: "V70__mcp_server_ownership.sql",
        libsql_sql: include_str!("../migrations/V70__mcp_server_ownership.sql"),
        postgres_sql: include_str!("../migrations/postgres/V70__mcp_server_ownership.sql"),
    },
    MigrationManifest {
        version: 71,
        name: "mcp_oauth_state_issuer",
        checksum: "V71__mcp_oauth_state_issuer.sql",
        libsql_sql: include_str!("../migrations/V71__mcp_oauth_state_issuer.sql"),
        postgres_sql: include_str!("../migrations/postgres/V71__mcp_oauth_state_issuer.sql"),
    },
    MigrationManifest {
        version: 72,
        name: "mcp_discovery_run_started_ms",
        checksum: "V72__mcp_discovery_run_started_ms.sql",
        libsql_sql: include_str!("../migrations/V72__mcp_discovery_run_started_ms.sql"),
        postgres_sql: include_str!("../migrations/postgres/V72__mcp_discovery_run_started_ms.sql"),
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
