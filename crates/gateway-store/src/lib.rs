//! Store crate boundary.
//!
//! Split plan: keep this root as a narrow export surface and move the large
//! backend integration test module into domain-focused files under
//! `tests/` or private test submodules as store domains continue to grow.
mod any_store_mcp_access;
mod any_store_mcp_aggregate_sessions;
mod any_store_mcp_credentials;
mod any_store_mcp_registry;
mod any_store_mcp_token_overhead;
mod libsql_store;
mod migrate;
mod migration_registry;
mod postgres_store;
mod seed;
mod shared;
mod store;

pub use libsql_store::LibsqlStore;
pub use migrate::{
    MigrationStatus, MigrationStatusEntry, check_migrations_with_options, run_migrations,
    run_migrations_with_options, status_migrations_with_options,
};
pub use postgres_store::PostgresStore;
pub use store::{AnyStore, GatewayStore, StoreConnectionOptions};

#[cfg(test)]
pub(crate) use migrate::{MigrationTestHook, run_migrations_with_options_for_test};

#[cfg(test)]
mod tests {
    use std::env;

    use gateway_core::{
        ApiKeyOwnerKind, ApiKeyRepository, AuthMode, BudgetAlertChannel, BudgetAlertDeliveryRecord,
        BudgetAlertDeliveryStatus, BudgetAlertHistoryQuery, BudgetAlertRecord,
        BudgetAlertRepository, BudgetCadence, BudgetRepository, BudgetScope, BudgetSettings,
        ExternalMcpAuthMode, ExternalMcpDiscoveryRunRecord, ExternalMcpDiscoveryStatus,
        ExternalMcpServerStatus, ExternalMcpTransport, GlobalRole, IdentityRepository,
        McpRegistryRepository, McpToolInvocationPayloadRecord, McpToolInvocationQuery,
        McpToolInvocationRecord, McpToolInvocationRepository, McpToolInvocationStatus,
        McpToolPolicyResult, McpUpstreamCredentialMaterialKind,
        McpUpstreamCredentialOwnerScopeKind, McpUpstreamCredentialRepository,
        McpUpstreamSecretStorageKind, MembershipRole, ModelPricingRecord, ModelRepository, Money4,
        NewExternalMcpServerRecord, OauthJitPolicy, OidcLoginStateRecord,
        OpenAiCompatDeveloperRole, OpenAiCompatMaxTokensField, OpenAiCompatReasoningEffort,
        OpenAiCompatRouteCompatibility, PricingCatalogCacheRecord, PricingCatalogRepository,
        PricingLimits, PricingModalities, PricingProvenance, ProviderCapabilities,
        RequestAttemptRecord, RequestAttemptStatus, RequestLogPayloadRecord, RequestLogQuery,
        RequestLogRecord, RequestLogRepository, RequestTag, RequestTags, RequestToolCardinality,
        RouteCompatibility, SeedApiKey, SeedBudget, SeedModel, SeedModelRoute, SeedOauthProvider,
        SeedProvider, SeedTeam, SeedUser, SeedUserMembership, StoreError, StoreHealth,
        UpdateExternalMcpServerRecord, UpsertExternalMcpToolRecord,
        UpsertMcpUpstreamCredentialBindingRecord, UsageLedgerRecord, UsagePricingStatus,
        UserStatus,
    };
    use serde_json::{Map, json};
    use serial_test::serial;
    use sqlx::Row;
    use tempfile::tempdir;
    use time::{Duration, OffsetDateTime};
    use url::Url;
    use uuid::Uuid;

    use crate::{
        GatewayStore, LibsqlStore, MigrationTestHook, PostgresStore, StoreConnectionOptions,
        check_migrations_with_options,
        migration_registry::{MIGRATION_REGISTRY, MigrationBackend},
        run_migrations, run_migrations_with_options, run_migrations_with_options_for_test,
        status_migrations_with_options,
    };

    fn focus_export_owner_tags() -> (Vec<RequestTag>, Vec<RequestTag>) {
        (
            vec![RequestTag {
                key: "cost_center".to_string(),
                value: "eng".to_string(),
            }],
            vec![RequestTag {
                key: "team".to_string(),
                value: "platform".to_string(),
            }],
        )
    }

    fn seed_api_key_teams() -> Vec<SeedTeam> {
        vec![SeedTeam {
            team_key: "seed-workloads".to_string(),
            team_name: "Seed Workloads".to_string(),
        }]
    }

    fn seed_github_oauth_provider_with_domains(domains: Vec<&str>) -> SeedOauthProvider {
        SeedOauthProvider {
            provider_key: "github".to_string(),
            provider_type: "github".to_string(),
            label: "GitHub".to_string(),
            client_id: "client-id".to_string(),
            client_secret_ref: "literal.secret".to_string(),
            scopes: vec!["read:user".to_string(), "user:email".to_string()],
            allowed_email_domains: domains.into_iter().map(str::to_string).collect(),
            enabled: true,
            jit: OauthJitPolicy::default(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn build_usage_ledger_record(
        request_id: &str,
        ownership_scope_key: String,
        api_key_id: Uuid,
        user_id: Option<Uuid>,
        team_id: Option<Uuid>,
        service_account_id: Option<Uuid>,
        model_id: Option<Uuid>,
        upstream_model: &str,
        pricing_status: UsagePricingStatus,
        computed_cost_10000: i64,
        occurred_at: OffsetDateTime,
    ) -> UsageLedgerRecord {
        let unpriced_reason = match pricing_status {
            UsagePricingStatus::Unpriced => Some("missing_pricing".to_string()),
            _ => None,
        };

        UsageLedgerRecord {
            usage_event_id: Uuid::new_v4(),
            request_id: request_id.to_string(),
            ownership_scope_key,
            api_key_id,
            user_id,
            team_id,
            service_account_id,
            actor_user_id: None,
            model_id,
            provider_key: "openai-prod".to_string(),
            upstream_model: upstream_model.to_string(),
            prompt_tokens: Some(100),
            completion_tokens: Some(50),
            total_tokens: Some(150),
            provider_usage: json!({
                "prompt_tokens": 100,
                "completion_tokens": 50,
                "total_tokens": 150
            }),
            pricing_status,
            unpriced_reason,
            pricing_row_id: None,
            pricing_provider_id: Some("openai".to_string()),
            pricing_model_id: Some(upstream_model.to_string()),
            pricing_source: Some("test".to_string()),
            pricing_source_etag: Some("etag-1".to_string()),
            pricing_source_fetched_at: Some(occurred_at),
            pricing_last_updated: Some("2026-03-15".to_string()),
            input_cost_per_million_tokens: Some(Money4::from_scaled(1_250)),
            output_cost_per_million_tokens: Some(Money4::from_scaled(10_000)),
            computed_cost_usd: Money4::from_scaled(computed_cost_10000),
            occurred_at,
        }
    }

    async fn assert_focus_export_aggregates<S>(
        store: &S,
        window_start: OffsetDateTime,
        window_end: OffsetDateTime,
        user_id: Uuid,
        service_account_id: Uuid,
    ) where
        S: BudgetRepository,
    {
        let rows = store
            .list_focus_export_aggregates(window_start, window_end, None, None)
            .await
            .expect("focus export aggregates");
        assert_eq!(rows.len(), 2);
        assert!(
            rows.iter()
                .all(|row| row.pricing_status.counts_toward_spend())
        );

        let user_row = rows
            .iter()
            .find(|row| row.owner_kind == ApiKeyOwnerKind::User)
            .expect("user focus row");
        assert_eq!(user_row.owner_id, user_id);
        assert_eq!(user_row.model_key, "fast");
        assert_eq!(user_row.computed_cost_usd, Money4::from_scaled(11_000));
        assert_eq!(user_row.request_count, 1);
        assert_eq!(user_row.prompt_tokens, 100);
        assert_eq!(user_row.completion_tokens, 50);
        assert_eq!(user_row.total_tokens, 150);
        assert_eq!(
            user_row.owner_tags,
            vec![RequestTag {
                key: "cost_center".to_string(),
                value: "eng".to_string(),
            }]
        );

        let service_account_row = rows
            .iter()
            .find(|row| row.owner_kind == ApiKeyOwnerKind::ServiceAccount)
            .expect("service account focus row");
        assert_eq!(service_account_row.owner_id, service_account_id);
        assert_eq!(service_account_row.model_key, "claude-3-5-sonnet");
        assert_eq!(
            service_account_row.computed_cost_usd,
            Money4::from_scaled(22_000)
        );
        assert_eq!(
            service_account_row.pricing_status,
            UsagePricingStatus::LegacyEstimated
        );
        assert_eq!(
            service_account_row.owner_tags,
            vec![RequestTag {
                key: "team".to_string(),
                value: "platform".to_string(),
            }]
        );

        let diagnostics = store
            .get_focus_export_diagnostics(window_start, window_end, None, None)
            .await
            .expect("focus diagnostics");
        assert_eq!(diagnostics.unpriced_request_count, 2);
        assert_eq!(diagnostics.usage_missing_request_count, 1);

        let user_rows = store
            .list_focus_export_aggregates(
                window_start,
                window_end,
                Some(ApiKeyOwnerKind::User),
                Some(user_id),
            )
            .await
            .expect("self focus rows");
        assert_eq!(user_rows.len(), 1);
        assert_eq!(user_rows[0].owner_id, user_id);

        let daily_rows = store
            .list_focus_export_aggregates(
                user_row.day_start,
                user_row.day_start + Duration::days(1),
                None,
                None,
            )
            .await
            .expect("daily focus rows");
        assert_eq!(daily_rows.len(), 1);
        assert_eq!(daily_rows[0].owner_id, user_id);
    }

    fn build_mcp_tool_invocation(
        request_id: &str,
        api_key_id: Option<Uuid>,
        user_id: Option<Uuid>,
        team_id: Option<Uuid>,
        occurred_at: OffsetDateTime,
    ) -> McpToolInvocationRecord {
        McpToolInvocationRecord {
            mcp_tool_invocation_id: Uuid::new_v4(),
            request_log_id: None,
            request_id: request_id.to_string(),
            api_key_id,
            user_id,
            team_id,
            owner_kind: if team_id.is_some() {
                ApiKeyOwnerKind::ServiceAccount
            } else {
                ApiKeyOwnerKind::User
            },
            server_id: None,
            server_display_key: "github-prod".to_string(),
            server_display_name: "GitHub Production".to_string(),
            tool_id: None,
            tool_display_key: "issues.create".to_string(),
            tool_display_name: "Create Issue".to_string(),
            status: McpToolInvocationStatus::Success,
            policy_result: McpToolPolicyResult::Allowed,
            latency_ms: Some(42),
            error_code: None,
            has_payload: true,
            arguments_payload_truncated: false,
            result_payload_truncated: true,
            arguments_payload_redacted: true,
            result_payload_redacted: false,
            metadata: Map::new(),
            occurred_at,
        }
    }

    async fn exercise_mcp_tool_invocation_repository<S>(store: &S)
    where
        S: GatewayStore + McpToolInvocationRepository + Sync,
    {
        let providers = vec![SeedProvider {
            provider_key: "openai-prod".to_string(),
            provider_type: "openai_compat".to_string(),
            config: json!({"base_url": "https://api.openai.com/v1"}),
            secrets: None,
        }];
        let models = vec![SeedModel {
            model_key: "fast".to_string(),
            alias_target_model_key: None,
            description: None,
            tags: vec![],
            rank: 10,
            routes: vec![SeedModelRoute {
                provider_key: "openai-prod".to_string(),
                upstream_model: "gpt-4o-mini".to_string(),
                priority: 10,
                weight: 1.0,
                enabled: true,
                extra_headers: Map::new(),
                extra_body: Map::new(),
                capabilities: ProviderCapabilities::all_enabled(),
                compatibility: Default::default(),
            }],
        }];
        let api_keys = vec![SeedApiKey {
            name: "dev".to_string(),
            public_id: "dev123".to_string(),
            secret_hash: "hash".to_string(),
            service_account_key: "seed-workloads".to_string(),
            service_account_name: "Seed Workloads".to_string(),
            service_account_team_key: "seed-workloads".to_string(),
            service_account_budget: SeedBudget {
                cadence: BudgetCadence::Daily,
                amount_usd: Money4::from_scaled(100_000),
                hard_limit: true,
                timezone: "UTC".to_string(),
            },
            allowed_models: vec!["fast".to_string()],
        }];

        store
            .seed_from_inputs(
                &providers,
                &models,
                &api_keys,
                &[],
                &[],
                &seed_api_key_teams(),
                &[],
            )
            .await
            .expect("seed");
        let api_key = store
            .get_api_key_by_public_id("dev123")
            .await
            .expect("load api key")
            .expect("api key");
        let occurred_at = OffsetDateTime::now_utc();
        let invocation = build_mcp_tool_invocation(
            "req-mcp-1",
            Some(api_key.id),
            None,
            api_key.owner_team_id,
            occurred_at,
        );
        let payload = McpToolInvocationPayloadRecord {
            mcp_tool_invocation_id: invocation.mcp_tool_invocation_id,
            arguments_json: json!({"owner": "ahstn", "token": "[REDACTED]"}),
            result_json: json!({"issue": 115, "truncated": true}),
        };
        let denied = McpToolInvocationRecord {
            mcp_tool_invocation_id: Uuid::new_v4(),
            request_id: "req-mcp-2".to_string(),
            tool_display_key: "repos.delete".to_string(),
            tool_display_name: "Delete Repository".to_string(),
            status: McpToolInvocationStatus::Unauthorized,
            policy_result: McpToolPolicyResult::Denied,
            has_payload: false,
            occurred_at: occurred_at - Duration::seconds(60),
            ..invocation.clone()
        };

        store
            .insert_mcp_tool_invocation(&invocation, Some(&payload))
            .await
            .expect("insert invocation");
        store
            .insert_mcp_tool_invocation(&denied, None)
            .await
            .expect("insert denied invocation");

        let page = store
            .list_mcp_tool_invocations(&McpToolInvocationQuery {
                page: 1,
                page_size: 10,
                request_id: Some("req-mcp-1".to_string()),
                server_display_key: Some("github-prod".to_string()),
                server_display_name: Some("GitHub Production".to_string()),
                tool_display_key: Some("issues.create".to_string()),
                tool_display_name: Some("Create Issue".to_string()),
                api_key_id: Some(api_key.id),
                user_id: None,
                team_id: api_key.owner_team_id,
                status: Some(McpToolInvocationStatus::Success),
                policy_result: Some(McpToolPolicyResult::Allowed),
                occurred_at_start: Some(occurred_at - Duration::seconds(5)),
                occurred_at_end: Some(occurred_at + Duration::seconds(5)),
            })
            .await
            .expect("list invocations");
        assert_eq!(page.total, 1);
        assert_eq!(page.items[0].tool_display_key, "issues.create");
        assert!(page.items[0].arguments_payload_redacted);
        assert!(page.items[0].result_payload_truncated);

        let denied_page = store
            .list_mcp_tool_invocations(&McpToolInvocationQuery {
                page: 1,
                page_size: 10,
                status: Some(McpToolInvocationStatus::Unauthorized),
                policy_result: Some(McpToolPolicyResult::Denied),
                ..Default::default()
            })
            .await
            .expect("list denied invocations");
        assert_eq!(denied_page.total, 1);
        assert_eq!(denied_page.items[0].request_id, "req-mcp-2");

        let detail = store
            .get_mcp_tool_invocation_detail(invocation.mcp_tool_invocation_id)
            .await
            .expect("invocation detail");
        assert_eq!(detail.invocation.request_id, "req-mcp-1");
        assert_eq!(
            detail.payload.expect("payload").arguments_json["token"],
            "[REDACTED]"
        );
    }

    async fn exercise_mcp_registry_repository<S>(store: &S)
    where
        S: GatewayStore + McpRegistryRepository + Sync,
    {
        let now = OffsetDateTime::now_utc();
        let server = store
            .create_external_mcp_server(&NewExternalMcpServerRecord {
                server_key: "github-prod".to_string(),
                display_name: "GitHub Production".to_string(),
                description: Some("Production GitHub MCP".to_string()),
                transport: ExternalMcpTransport::StreamableHttp,
                server_url: "https://mcp.example.com/mcp".to_string(),
                auth_mode: ExternalMcpAuthMode::None,
                auth_config: Map::new(),
                timeout_ms: 30_000,
                created_at: now,
            })
            .await
            .expect("create MCP server");

        let listed = store
            .list_external_mcp_servers(false)
            .await
            .expect("list MCP servers");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].server_key, "github-prod");

        let updated = store
            .update_external_mcp_server(&UpdateExternalMcpServerRecord {
                mcp_server_id: server.mcp_server_id,
                display_name: "GitHub".to_string(),
                description: None,
                server_url: "https://mcp.example.com/updated".to_string(),
                auth_mode: ExternalMcpAuthMode::GatewayBearerToken,
                auth_config: Map::from_iter([(
                    "secret_ref".to_string(),
                    json!("env/OCEANS_MCP_DISCOVERY_GITHUB_TOKEN"),
                )]),
                timeout_ms: 45_000,
                updated_at: now + Duration::seconds(1),
            })
            .await
            .expect("update MCP server");
        assert_eq!(updated.display_name, "GitHub");
        assert_eq!(updated.auth_mode, ExternalMcpAuthMode::GatewayBearerToken);

        let first_run = ExternalMcpDiscoveryRunRecord {
            discovery_run_id: Uuid::new_v4(),
            mcp_server_id: server.mcp_server_id,
            status: ExternalMcpDiscoveryStatus::Success,
            started_at: now + Duration::seconds(2),
            finished_at: now + Duration::seconds(3),
            discovered_tool_count: 2,
            active_tool_count: 2,
            schema_set_hash: Some("sha256:first".to_string()),
            error_summary: None,
            details: Map::new(),
        };
        let first_tools = store
            .record_external_mcp_discovery_success(
                &first_run,
                &[
                    UpsertExternalMcpToolRecord {
                        mcp_server_id: server.mcp_server_id,
                        upstream_name: "issues.create".to_string(),
                        display_name: "issues.create".to_string(),
                        description: Some("Create issue".to_string()),
                        input_schema: json!({
                            "type": "object",
                            "properties": {"title": {"type": "string"}}
                        }),
                        schema_hash: "sha256:title".to_string(),
                    },
                    UpsertExternalMcpToolRecord {
                        mcp_server_id: server.mcp_server_id,
                        upstream_name: "repos.get".to_string(),
                        display_name: "repos.get".to_string(),
                        description: None,
                        input_schema: json!({
                            "type": "object",
                            "properties": {"repo": {"type": "string"}}
                        }),
                        schema_hash: "sha256:repo".to_string(),
                    },
                ],
            )
            .await
            .expect("record discovery success");
        assert_eq!(first_tools.len(), 2);
        let issue_tool = first_tools
            .iter()
            .find(|tool| tool.upstream_name == "issues.create")
            .expect("issue tool");
        assert_eq!(issue_tool.schema_version, 1);
        let issue_tool_id = issue_tool.mcp_tool_id;

        let second_run = ExternalMcpDiscoveryRunRecord {
            discovery_run_id: Uuid::new_v4(),
            mcp_server_id: server.mcp_server_id,
            status: ExternalMcpDiscoveryStatus::Success,
            started_at: now + Duration::seconds(4),
            finished_at: now + Duration::seconds(5),
            discovered_tool_count: 1,
            active_tool_count: 1,
            schema_set_hash: Some("sha256:second".to_string()),
            error_summary: None,
            details: Map::new(),
        };
        let second_tools = store
            .record_external_mcp_discovery_success(
                &second_run,
                &[UpsertExternalMcpToolRecord {
                    mcp_server_id: server.mcp_server_id,
                    upstream_name: "issues.create".to_string(),
                    display_name: "issues.create".to_string(),
                    description: Some("Create issue".to_string()),
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "title": {"type": "string"},
                            "body": {"type": "string"}
                        }
                    }),
                    schema_hash: "sha256:title-body".to_string(),
                }],
            )
            .await
            .expect("record second discovery success");
        assert_eq!(second_tools.len(), 1);
        assert_eq!(second_tools[0].mcp_tool_id, issue_tool_id);
        assert_eq!(second_tools[0].schema_version, 2);

        let all_tools = store
            .list_external_mcp_tools(server.mcp_server_id, true)
            .await
            .expect("list inactive tools");
        assert_eq!(all_tools.len(), 2);
        assert!(
            all_tools
                .iter()
                .any(|tool| tool.upstream_name == "repos.get" && !tool.is_active)
        );

        let invalidated = store
            .update_external_mcp_server(&UpdateExternalMcpServerRecord {
                mcp_server_id: server.mcp_server_id,
                display_name: "GitHub".to_string(),
                description: None,
                server_url: "https://mcp.example.com/rotated".to_string(),
                auth_mode: ExternalMcpAuthMode::GatewayBearerToken,
                auth_config: Map::from_iter([(
                    "secret_ref".to_string(),
                    json!("env/OCEANS_MCP_DISCOVERY_GITHUB_TOKEN"),
                )]),
                timeout_ms: 45_000,
                updated_at: now + Duration::seconds(6),
            })
            .await
            .expect("invalidate discovery on config update");
        assert_eq!(invalidated.last_discovery_status, None);
        assert_eq!(invalidated.last_tool_count, Some(0));
        assert_eq!(invalidated.last_error_summary, None);

        let active_after_config_change = store
            .list_external_mcp_tools(server.mcp_server_id, false)
            .await
            .expect("list active tools after config change");
        assert!(active_after_config_change.is_empty());

        store
            .record_external_mcp_discovery_failure(&ExternalMcpDiscoveryRunRecord {
                discovery_run_id: Uuid::new_v4(),
                mcp_server_id: server.mcp_server_id,
                status: ExternalMcpDiscoveryStatus::Failed,
                started_at: now + Duration::seconds(7),
                finished_at: now + Duration::seconds(8),
                discovered_tool_count: 0,
                active_tool_count: 0,
                schema_set_hash: None,
                error_summary: Some("timeout".to_string()),
                details: Map::new(),
            })
            .await
            .expect("record discovery failure");
        let failed_server = store
            .get_external_mcp_server(server.mcp_server_id)
            .await
            .expect("load failed server")
            .expect("server");
        assert_eq!(
            failed_server.last_discovery_status,
            Some(ExternalMcpDiscoveryStatus::Failed)
        );
        assert_eq!(failed_server.last_error_summary.as_deref(), Some("timeout"));

        let disabled = store
            .disable_external_mcp_server(server.mcp_server_id, now + Duration::seconds(9))
            .await
            .expect("disable server");
        assert_eq!(disabled.status, ExternalMcpServerStatus::Disabled);
        assert!(disabled.disabled_at.is_some());
        assert!(
            store
                .list_external_mcp_servers(false)
                .await
                .expect("list active")
                .is_empty()
        );
    }

    async fn exercise_mcp_upstream_credential_repository<S>(store: &S)
    where
        S: GatewayStore
            + IdentityRepository
            + McpRegistryRepository
            + McpUpstreamCredentialRepository
            + Sync,
    {
        let now = OffsetDateTime::now_utc();
        let server = store
            .create_external_mcp_server(&NewExternalMcpServerRecord {
                server_key: "github-creds".to_string(),
                display_name: "GitHub Credentials".to_string(),
                description: None,
                transport: ExternalMcpTransport::StreamableHttp,
                server_url: "https://mcp.example.com/mcp".to_string(),
                auth_mode: ExternalMcpAuthMode::UserPassthrough,
                auth_config: Map::new(),
                timeout_ms: 30_000,
                created_at: now,
            })
            .await
            .expect("create MCP server for credentials");
        let user = store
            .create_identity_user(
                "MCP Credential User",
                "mcp-credential-user@example.com",
                "mcp-credential-user@example.com",
                GlobalRole::User,
                AuthMode::Password,
                UserStatus::Active,
            )
            .await
            .expect("create credential owner user");
        let user_id = user.user_id;
        let scope_key = format!("mcp_credential:v1:user:{user_id}");
        let binding = store
            .upsert_mcp_upstream_credential_binding(&UpsertMcpUpstreamCredentialBindingRecord {
                credential_binding_id: None,
                mcp_server_id: server.mcp_server_id,
                owner_scope_kind: McpUpstreamCredentialOwnerScopeKind::User,
                owner_scope_key: scope_key.clone(),
                owner_user_id: Some(user_id),
                owner_team_id: None,
                owner_service_account_id: None,
                material_kind: McpUpstreamCredentialMaterialKind::BearerToken,
                header_name: None,
                storage_kind: McpUpstreamSecretStorageKind::SecretRef,
                secret_ciphertext: None,
                secret_nonce: None,
                secret_key_id: None,
                secret_ref: Some("env/OCEANS_MCP_CREDENTIAL_GITHUB".to_string()),
                expires_at: Some(now + Duration::hours(1)),
                metadata: Map::from_iter([("source".to_string(), json!("test"))]),
                updated_at: now,
            })
            .await
            .expect("create credential binding");
        assert_eq!(binding.owner_scope_key, scope_key);
        assert_eq!(
            binding.secret_ref.as_deref(),
            Some("env/OCEANS_MCP_CREDENTIAL_GITHUB")
        );
        assert!(binding.secret_ciphertext.is_none());

        let duplicate = store
            .upsert_mcp_upstream_credential_binding(&UpsertMcpUpstreamCredentialBindingRecord {
                credential_binding_id: None,
                mcp_server_id: server.mcp_server_id,
                owner_scope_kind: McpUpstreamCredentialOwnerScopeKind::User,
                owner_scope_key: scope_key.clone(),
                owner_user_id: Some(user_id),
                owner_team_id: None,
                owner_service_account_id: None,
                material_kind: McpUpstreamCredentialMaterialKind::BearerToken,
                header_name: None,
                storage_kind: McpUpstreamSecretStorageKind::SecretRef,
                secret_ciphertext: None,
                secret_nonce: None,
                secret_key_id: None,
                secret_ref: Some("env/OCEANS_MCP_CREDENTIAL_DUPLICATE".to_string()),
                expires_at: None,
                metadata: Map::new(),
                updated_at: now + Duration::seconds(1),
            })
            .await;
        assert!(
            duplicate.is_err(),
            "active credential bindings should be unique per server/scope"
        );

        let active = store
            .get_active_mcp_upstream_credential_binding(server.mcp_server_id, &scope_key)
            .await
            .expect("get active credential")
            .expect("active credential");
        assert_eq!(active.credential_binding_id, binding.credential_binding_id);

        let user_filtered = store
            .list_mcp_upstream_credential_bindings(
                Some(server.mcp_server_id),
                Some(McpUpstreamCredentialOwnerScopeKind::User),
                Some(user_id),
                false,
            )
            .await
            .expect("list user credentials");
        assert_eq!(user_filtered.len(), 1);

        let last_used_at = now + Duration::minutes(5);
        assert!(
            store
                .touch_mcp_upstream_credential_binding_last_used(
                    binding.credential_binding_id,
                    last_used_at,
                )
                .await
                .expect("touch last used")
        );
        let touched = store
            .get_active_mcp_upstream_credential_binding(server.mcp_server_id, &scope_key)
            .await
            .expect("get touched credential")
            .expect("touched credential");
        assert_eq!(
            touched.last_used_at.map(|value| value.unix_timestamp()),
            Some(last_used_at.unix_timestamp())
        );

        assert!(
            store
                .revoke_mcp_upstream_credential_binding(
                    binding.credential_binding_id,
                    now + Duration::minutes(10),
                )
                .await
                .expect("revoke credential")
        );
        assert!(
            store
                .get_active_mcp_upstream_credential_binding(server.mcp_server_id, &scope_key)
                .await
                .expect("get revoked credential")
                .is_none()
        );
        assert!(
            !store
                .touch_mcp_upstream_credential_binding_last_used(
                    binding.credential_binding_id,
                    now + Duration::minutes(15),
                )
                .await
                .expect("touch revoked credential")
        );
        assert_eq!(
            store
                .list_mcp_upstream_credential_bindings(
                    Some(server.mcp_server_id),
                    None,
                    None,
                    false
                )
                .await
                .expect("list active after revoke")
                .len(),
            0
        );
        assert_eq!(
            store
                .list_mcp_upstream_credential_bindings(Some(server.mcp_server_id), None, None, true)
                .await
                .expect("list revoked")
                .len(),
            1
        );

        let replacement = store
            .upsert_mcp_upstream_credential_binding(&UpsertMcpUpstreamCredentialBindingRecord {
                credential_binding_id: None,
                mcp_server_id: server.mcp_server_id,
                owner_scope_kind: McpUpstreamCredentialOwnerScopeKind::User,
                owner_scope_key: scope_key,
                owner_user_id: Some(user_id),
                owner_team_id: None,
                owner_service_account_id: None,
                material_kind: McpUpstreamCredentialMaterialKind::StaticHeader,
                header_name: Some("X-Api-Key".to_string()),
                storage_kind: McpUpstreamSecretStorageKind::EncryptedBlob,
                secret_ciphertext: Some("ciphertext".to_string()),
                secret_nonce: Some("nonce".to_string()),
                secret_key_id: Some("env/OCEANS_MCP_CREDENTIAL_ENCRYPTION_KEY".to_string()),
                secret_ref: None,
                expires_at: None,
                metadata: Map::new(),
                updated_at: now + Duration::minutes(11),
            })
            .await
            .expect("create replacement credential");
        assert_eq!(
            replacement.material_kind,
            McpUpstreamCredentialMaterialKind::StaticHeader
        );
    }

    async fn exercise_usage_leaderboard_reporting<S>(store: &S)
    where
        S: ApiKeyRepository
            + BudgetRepository
            + GatewayStore
            + IdentityRepository
            + ModelRepository
            + RequestLogRepository
            + Sync,
    {
        let providers = vec![SeedProvider {
            provider_key: "openai-prod".to_string(),
            provider_type: "openai_compat".to_string(),
            config: json!({
                "base_url": "https://api.openai.com/v1",
                "timeout_ms": 120_000
            }),
            secrets: Some(json!({"token": "env.OPENAI_API_KEY"})),
        }];
        let models = vec![
            SeedModel {
                model_key: "fast".to_string(),
                alias_target_model_key: None,
                description: Some("fast tier".to_string()),
                tags: vec!["fast".to_string()],
                rank: 10,
                routes: vec![SeedModelRoute {
                    provider_key: "openai-prod".to_string(),
                    upstream_model: "gpt-4o-mini".to_string(),
                    priority: 10,
                    weight: 1.0,
                    enabled: true,
                    extra_headers: Map::new(),
                    extra_body: Map::new(),
                    capabilities: ProviderCapabilities::all_enabled(),
                    compatibility: Default::default(),
                }],
            },
            SeedModel {
                model_key: "reasoning".to_string(),
                alias_target_model_key: None,
                description: Some("reasoning tier".to_string()),
                tags: vec!["reasoning".to_string()],
                rank: 20,
                routes: vec![SeedModelRoute {
                    provider_key: "openai-prod".to_string(),
                    upstream_model: "gpt-5".to_string(),
                    priority: 10,
                    weight: 1.0,
                    enabled: true,
                    extra_headers: Map::new(),
                    extra_body: Map::new(),
                    capabilities: ProviderCapabilities::all_enabled(),
                    compatibility: Default::default(),
                }],
            },
        ];
        let api_keys = vec![SeedApiKey {
            name: "dev".to_string(),
            public_id: "dev123".to_string(),
            secret_hash: "hash".to_string(),
            service_account_key: "seed-workloads".to_string(),
            service_account_name: "Seed Workloads".to_string(),
            service_account_team_key: "seed-workloads".to_string(),
            service_account_budget: SeedBudget {
                cadence: BudgetCadence::Daily,
                amount_usd: Money4::from_scaled(100_000),
                hard_limit: true,
                timezone: "UTC".to_string(),
            },
            allowed_models: vec!["fast".to_string(), "reasoning".to_string()],
        }];
        store
            .seed_from_inputs(
                &providers,
                &models,
                &api_keys,
                &[],
                &[],
                &seed_api_key_teams(),
                &[],
            )
            .await
            .expect("seed");

        let api_key = store
            .get_api_key_by_public_id("dev123")
            .await
            .expect("load api key")
            .expect("api key");
        let fast_model = store
            .get_model_by_key("fast")
            .await
            .expect("load fast model")
            .expect("fast model");
        let reasoning_model = store
            .get_model_by_key("reasoning")
            .await
            .expect("load reasoning model")
            .expect("reasoning model");

        let ada = store
            .create_identity_user(
                "Ada",
                "ada@example.com",
                "ada@example.com",
                GlobalRole::User,
                AuthMode::Password,
                UserStatus::Active,
            )
            .await
            .expect("create ada");
        let ben = store
            .create_identity_user(
                "Ben",
                "ben@example.com",
                "ben@example.com",
                GlobalRole::User,
                AuthMode::Password,
                UserStatus::Active,
            )
            .await
            .expect("create ben");
        let cleo = store
            .create_identity_user(
                "Cleo",
                "cleo@example.com",
                "cleo@example.com",
                GlobalRole::User,
                AuthMode::Password,
                UserStatus::Active,
            )
            .await
            .expect("create cleo");

        let ada_bucket_one = OffsetDateTime::parse(
            "2026-03-02T03:00:00Z",
            &time::format_description::well_known::Rfc3339,
        )
        .expect("ada bucket one");
        let ada_bucket_two = OffsetDateTime::parse(
            "2026-03-02T16:00:00Z",
            &time::format_description::well_known::Rfc3339,
        )
        .expect("ada bucket two");
        let ben_bucket = OffsetDateTime::parse(
            "2026-03-04T05:00:00Z",
            &time::format_description::well_known::Rfc3339,
        )
        .expect("ben bucket");
        let ben_bucket_same = OffsetDateTime::parse(
            "2026-03-04T06:00:00Z",
            &time::format_description::well_known::Rfc3339,
        )
        .expect("ben bucket same");
        let cleo_bucket = OffsetDateTime::parse(
            "2026-03-05T02:00:00Z",
            &time::format_description::well_known::Rfc3339,
        )
        .expect("cleo bucket");
        let window_start = OffsetDateTime::parse(
            "2026-03-01T00:00:00Z",
            &time::format_description::well_known::Rfc3339,
        )
        .expect("window start");
        let window_end = OffsetDateTime::parse(
            "2026-03-08T00:00:00Z",
            &time::format_description::well_known::Rfc3339,
        )
        .expect("window end");

        for event in [
            build_usage_ledger_record(
                "leaderboard-ada-fast",
                format!("user:{}", ada.user_id),
                api_key.id,
                Some(ada.user_id),
                None,
                None,
                Some(fast_model.id),
                "gpt-4o-mini",
                UsagePricingStatus::Priced,
                20_000,
                ada_bucket_one,
            ),
            build_usage_ledger_record(
                "leaderboard-ada-reasoning",
                format!("user:{}", ada.user_id),
                api_key.id,
                Some(ada.user_id),
                None,
                None,
                Some(reasoning_model.id),
                "gpt-5",
                UsagePricingStatus::Priced,
                10_000,
                ada_bucket_two,
            ),
            build_usage_ledger_record(
                "leaderboard-ada-unpriced",
                format!("user:{}", ada.user_id),
                api_key.id,
                Some(ada.user_id),
                None,
                None,
                Some(fast_model.id),
                "gpt-4o-mini",
                UsagePricingStatus::Unpriced,
                0,
                ada_bucket_two + Duration::hours(1),
            ),
            build_usage_ledger_record(
                "leaderboard-ben-fast",
                format!("user:{}", ben.user_id),
                api_key.id,
                Some(ben.user_id),
                None,
                None,
                Some(fast_model.id),
                "gpt-4o-mini",
                UsagePricingStatus::Priced,
                29_000,
                ben_bucket,
            ),
            build_usage_ledger_record(
                "leaderboard-ben-reasoning",
                format!("user:{}", ben.user_id),
                api_key.id,
                Some(ben.user_id),
                None,
                None,
                Some(reasoning_model.id),
                "gpt-5",
                UsagePricingStatus::Priced,
                1_000,
                ben_bucket_same,
            ),
            build_usage_ledger_record(
                "leaderboard-cleo-fast",
                format!("user:{}", cleo.user_id),
                api_key.id,
                Some(cleo.user_id),
                None,
                None,
                Some(fast_model.id),
                "gpt-4o-mini",
                UsagePricingStatus::Priced,
                15_000,
                cleo_bucket,
            ),
        ] {
            assert!(
                store
                    .insert_usage_ledger_if_absent(&event)
                    .await
                    .expect("insert leaderboard event")
            );
        }

        for log in [
            RequestLogRecord {
                request_log_id: Uuid::new_v4(),
                request_id: "tool-ada-zero".to_string(),
                api_key_id: api_key.id,
                user_id: Some(ada.user_id),
                team_id: None,
                service_account_id: None,
                model_key: "fast".to_string(),
                resolved_model_key: "fast".to_string(),
                provider_key: "openai-prod".to_string(),
                status_code: Some(200),
                latency_ms: Some(20),
                prompt_tokens: Some(1),
                completion_tokens: Some(1),
                total_tokens: Some(2),
                error_code: None,
                has_payload: false,
                request_payload_truncated: false,
                response_payload_truncated: false,
                request_tags: RequestTags::default(),
                tool_cardinality: RequestToolCardinality {
                    referenced_mcp_server_count: None,
                    exposed_tool_count: Some(0),
                    invoked_tool_count: Some(0),
                    filtered_tool_count: None,
                },
                user_agent_raw: None,
                agent_harness_key: "unknown".to_string(),
                agent_harness_label: "Unknown".to_string(),
                metadata: Map::new(),
                occurred_at: ada_bucket_one,
            },
            RequestLogRecord {
                request_log_id: Uuid::new_v4(),
                request_id: "tool-ada-three".to_string(),
                api_key_id: api_key.id,
                user_id: Some(ada.user_id),
                team_id: None,
                service_account_id: None,
                model_key: "fast".to_string(),
                resolved_model_key: "fast".to_string(),
                provider_key: "openai-prod".to_string(),
                status_code: Some(200),
                latency_ms: Some(20),
                prompt_tokens: Some(1),
                completion_tokens: Some(1),
                total_tokens: Some(2),
                error_code: None,
                has_payload: false,
                request_payload_truncated: false,
                response_payload_truncated: false,
                request_tags: RequestTags::default(),
                tool_cardinality: RequestToolCardinality {
                    referenced_mcp_server_count: None,
                    exposed_tool_count: Some(3),
                    invoked_tool_count: Some(1),
                    filtered_tool_count: None,
                },
                user_agent_raw: None,
                agent_harness_key: "unknown".to_string(),
                agent_harness_label: "Unknown".to_string(),
                metadata: Map::new(),
                occurred_at: ada_bucket_two,
            },
            RequestLogRecord {
                request_log_id: Uuid::new_v4(),
                request_id: "tool-ada-historical-null".to_string(),
                api_key_id: api_key.id,
                user_id: Some(ada.user_id),
                team_id: None,
                service_account_id: None,
                model_key: "fast".to_string(),
                resolved_model_key: "fast".to_string(),
                provider_key: "openai-prod".to_string(),
                status_code: Some(200),
                latency_ms: Some(20),
                prompt_tokens: Some(1),
                completion_tokens: Some(1),
                total_tokens: Some(2),
                error_code: None,
                has_payload: false,
                request_payload_truncated: false,
                response_payload_truncated: false,
                request_tags: RequestTags::default(),
                tool_cardinality: RequestToolCardinality::default(),
                user_agent_raw: None,
                agent_harness_key: "unknown".to_string(),
                agent_harness_label: "Unknown".to_string(),
                metadata: Map::new(),
                occurred_at: ada_bucket_two,
            },
        ] {
            store
                .insert_request_log(&log, None)
                .await
                .expect("insert request log for leaderboard tool averages");
        }

        let leaders = store
            .list_usage_user_leaderboard(window_start, window_end, 30)
            .await
            .expect("list usage leaderboard");
        assert_eq!(leaders.len(), 3);
        assert_eq!(leaders[0].user_name, "Ada");
        assert_eq!(leaders[0].priced_cost_usd.as_scaled_i64(), 30_000);
        assert_eq!(leaders[0].total_request_count, 3);
        assert_eq!(leaders[0].top_model_key.as_deref(), Some("fast"));
        assert_eq!(
            leaders[0].tool_cardinality_averages.exposed_tool_count,
            Some(1.5)
        );
        assert_eq!(
            leaders[0].tool_cardinality_averages.invoked_tool_count,
            Some(0.5)
        );
        assert_eq!(
            leaders[0]
                .tool_cardinality_averages
                .referenced_mcp_server_count,
            None
        );
        assert_eq!(leaders[1].user_name, "Ben");
        assert_eq!(leaders[1].priced_cost_usd.as_scaled_i64(), 30_000);
        assert_eq!(leaders[1].total_request_count, 2);
        assert_eq!(leaders[1].top_model_key.as_deref(), Some("fast"));
        assert_eq!(leaders[2].user_name, "Cleo");

        let bucket_rows = store
            .list_usage_user_bucket_aggregates(
                window_start,
                window_end,
                12,
                &[ada.user_id, ben.user_id],
            )
            .await
            .expect("list usage bucket aggregates");
        assert_eq!(bucket_rows.len(), 3);
        assert_eq!(bucket_rows[0].user_id, ada.user_id);
        assert_eq!(
            bucket_rows[0].bucket_start,
            ada_bucket_one
                .replace_hour(0)
                .expect("bucket hour")
                .replace_minute(0)
                .expect("bucket minute")
                .replace_second(0)
                .expect("bucket second")
        );
        assert_eq!(bucket_rows[0].priced_cost_usd.as_scaled_i64(), 20_000);
        assert_eq!(bucket_rows[1].user_id, ada.user_id);
        assert_eq!(
            bucket_rows[1].bucket_start,
            ada_bucket_two
                .replace_hour(12)
                .expect("bucket hour")
                .replace_minute(0)
                .expect("bucket minute")
                .replace_second(0)
                .expect("bucket second")
        );
        assert_eq!(bucket_rows[1].priced_cost_usd.as_scaled_i64(), 10_000);
        assert_eq!(bucket_rows[2].user_id, ben.user_id);
        assert_eq!(
            bucket_rows[2].bucket_start,
            ben_bucket
                .replace_hour(0)
                .expect("bucket hour")
                .replace_minute(0)
                .expect("bucket minute")
                .replace_second(0)
                .expect("bucket second")
        );
        assert_eq!(bucket_rows[2].priced_cost_usd.as_scaled_i64(), 30_000);
    }

    async fn insert_libsql_oidc_provider(store: &LibsqlStore, provider_key: &str) -> String {
        let provider_id = format!("oidc-{provider_key}");
        let now = OffsetDateTime::now_utc().unix_timestamp();
        store
            .connection()
            .execute(
                r#"
                INSERT INTO oidc_providers (
                    oidc_provider_id, provider_key, provider_type, issuer_url, client_id,
                    client_secret_ref, scopes_json, enabled, created_at, updated_at
                ) VALUES (?1, ?2, 'generic_oidc', 'https://id.example.com', 'client-id',
                          'env.OIDC_CLIENT_SECRET', '["openid","email","profile"]', 1, ?3, ?3)
                "#,
                libsql::params![provider_id.as_str(), provider_key, now],
            )
            .await
            .expect("insert oidc provider");
        provider_id
    }

    async fn insert_postgres_oidc_provider(store: &PostgresStore, provider_key: &str) -> String {
        let provider_id = format!("oidc-{provider_key}");
        let now = OffsetDateTime::now_utc().unix_timestamp();
        sqlx::query(
            r#"
            INSERT INTO oidc_providers (
                oidc_provider_id, provider_key, provider_type, issuer_url, client_id,
                client_secret_ref, scopes_json, enabled, created_at, updated_at
            ) VALUES ($1, $2, 'generic_oidc', 'https://id.example.com', 'client-id',
                      'env.OIDC_CLIENT_SECRET', '["openid","email","profile"]', 1, $3, $3)
            "#,
        )
        .bind(provider_id.as_str())
        .bind(provider_key)
        .bind(now)
        .execute(store.pool())
        .await
        .expect("insert oidc provider");
        provider_id
    }

    #[tokio::test]
    #[serial]
    async fn libsql_oidc_login_state_consumes_once() {
        let tmp = tempdir().expect("tempdir");
        let db_path = tmp.path().join("gateway.db");
        run_migrations(&db_path).await.expect("migrations");
        let store = LibsqlStore::new_local(db_path.to_str().expect("db path"))
            .await
            .expect("store");
        let provider_id = insert_libsql_oidc_provider(&store, "authentik").await;
        let now = OffsetDateTime::now_utc();
        let state = OidcLoginStateRecord {
            state_hash: "state-hash".to_string(),
            oidc_provider_id: provider_id.clone(),
            nonce: "nonce".to_string(),
            pkce_verifier: "verifier".to_string(),
            redirect_to: "/admin".to_string(),
            login_hint: Some("sso-user@example.com".to_string()),
            expires_at: now + Duration::minutes(10),
            consumed_at: None,
            created_at: now,
        };

        store
            .create_oidc_login_state(&state)
            .await
            .expect("create login state");

        let consumed = store
            .consume_oidc_login_state("state-hash", now + Duration::seconds(1))
            .await
            .expect("consume state")
            .expect("state exists");
        assert_eq!(consumed.oidc_provider_id, provider_id);
        assert_eq!(consumed.redirect_to, "/admin");
        assert!(consumed.consumed_at.is_some());

        let reused = store
            .consume_oidc_login_state("state-hash", now + Duration::seconds(2))
            .await
            .expect("consume reused state");
        assert!(reused.is_none());
    }

    #[tokio::test]
    #[serial]
    async fn postgres_oidc_login_state_consumes_once() {
        let Some(test_db) = create_postgres_test_database().await else {
            eprintln!(
                "skipping postgres oidc login state test because TEST_POSTGRES_URL is not set"
            );
            return;
        };

        let options = StoreConnectionOptions::Postgres {
            url: test_db.database_url.clone(),
            max_connections: 4,
        };
        run_migrations_with_options(&options)
            .await
            .expect("postgres migrations");

        let store = PostgresStore::connect(&test_db.database_url, 4)
            .await
            .expect("postgres store");
        let provider_id = insert_postgres_oidc_provider(&store, "authentik").await;
        let now = OffsetDateTime::now_utc();
        let state = OidcLoginStateRecord {
            state_hash: "state-hash".to_string(),
            oidc_provider_id: provider_id.clone(),
            nonce: "nonce".to_string(),
            pkce_verifier: "verifier".to_string(),
            redirect_to: "/admin".to_string(),
            login_hint: Some("sso-user@example.com".to_string()),
            expires_at: now + Duration::minutes(10),
            consumed_at: None,
            created_at: now,
        };

        store
            .create_oidc_login_state(&state)
            .await
            .expect("create login state");

        let consumed = store
            .consume_oidc_login_state("state-hash", now + Duration::seconds(1))
            .await
            .expect("consume state")
            .expect("state exists");
        assert_eq!(consumed.oidc_provider_id, provider_id);
        assert_eq!(consumed.redirect_to, "/admin");
        assert!(consumed.consumed_at.is_some());

        let reused = store
            .consume_oidc_login_state("state-hash", now + Duration::seconds(2))
            .await
            .expect("consume reused state");
        assert!(reused.is_none());

        store.pool().close().await;
        drop_postgres_test_database(&test_db).await;
    }

    async fn exercise_budget_alert_repository<R>(repo: &R)
    where
        R: BudgetAlertRepository + Sync,
    {
        let now = OffsetDateTime::now_utc()
            .replace_nanosecond(0)
            .expect("zero nanos");
        let alert_one = BudgetAlertRecord {
            budget_alert_id: Uuid::new_v4(),
            ownership_scope_key: format!("user:{}", Uuid::new_v4()),
            owner_kind: ApiKeyOwnerKind::User,
            owner_id: Uuid::new_v4(),
            owner_name: "Member One".to_string(),
            budget_id: Uuid::new_v4(),
            cadence: BudgetCadence::Monthly,
            threshold_bps: 2_000,
            window_start: now - Duration::days(10),
            window_end: now + Duration::days(20),
            spend_before_usd: Money4::from_scaled(7_500_000),
            spend_after_usd: Money4::from_scaled(8_200_000),
            remaining_budget_usd: Money4::from_scaled(1_800_000),
            created_at: now - Duration::minutes(5),
            updated_at: now - Duration::minutes(5),
        };
        let pending_delivery = BudgetAlertDeliveryRecord {
            budget_alert_delivery_id: Uuid::new_v4(),
            budget_alert_id: alert_one.budget_alert_id,
            channel: BudgetAlertChannel::Email,
            delivery_status: BudgetAlertDeliveryStatus::Pending,
            recipient: Some("member.one@example.com".to_string()),
            provider_message_id: None,
            failure_reason: None,
            queued_at: alert_one.created_at,
            last_attempted_at: None,
            sent_at: None,
            updated_at: alert_one.created_at,
        };

        assert!(
            repo.create_budget_alert_with_deliveries(
                &alert_one,
                std::slice::from_ref(&pending_delivery),
            )
            .await
            .expect("insert first alert")
        );
        assert!(
            !repo
                .create_budget_alert_with_deliveries(
                    &alert_one,
                    std::slice::from_ref(&pending_delivery),
                )
                .await
                .expect("suppress duplicate alert")
        );

        let reconfigured_alert = BudgetAlertRecord {
            budget_alert_id: Uuid::new_v4(),
            ownership_scope_key: alert_one.ownership_scope_key.clone(),
            owner_kind: alert_one.owner_kind,
            owner_id: alert_one.owner_id,
            owner_name: alert_one.owner_name.clone(),
            budget_id: Uuid::new_v4(),
            cadence: BudgetCadence::Weekly,
            threshold_bps: alert_one.threshold_bps,
            window_start: alert_one.window_start,
            window_end: alert_one.window_end,
            spend_before_usd: alert_one.spend_before_usd,
            spend_after_usd: alert_one.spend_after_usd,
            remaining_budget_usd: alert_one.remaining_budget_usd,
            created_at: now - Duration::minutes(3),
            updated_at: now - Duration::minutes(3),
        };
        let reconfigured_delivery = BudgetAlertDeliveryRecord {
            budget_alert_delivery_id: Uuid::new_v4(),
            budget_alert_id: reconfigured_alert.budget_alert_id,
            channel: BudgetAlertChannel::Email,
            delivery_status: BudgetAlertDeliveryStatus::Failed,
            recipient: Some("member.one@example.com".to_string()),
            provider_message_id: None,
            failure_reason: Some("smtp timeout".to_string()),
            queued_at: reconfigured_alert.created_at,
            last_attempted_at: Some(reconfigured_alert.created_at + Duration::seconds(5)),
            sent_at: None,
            updated_at: reconfigured_alert.created_at + Duration::seconds(5),
        };
        assert!(
            repo.create_budget_alert_with_deliveries(
                &reconfigured_alert,
                std::slice::from_ref(&reconfigured_delivery),
            )
            .await
            .expect("allow alert for reconfigured budget in same window")
        );

        let alert_two = BudgetAlertRecord {
            budget_alert_id: Uuid::new_v4(),
            ownership_scope_key: format!("service_account:{}", Uuid::new_v4()),
            owner_kind: ApiKeyOwnerKind::ServiceAccount,
            owner_id: Uuid::new_v4(),
            owner_name: "Ops Automation".to_string(),
            budget_id: Uuid::new_v4(),
            cadence: BudgetCadence::Weekly,
            threshold_bps: 2_000,
            window_start: now - Duration::days(7),
            window_end: now,
            spend_before_usd: Money4::from_scaled(90_000),
            spend_after_usd: Money4::from_scaled(95_000),
            remaining_budget_usd: Money4::from_scaled(5_000),
            created_at: now - Duration::minutes(1),
            updated_at: now - Duration::minutes(1),
        };
        let failed_delivery = BudgetAlertDeliveryRecord {
            budget_alert_delivery_id: Uuid::new_v4(),
            budget_alert_id: alert_two.budget_alert_id,
            channel: BudgetAlertChannel::Email,
            delivery_status: BudgetAlertDeliveryStatus::Failed,
            recipient: Some("ops@example.com".to_string()),
            provider_message_id: None,
            failure_reason: Some("smtp timeout".to_string()),
            queued_at: alert_two.created_at,
            last_attempted_at: Some(alert_two.created_at + Duration::seconds(10)),
            sent_at: None,
            updated_at: alert_two.created_at + Duration::seconds(10),
        };
        assert!(
            repo.create_budget_alert_with_deliveries(
                &alert_two,
                std::slice::from_ref(&failed_delivery),
            )
            .await
            .expect("insert failed alert")
        );

        let page = repo
            .list_budget_alert_history(&BudgetAlertHistoryQuery {
                page: 1,
                page_size: 10,
                owner_kind: None,
                channel: None,
                delivery_status: None,
            })
            .await
            .expect("list all alert history");
        assert_eq!(page.total, 3);
        assert_eq!(page.items.len(), 3);
        assert_eq!(page.items[0].budget_alert_id, alert_two.budget_alert_id);
        assert_eq!(
            page.items[0].delivery_status,
            BudgetAlertDeliveryStatus::Failed
        );
        assert_eq!(page.items[0].recipient_summary, "ops@example.com");
        assert_eq!(
            page.items[0].failure_reason.as_deref(),
            Some("smtp timeout")
        );
        assert_eq!(
            page.items[1].budget_alert_id,
            reconfigured_alert.budget_alert_id
        );
        assert_eq!(page.items[1].cadence, BudgetCadence::Weekly);
        assert_eq!(page.items[2].budget_alert_id, alert_one.budget_alert_id);
        assert_eq!(page.items[2].cadence, BudgetCadence::Monthly);

        let service_account_only = repo
            .list_budget_alert_history(&BudgetAlertHistoryQuery {
                page: 1,
                page_size: 10,
                owner_kind: Some(ApiKeyOwnerKind::ServiceAccount),
                channel: Some(BudgetAlertChannel::Email),
                delivery_status: Some(BudgetAlertDeliveryStatus::Failed),
            })
            .await
            .expect("filter alert history");
        assert_eq!(service_account_only.total, 1);
        assert_eq!(
            service_account_only.items[0].budget_alert_id,
            alert_two.budget_alert_id
        );

        let claimed_at = now + Duration::minutes(1);
        let claimed = repo
            .claim_pending_budget_alert_delivery_tasks(10, claimed_at)
            .await
            .expect("claim pending deliveries");
        assert_eq!(claimed.len(), 1);
        assert_eq!(
            claimed[0].delivery.budget_alert_delivery_id,
            pending_delivery.budget_alert_delivery_id
        );
        assert_eq!(claimed[0].alert.budget_alert_id, alert_one.budget_alert_id);

        let claimed_again = repo
            .claim_pending_budget_alert_delivery_tasks(10, claimed_at + Duration::seconds(5))
            .await
            .expect("avoid reclaiming in-flight delivery");
        assert!(claimed_again.is_empty());

        let sent_at = claimed_at + Duration::seconds(30);
        repo.mark_budget_alert_delivery_sent(
            pending_delivery.budget_alert_delivery_id,
            Some("smtp-123"),
            sent_at,
        )
        .await
        .expect("mark delivery sent");

        let sent_page = repo
            .list_budget_alert_history(&BudgetAlertHistoryQuery {
                page: 1,
                page_size: 10,
                owner_kind: Some(ApiKeyOwnerKind::User),
                channel: Some(BudgetAlertChannel::Email),
                delivery_status: Some(BudgetAlertDeliveryStatus::Sent),
            })
            .await
            .expect("list sent alerts");
        assert_eq!(sent_page.total, 1);
        assert_eq!(
            sent_page.items[0].budget_alert_id,
            alert_one.budget_alert_id
        );
        assert_eq!(
            sent_page.items[0].delivery_status,
            BudgetAlertDeliveryStatus::Sent
        );
        assert_eq!(sent_page.items[0].last_attempted_at, Some(claimed_at));
        assert_eq!(sent_page.items[0].sent_at, Some(sent_at));
        assert_eq!(
            sent_page.items[0].recipient_summary,
            "member.one@example.com"
        );
    }

    #[tokio::test]
    #[serial]
    async fn migrations_apply_and_are_idempotent() {
        let tmp = tempdir().expect("tempdir");
        let db_path = tmp.path().join("gateway.db");

        run_migrations(&db_path)
            .await
            .expect("initial migration run");
        run_migrations(&db_path)
            .await
            .expect("idempotent migration run");

        let store = LibsqlStore::new_local(db_path.to_str().expect("db path"))
            .await
            .expect("store");

        store.ping().await.expect("ping");
    }

    #[tokio::test]
    #[serial]
    async fn migration_status_reports_pending_versions_before_first_apply() {
        let tmp = tempdir().expect("tempdir");
        let db_path = tmp.path().join("gateway.db");

        let status = status_migrations_with_options(&StoreConnectionOptions::Libsql {
            path: db_path.clone(),
        })
        .await
        .expect("status");

        assert_eq!(status.backend, "libsql");
        assert_eq!(status.pending_count(), MIGRATION_REGISTRY.len());
        assert!(status.entries.iter().all(|entry| !entry.applied));
    }

    #[tokio::test]
    #[serial]
    async fn migration_check_fails_when_pending_versions_exist() {
        let tmp = tempdir().expect("tempdir");
        let db_path = tmp.path().join("gateway.db");

        let error =
            check_migrations_with_options(&StoreConnectionOptions::Libsql { path: db_path })
                .await
                .expect_err("check should fail");
        assert!(error.to_string().contains("pending migrations"));
    }

    #[tokio::test]
    #[serial]
    async fn libsql_migration_commands_reject_legacy_history() {
        let tmp = tempdir().expect("tempdir");
        let db_path = tmp.path().join("gateway.db");
        let options = StoreConnectionOptions::Libsql {
            path: db_path.clone(),
        };

        insert_libsql_history_entry(&db_path, 1, "init", "V1__init.sql")
            .await
            .expect("legacy history row");

        assert_database_reset_required(
            status_migrations_with_options(&options)
                .await
                .expect_err("status should reject legacy history"),
        );
        assert_database_reset_required(
            check_migrations_with_options(&options)
                .await
                .expect_err("check should reject legacy history"),
        );
        assert_database_reset_required(
            run_migrations_with_options(&options)
                .await
                .expect_err("apply should reject legacy history"),
        );
    }

    #[tokio::test]
    #[serial]
    async fn libsql_migration_commands_reject_empty_history_when_app_tables_exist() {
        let tmp = tempdir().expect("tempdir");
        let db_path = tmp.path().join("gateway.db");
        let options = StoreConnectionOptions::Libsql {
            path: db_path.clone(),
        };

        insert_libsql_application_table_without_history(&db_path)
            .await
            .expect("application table");

        assert_database_reset_required(
            status_migrations_with_options(&options)
                .await
                .expect_err("status should reject empty history with app tables"),
        );
        assert_database_reset_required(
            check_migrations_with_options(&options)
                .await
                .expect_err("check should reject empty history with app tables"),
        );
        assert_database_reset_required(
            run_migrations_with_options(&options)
                .await
                .expect_err("apply should reject empty history with app tables"),
        );
    }

    #[tokio::test]
    #[serial]
    async fn libsql_migration_status_rejects_manifest_identity_mismatch() {
        let tmp = tempdir().expect("tempdir");
        let db_path = tmp.path().join("gateway.db");

        insert_libsql_history_entry(
            &db_path,
            i64::from(MIGRATION_REGISTRY[0].version),
            MIGRATION_REGISTRY[0].name,
            "unexpected-checksum.sql",
        )
        .await
        .expect("mismatched history row");

        let error =
            status_migrations_with_options(&StoreConnectionOptions::Libsql { path: db_path })
                .await
                .expect_err("status should reject manifest mismatch");
        assert_database_reset_required(error);
    }

    #[tokio::test]
    #[serial]
    async fn migrations_rollback_when_history_write_fails() {
        let tmp = tempdir().expect("tempdir");
        let db_path = tmp.path().join("gateway.db");
        let baseline_version = MIGRATION_REGISTRY[0].version;

        run_migrations_with_options_for_test(
            &StoreConnectionOptions::Libsql {
                path: db_path.clone(),
            },
            MigrationTestHook {
                fail_after_apply_version: Some(baseline_version),
                ..MigrationTestHook::default()
            },
        )
        .await
        .expect_err("migration should fail");

        let db = libsql::Builder::new_local(db_path.to_str().expect("db path"))
            .build()
            .await
            .expect("db");
        let conn = db.connect().expect("connection");

        let mut history_rows = conn
            .query("SELECT COUNT(*) FROM refinery_schema_history", ())
            .await
            .expect("history count query");
        let history_row = history_rows
            .next()
            .await
            .expect("history row fetch")
            .expect("history row");
        let history_count: i64 = history_row.get(0).expect("history count");
        assert_eq!(history_count, 0);

        let mut table_rows = conn
            .query(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'providers'",
                (),
            )
            .await
            .expect("providers table query");
        let table_row = table_rows
            .next()
            .await
            .expect("providers row fetch")
            .expect("providers row");
        let table_count: i64 = table_row.get(0).expect("providers count");
        assert_eq!(table_count, 0);
    }

    #[tokio::test]
    #[serial]
    async fn migrations_rollback_when_schema_history_insert_fails() {
        let tmp = tempdir().expect("tempdir");
        let db_path = tmp.path().join("gateway.db");
        let baseline_version = MIGRATION_REGISTRY[0].version;

        run_migrations_with_options_for_test(
            &StoreConnectionOptions::Libsql {
                path: db_path.clone(),
            },
            MigrationTestHook {
                fail_history_insert_version: Some(baseline_version),
                ..MigrationTestHook::default()
            },
        )
        .await
        .expect_err("migration should fail when history insert fails");

        let db = libsql::Builder::new_local(db_path.to_str().expect("db path"))
            .build()
            .await
            .expect("db");
        let conn = db.connect().expect("connection");

        let mut history_rows = conn
            .query("SELECT COUNT(*) FROM refinery_schema_history", ())
            .await
            .expect("history count query");
        let history_row = history_rows
            .next()
            .await
            .expect("history row fetch")
            .expect("history row");
        let history_count: i64 = history_row.get(0).expect("history count");
        assert_eq!(history_count, 0);

        let mut table_rows = conn
            .query(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'providers'",
                (),
            )
            .await
            .expect("providers table query");
        let table_row = table_rows
            .next()
            .await
            .expect("providers row fetch")
            .expect("providers row");
        let table_count: i64 = table_row.get(0).expect("providers count");
        assert_eq!(table_count, 0);
    }

    #[tokio::test]
    #[serial]
    async fn libsql_migration_status_recovers_after_failure_and_retry() {
        let tmp = tempdir().expect("tempdir");
        let db_path = tmp.path().join("gateway.db");
        let options = StoreConnectionOptions::Libsql {
            path: db_path.clone(),
        };
        let baseline_version = MIGRATION_REGISTRY[0].version;

        let initial_status = status_migrations_with_options(&options)
            .await
            .expect("initial migration status");
        assert_eq!(initial_status.backend, "libsql");
        assert_eq!(initial_status.pending_count(), MIGRATION_REGISTRY.len());
        assert!(initial_status.entries.iter().all(|entry| !entry.applied));

        run_migrations_with_options_for_test(
            &options,
            MigrationTestHook {
                fail_history_insert_version: Some(baseline_version),
                ..MigrationTestHook::default()
            },
        )
        .await
        .expect_err("migration should fail when history insert fails");

        let failed_status = status_migrations_with_options(&options)
            .await
            .expect("status after failed migration");
        assert_eq!(failed_status.pending_count(), MIGRATION_REGISTRY.len());
        assert!(failed_status.entries.iter().all(|entry| !entry.applied));

        run_migrations_with_options(&options)
            .await
            .expect("retry migrations");

        let applied_status = status_migrations_with_options(&options)
            .await
            .expect("status after retry");
        assert_eq!(applied_status.pending_count(), 0);
        assert!(applied_status.entries.iter().all(|entry| entry.applied));
    }

    #[tokio::test]
    #[serial]
    async fn seeding_is_idempotent_and_queries_return_expected_records() {
        let tmp = tempdir().expect("tempdir");
        let db_path = tmp.path().join("gateway.db");
        run_migrations(&db_path).await.expect("migrations");

        let store = LibsqlStore::new_local(db_path.to_str().expect("db path"))
            .await
            .expect("store");

        let providers = vec![SeedProvider {
            provider_key: "openai-prod".to_string(),
            provider_type: "openai_compat".to_string(),
            config: json!({
                "base_url": "https://api.openai.com/v1",
                "timeout_ms": 120_000
            }),
            secrets: Some(json!({"token": "env.OPENAI_API_KEY"})),
        }];

        let models = vec![SeedModel {
            model_key: "fast".to_string(),
            alias_target_model_key: None,
            description: Some("fast tier".to_string()),
            tags: vec!["fast".to_string(), "cheap".to_string()],
            rank: 10,
            routes: vec![SeedModelRoute {
                provider_key: "openai-prod".to_string(),
                upstream_model: "gpt-4o-mini".to_string(),
                priority: 10,
                weight: 1.0,
                enabled: true,
                extra_headers: Map::new(),
                extra_body: Map::new(),
                capabilities: ProviderCapabilities::with_dimensions(
                    true, false, true, false, false, true, true,
                ),
                compatibility: RouteCompatibility {
                    openai_compat: Some(OpenAiCompatRouteCompatibility {
                        supports_store: false,
                        max_tokens_field: OpenAiCompatMaxTokensField::MaxTokens,
                        developer_role: OpenAiCompatDeveloperRole::System,
                        reasoning_effort: OpenAiCompatReasoningEffort::ReasoningObject,
                        supports_stream_usage: true,
                    }),
                    ..Default::default()
                },
            }],
        }];

        let api_keys = vec![SeedApiKey {
            name: "dev".to_string(),
            public_id: "dev123".to_string(),
            secret_hash: "$argon2id$v=19$m=19456,t=2,p=1$8WJ6UydAx2RbDXy+zuYbAw$EF+rEtkc71VhwwvS+TS6EiZZvW6rtrjzXX4XvIsDhbU".to_string(),
            service_account_key: "seed-workloads".to_string(),
            service_account_name: "Seed Workloads".to_string(),
            service_account_team_key: "seed-workloads".to_string(),
            service_account_budget: SeedBudget {
                cadence: BudgetCadence::Daily,
                amount_usd: Money4::from_scaled(100_000),
                hard_limit: true,
                timezone: "UTC".to_string(),
            },
            allowed_models: vec!["fast".to_string()],
        }];

        store
            .seed_from_inputs(
                &providers,
                &models,
                &api_keys,
                &[],
                &[],
                &seed_api_key_teams(),
                &[],
            )
            .await
            .expect("seed #1");

        store
            .seed_from_inputs(
                &providers,
                &models,
                &api_keys,
                &[],
                &[],
                &seed_api_key_teams(),
                &[],
            )
            .await
            .expect("seed #2 idempotent");

        let api_key = store
            .get_api_key_by_public_id("dev123")
            .await
            .expect("query key")
            .expect("api key should exist");
        let seed_team_id = store
            .get_team_by_key("seed-workloads")
            .await
            .expect("load seed team")
            .expect("seed team")
            .team_id;
        let seed_service_account_id =
            Uuid::new_v5(&Uuid::NAMESPACE_OID, b"service_account:seed-workloads");
        assert_eq!(api_key.owner_kind, ApiKeyOwnerKind::ServiceAccount);
        assert_eq!(api_key.owner_team_id, Some(seed_team_id));
        assert_eq!(
            api_key.owner_service_account_id,
            Some(seed_service_account_id)
        );
        assert_eq!(api_key.owner_user_id, None);

        let accessible_models = store
            .list_models_for_api_key(api_key.id)
            .await
            .expect("models by key");
        assert_eq!(accessible_models.len(), 1);
        assert_eq!(accessible_models[0].model_key, "fast");

        let routes = store
            .list_routes_for_model(accessible_models[0].id)
            .await
            .expect("model routes");
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].provider_key, "openai-prod");
        assert!(!routes[0].capabilities.stream);
        assert!(!routes[0].capabilities.tools);
        assert!(!routes[0].capabilities.vision);
        let profile = routes[0]
            .compatibility
            .openai_compat
            .as_ref()
            .expect("openai compatibility profile");
        assert!(!profile.supports_store);
        assert_eq!(
            profile.max_tokens_field,
            OpenAiCompatMaxTokensField::MaxTokens
        );
        assert_eq!(profile.developer_role, OpenAiCompatDeveloperRole::System);
        assert_eq!(
            profile.reasoning_effort,
            OpenAiCompatReasoningEffort::ReasoningObject
        );
        assert!(profile.supports_stream_usage);

        let occurred_at = OffsetDateTime::now_utc();
        let zero_counts_log_id = Uuid::new_v4();
        let zero_counts_log = RequestLogRecord {
            request_log_id: zero_counts_log_id,
            request_id: "req-zero-tools".to_string(),
            api_key_id: api_key.id,
            user_id: None,
            team_id: api_key.owner_team_id,
            service_account_id: None,
            model_key: "fast".to_string(),
            resolved_model_key: "fast".to_string(),
            provider_key: "openai-prod".to_string(),
            status_code: Some(200),
            latency_ms: Some(42),
            prompt_tokens: Some(1),
            completion_tokens: Some(2),
            total_tokens: Some(3),
            error_code: None,
            has_payload: false,
            request_payload_truncated: false,
            response_payload_truncated: false,
            request_tags: RequestTags::default(),
            tool_cardinality: RequestToolCardinality {
                referenced_mcp_server_count: None,
                exposed_tool_count: Some(0),
                invoked_tool_count: Some(0),
                filtered_tool_count: None,
            },
            user_agent_raw: Some("opencode/1.2.3".to_string()),
            agent_harness_key: "opencode".to_string(),
            agent_harness_label: "Opencode".to_string(),
            metadata: Map::new(),
            occurred_at,
        };
        let null_counts_log = RequestLogRecord {
            request_log_id: Uuid::new_v4(),
            request_id: "req-null-tools".to_string(),
            tool_cardinality: RequestToolCardinality::default(),
            occurred_at: occurred_at - Duration::seconds(1),
            ..zero_counts_log.clone()
        };
        let changed_label_log = RequestLogRecord {
            request_log_id: Uuid::new_v4(),
            request_id: "req-changed-harness-label".to_string(),
            agent_harness_label: "Zpencode".to_string(),
            occurred_at: occurred_at - Duration::seconds(2),
            ..zero_counts_log.clone()
        };

        store
            .insert_request_log(&zero_counts_log, None)
            .await
            .expect("insert zero-count request log");
        store
            .insert_request_log(&null_counts_log, None)
            .await
            .expect("insert null-count request log");
        store
            .insert_request_log(&changed_label_log, None)
            .await
            .expect("insert changed-label request log");

        let page = store
            .list_request_logs(&RequestLogQuery {
                page: 1,
                page_size: 10,
                request_id: None,
                model_key: None,
                provider_key: None,
                status_code: None,
                user_id: None,
                team_id: None,
                service_account_id: None,
                service: None,
                component: None,
                env: None,
                tag_key: None,
                tag_value: None,
            })
            .await
            .expect("list request logs");
        let zero_summary = page
            .items
            .iter()
            .find(|log| log.request_log_id == zero_counts_log_id)
            .expect("zero count log in list");
        assert_eq!(zero_summary.tool_cardinality.exposed_tool_count, Some(0));
        assert_eq!(zero_summary.tool_cardinality.invoked_tool_count, Some(0));
        assert_eq!(
            zero_summary.user_agent_raw.as_deref(),
            Some("opencode/1.2.3")
        );
        assert_eq!(zero_summary.agent_harness_key, "opencode");
        assert_eq!(zero_summary.agent_harness_label, "Opencode");
        assert_eq!(
            zero_summary.tool_cardinality.referenced_mcp_server_count,
            None
        );
        let detail = store
            .get_request_log_detail(zero_counts_log_id)
            .await
            .expect("request log detail");
        assert_eq!(detail.log.tool_cardinality.exposed_tool_count, Some(0));
        assert_eq!(detail.log.tool_cardinality.filtered_tool_count, None);
        assert_eq!(detail.log.user_agent_raw.as_deref(), Some("opencode/1.2.3"));
        assert_eq!(detail.log.agent_harness_key, "opencode");
        assert_eq!(detail.log.agent_harness_label, "Opencode");

        let harness_leaders = store
            .list_harness_usage_leaders(
                occurred_at - Duration::hours(1),
                occurred_at + Duration::hours(1),
                10,
            )
            .await
            .expect("harness leaders");
        assert_eq!(harness_leaders.len(), 1);
        assert_eq!(harness_leaders[0].agent_harness_key, "opencode");
        assert_eq!(harness_leaders[0].agent_harness_label, "Opencode");
        assert_eq!(harness_leaders[0].request_count, 3);

        let harness_buckets = store
            .list_harness_usage_bucket_aggregates(
                occurred_at - Duration::hours(1),
                occurred_at + Duration::hours(1),
                12,
                &["opencode".to_string()],
            )
            .await
            .expect("harness bucket aggregates");
        assert_eq!(harness_buckets.len(), 1);
        assert_eq!(harness_buckets[0].agent_harness_key, "opencode");
        assert_eq!(harness_buckets[0].request_count, 3);
    }

    #[tokio::test]
    #[serial]
    async fn libsql_mcp_tool_invocations_round_trip_and_filter() {
        let tmp = tempdir().expect("tempdir");
        let db_path = tmp.path().join("gateway.db");
        run_migrations(&db_path).await.expect("migrations");

        let store = LibsqlStore::new_local(db_path.to_str().expect("db path"))
            .await
            .expect("store");

        exercise_mcp_tool_invocation_repository(&store).await;
    }

    #[tokio::test]
    #[serial]
    async fn libsql_external_mcp_registry_round_trip_and_rediscovery() {
        let tmp = tempdir().expect("tempdir");
        let db_path = tmp.path().join("gateway.db");
        run_migrations(&db_path).await.expect("migrations");

        let store = LibsqlStore::new_local(db_path.to_str().expect("db path"))
            .await
            .expect("store");

        exercise_mcp_registry_repository(&store).await;
    }

    #[tokio::test]
    #[serial]
    async fn libsql_mcp_upstream_credentials_round_trip_and_revoke() {
        let tmp = tempdir().expect("tempdir");
        let db_path = tmp.path().join("gateway.db");
        run_migrations(&db_path).await.expect("migrations");

        let store = LibsqlStore::new_local(db_path.to_str().expect("db path"))
            .await
            .expect("store");

        exercise_mcp_upstream_credential_repository(&store).await;
    }

    #[tokio::test]
    #[serial]
    async fn libsql_request_log_purge_supports_dry_run_and_cascades_children() {
        let tmp = tempdir().expect("tempdir");
        let db_path = tmp.path().join("gateway.db");
        run_migrations(&db_path).await.expect("migrations");

        let store = LibsqlStore::new_local(db_path.to_str().expect("db path"))
            .await
            .expect("store");
        store
            .seed_from_inputs(&[SeedProvider {
                    provider_key: "openai-prod".to_string(),
                    provider_type: "openai_compat".to_string(),
                    config: json!({}),
                    secrets: None,
                }], &[SeedModel {
                    model_key: "fast".to_string(),
                    alias_target_model_key: None,
                    description: None,
                    tags: Vec::new(),
                    rank: 10,
                    routes: Vec::new(),
                }], &[SeedApiKey {
                    name: "dev".to_string(),
                    public_id: "dev123".to_string(),
                    secret_hash: "$argon2id$v=19$m=19456,t=2,p=1$8WJ6UydAx2RbDXy+zuYbAw$EF+rEtkc71VhwwvS+TS6EiZZvW6rtrjzXX4XvIsDhbU".to_string(),
            service_account_key: "seed-workloads".to_string(),
            service_account_name: "Seed Workloads".to_string(),
            service_account_team_key: "seed-workloads".to_string(),
            service_account_budget: SeedBudget {
                cadence: BudgetCadence::Daily,
                amount_usd: Money4::from_scaled(100_000),
                hard_limit: true,
                timezone: "UTC".to_string(),
            },
                    allowed_models: vec!["fast".to_string()],
                }], &[], &[], &seed_api_key_teams(), &[])
            .await
            .expect("seed");
        let api_key = store
            .get_api_key_by_public_id("dev123")
            .await
            .expect("query key")
            .expect("api key should exist");

        let now = OffsetDateTime::now_utc();
        let old_log = RequestLogRecord {
            request_log_id: Uuid::new_v4(),
            request_id: "req-old".to_string(),
            api_key_id: api_key.id,
            user_id: None,
            team_id: api_key.owner_team_id,
            service_account_id: None,
            model_key: "fast".to_string(),
            resolved_model_key: "fast".to_string(),
            provider_key: "openai-prod".to_string(),
            status_code: Some(200),
            latency_ms: Some(42),
            prompt_tokens: Some(1),
            completion_tokens: Some(2),
            total_tokens: Some(3),
            error_code: None,
            has_payload: true,
            request_payload_truncated: false,
            response_payload_truncated: false,
            request_tags: RequestTags {
                service: Some("svc".to_string()),
                component: None,
                env: None,
                bespoke: vec![RequestTag {
                    key: "tenant".to_string(),
                    value: "alpha".to_string(),
                }],
            },
            tool_cardinality: RequestToolCardinality::default(),
            user_agent_raw: None,
            agent_harness_key: "unknown".to_string(),
            agent_harness_label: "Unknown".to_string(),
            metadata: Map::new(),
            occurred_at: now - Duration::days(4),
        };
        let young_log = RequestLogRecord {
            request_log_id: Uuid::new_v4(),
            request_id: "req-young".to_string(),
            has_payload: false,
            request_tags: RequestTags::default(),
            occurred_at: now,
            ..old_log.clone()
        };
        let payload = RequestLogPayloadRecord {
            request_log_id: old_log.request_log_id,
            request_json: json!({"prompt": "old"}),
            response_json: json!({"ok": true}),
        };
        let attempt = RequestAttemptRecord {
            request_attempt_id: Uuid::new_v4(),
            request_log_id: old_log.request_log_id,
            request_id: old_log.request_id.clone(),
            attempt_number: 1,
            route_id: Uuid::new_v4(),
            provider_key: "openai-prod".to_string(),
            upstream_model: "gpt-4o-mini".to_string(),
            status: RequestAttemptStatus::Success,
            status_code: Some(200),
            error_code: None,
            error_detail: None,
            error_detail_truncated: false,
            retryable: false,
            terminal: true,
            produced_final_response: true,
            stream: false,
            started_at: old_log.occurred_at,
            completed_at: Some(old_log.occurred_at + Duration::milliseconds(42)),
            latency_ms: Some(42),
            metadata: Map::new(),
        };

        store
            .insert_request_log_with_attempts(&old_log, Some(&payload), &[attempt])
            .await
            .expect("insert old request log");
        store
            .insert_request_log(&young_log, None)
            .await
            .expect("insert young request log");

        let cutoff = now - Duration::days(3);
        let dry_run = store
            .purge_request_logs_older_than(cutoff, true)
            .await
            .expect("dry run purge");
        assert_eq!(dry_run.matched_count, 1);
        assert_eq!(dry_run.deleted_count, 0);
        assert!(
            store
                .get_request_log_detail(old_log.request_log_id)
                .await
                .is_ok()
        );

        let purge = store
            .purge_request_logs_older_than(cutoff, false)
            .await
            .expect("purge old request logs");
        assert_eq!(purge.matched_count, 1);
        assert_eq!(purge.deleted_count, 1);
        assert!(
            store
                .get_request_log_detail(old_log.request_log_id)
                .await
                .is_err()
        );
        assert!(
            store
                .get_request_log_detail(young_log.request_log_id)
                .await
                .is_ok()
        );

        let db = libsql::Builder::new_local(db_path)
            .build()
            .await
            .expect("libsql db");
        let connection = db.connect().expect("libsql connection");
        let mut rows = connection
            .query(
                r#"
                SELECT
                    (SELECT COUNT(*) FROM request_log_payloads),
                    (SELECT COUNT(*) FROM request_log_tags),
                    (SELECT COUNT(*) FROM request_log_attempts)
                "#,
                (),
            )
            .await
            .expect("child counts");
        let row = rows
            .next()
            .await
            .expect("child count row")
            .expect("child count row exists");
        let payload_count: i64 = row.get(0).expect("payload count");
        let tag_count: i64 = row.get(1).expect("tag count");
        let attempt_count: i64 = row.get(2).expect("attempt count");
        assert_eq!((payload_count, tag_count, attempt_count), (0, 0, 0));
    }

    #[tokio::test]
    #[serial]
    async fn postgres_request_log_purge_supports_dry_run_and_cascades_children() {
        let Some(test_db) = create_postgres_test_database().await else {
            eprintln!(
                "skipping postgres request log purge test because TEST_POSTGRES_URL is not set"
            );
            return;
        };

        let options = StoreConnectionOptions::Postgres {
            url: test_db.database_url.clone(),
            max_connections: 4,
        };
        run_migrations_with_options(&options)
            .await
            .expect("postgres migrations");

        let store = PostgresStore::connect(&test_db.database_url, 4)
            .await
            .expect("postgres store");
        store
            .seed_from_inputs(&[SeedProvider {
                    provider_key: "openai-prod".to_string(),
                    provider_type: "openai_compat".to_string(),
                    config: json!({}),
                    secrets: None,
                }], &[SeedModel {
                    model_key: "fast".to_string(),
                    alias_target_model_key: None,
                    description: None,
                    tags: Vec::new(),
                    rank: 10,
                    routes: Vec::new(),
                }], &[SeedApiKey {
                    name: "dev".to_string(),
                    public_id: "dev123".to_string(),
                    secret_hash: "$argon2id$v=19$m=19456,t=2,p=1$8WJ6UydAx2RbDXy+zuYbAw$EF+rEtkc71VhwwvS+TS6EiZZvW6rtrjzXX4XvIsDhbU".to_string(),
            service_account_key: "seed-workloads".to_string(),
            service_account_name: "Seed Workloads".to_string(),
            service_account_team_key: "seed-workloads".to_string(),
            service_account_budget: SeedBudget {
                cadence: BudgetCadence::Daily,
                amount_usd: Money4::from_scaled(100_000),
                hard_limit: true,
                timezone: "UTC".to_string(),
            },
                    allowed_models: vec!["fast".to_string()],
                }], &[], &[], &seed_api_key_teams(), &[])
            .await
            .expect("seed");
        let api_key = store
            .get_api_key_by_public_id("dev123")
            .await
            .expect("query key")
            .expect("api key should exist");

        let now = OffsetDateTime::now_utc();
        let old_log = RequestLogRecord {
            request_log_id: Uuid::new_v4(),
            request_id: "req-old-postgres".to_string(),
            api_key_id: api_key.id,
            user_id: None,
            team_id: api_key.owner_team_id,
            service_account_id: None,
            model_key: "fast".to_string(),
            resolved_model_key: "fast".to_string(),
            provider_key: "openai-prod".to_string(),
            status_code: Some(200),
            latency_ms: Some(42),
            prompt_tokens: Some(1),
            completion_tokens: Some(2),
            total_tokens: Some(3),
            error_code: None,
            has_payload: true,
            request_payload_truncated: false,
            response_payload_truncated: false,
            request_tags: RequestTags {
                service: Some("svc".to_string()),
                component: None,
                env: None,
                bespoke: vec![RequestTag {
                    key: "tenant".to_string(),
                    value: "alpha".to_string(),
                }],
            },
            tool_cardinality: RequestToolCardinality::default(),
            user_agent_raw: None,
            agent_harness_key: "unknown".to_string(),
            agent_harness_label: "Unknown".to_string(),
            metadata: Map::new(),
            occurred_at: now - Duration::days(4),
        };
        let young_log = RequestLogRecord {
            request_log_id: Uuid::new_v4(),
            request_id: "req-young-postgres".to_string(),
            has_payload: false,
            request_tags: RequestTags::default(),
            occurred_at: now,
            ..old_log.clone()
        };
        let payload = RequestLogPayloadRecord {
            request_log_id: old_log.request_log_id,
            request_json: json!({"prompt": "old"}),
            response_json: json!({"ok": true}),
        };
        let attempt = RequestAttemptRecord {
            request_attempt_id: Uuid::new_v4(),
            request_log_id: old_log.request_log_id,
            request_id: old_log.request_id.clone(),
            attempt_number: 1,
            route_id: Uuid::new_v4(),
            provider_key: "openai-prod".to_string(),
            upstream_model: "gpt-4o-mini".to_string(),
            status: RequestAttemptStatus::Success,
            status_code: Some(200),
            error_code: None,
            error_detail: None,
            error_detail_truncated: false,
            retryable: false,
            terminal: true,
            produced_final_response: true,
            stream: false,
            started_at: old_log.occurred_at,
            completed_at: Some(old_log.occurred_at + Duration::milliseconds(42)),
            latency_ms: Some(42),
            metadata: Map::new(),
        };

        store
            .insert_request_log_with_attempts(&old_log, Some(&payload), &[attempt])
            .await
            .expect("insert old request log");
        store
            .insert_request_log(&young_log, None)
            .await
            .expect("insert young request log");

        let cutoff = now - Duration::days(3);
        let dry_run = store
            .purge_request_logs_older_than(cutoff, true)
            .await
            .expect("dry run purge");
        assert_eq!(dry_run.matched_count, 1);
        assert_eq!(dry_run.deleted_count, 0);
        assert!(
            store
                .get_request_log_detail(old_log.request_log_id)
                .await
                .is_ok()
        );

        let purge = store
            .purge_request_logs_older_than(cutoff, false)
            .await
            .expect("purge old request logs");
        assert_eq!(purge.matched_count, 1);
        assert_eq!(purge.deleted_count, 1);
        assert!(
            store
                .get_request_log_detail(old_log.request_log_id)
                .await
                .is_err()
        );
        assert!(
            store
                .get_request_log_detail(young_log.request_log_id)
                .await
                .is_ok()
        );

        let pool = sqlx::PgPool::connect(&test_db.database_url)
            .await
            .expect("postgres pool");
        let row = sqlx::query(
            r#"
            SELECT
                (SELECT COUNT(*) FROM request_log_payloads),
                (SELECT COUNT(*) FROM request_log_tags),
                (SELECT COUNT(*) FROM request_log_attempts)
            "#,
        )
        .fetch_one(&pool)
        .await
        .expect("child counts");
        let payload_count: i64 = row.try_get(0).expect("payload count");
        let tag_count: i64 = row.try_get(1).expect("tag count");
        let attempt_count: i64 = row.try_get(2).expect("attempt count");
        assert_eq!((payload_count, tag_count, attempt_count), (0, 0, 0));

        pool.close().await;
        drop_postgres_test_database(&test_db).await;
    }

    #[tokio::test]
    #[serial]
    async fn libsql_deletes_request_logs_and_usage_events_by_request_ids() {
        let tmp = tempdir().expect("tempdir");
        let db_path = tmp.path().join("gateway.db");
        run_migrations(&db_path).await.expect("migrations");

        let store = LibsqlStore::new_local(db_path.to_str().expect("db path"))
            .await
            .expect("store");
        store
            .seed_from_inputs(&[SeedProvider {
                    provider_key: "openai-prod".to_string(),
                    provider_type: "openai_compat".to_string(),
                    config: json!({}),
                    secrets: None,
                }], &[SeedModel {
                    model_key: "fast".to_string(),
                    alias_target_model_key: None,
                    description: None,
                    tags: Vec::new(),
                    rank: 10,
                    routes: Vec::new(),
                }], &[SeedApiKey {
                    name: "dev".to_string(),
                    public_id: "dev123".to_string(),
                    secret_hash: "$argon2id$v=19$m=19456,t=2,p=1$8WJ6UydAx2RbDXy+zuYbAw$EF+rEtkc71VhwwvS+TS6EiZZvW6rtrjzXX4XvIsDhbU".to_string(),
            service_account_key: "seed-workloads".to_string(),
            service_account_name: "Seed Workloads".to_string(),
            service_account_team_key: "seed-workloads".to_string(),
            service_account_budget: SeedBudget {
                cadence: BudgetCadence::Daily,
                amount_usd: Money4::from_scaled(100_000),
                hard_limit: true,
                timezone: "UTC".to_string(),
            },
                    allowed_models: vec!["fast".to_string()],
                }], &[], &[], &seed_api_key_teams(), &[])
            .await
            .expect("seed");
        let api_key = store
            .get_api_key_by_public_id("dev123")
            .await
            .expect("query key")
            .expect("api key should exist");

        let now = OffsetDateTime::now_utc();
        let seeded_log = RequestLogRecord {
            request_log_id: Uuid::new_v4(),
            request_id: "demo-req-001".to_string(),
            api_key_id: api_key.id,
            user_id: None,
            team_id: api_key.owner_team_id,
            service_account_id: None,
            model_key: "fast".to_string(),
            resolved_model_key: "fast".to_string(),
            provider_key: "openai-prod".to_string(),
            status_code: Some(200),
            latency_ms: Some(42),
            prompt_tokens: Some(1),
            completion_tokens: Some(2),
            total_tokens: Some(3),
            error_code: None,
            has_payload: true,
            request_payload_truncated: false,
            response_payload_truncated: false,
            request_tags: RequestTags {
                service: Some("svc".to_string()),
                component: None,
                env: None,
                bespoke: vec![RequestTag {
                    key: "tenant".to_string(),
                    value: "alpha".to_string(),
                }],
            },
            tool_cardinality: RequestToolCardinality::default(),
            user_agent_raw: None,
            agent_harness_key: "unknown".to_string(),
            agent_harness_label: "Unknown".to_string(),
            metadata: Map::new(),
            occurred_at: now - Duration::hours(2),
        };
        let kept_log = RequestLogRecord {
            request_log_id: Uuid::new_v4(),
            request_id: "req-kept".to_string(),
            has_payload: false,
            request_tags: RequestTags::default(),
            occurred_at: now,
            ..seeded_log.clone()
        };
        let payload = RequestLogPayloadRecord {
            request_log_id: seeded_log.request_log_id,
            request_json: json!({"prompt": "seeded"}),
            response_json: json!({"ok": true}),
        };
        let attempt = RequestAttemptRecord {
            request_attempt_id: Uuid::new_v4(),
            request_log_id: seeded_log.request_log_id,
            request_id: seeded_log.request_id.clone(),
            attempt_number: 1,
            route_id: Uuid::new_v4(),
            provider_key: "openai-prod".to_string(),
            upstream_model: "gpt-4o-mini".to_string(),
            status: RequestAttemptStatus::Success,
            status_code: Some(200),
            error_code: None,
            error_detail: None,
            error_detail_truncated: false,
            retryable: false,
            terminal: true,
            produced_final_response: true,
            stream: false,
            started_at: seeded_log.occurred_at,
            completed_at: Some(seeded_log.occurred_at + Duration::milliseconds(42)),
            latency_ms: Some(42),
            metadata: Map::new(),
        };

        store
            .insert_request_log_with_attempts(&seeded_log, Some(&payload), &[attempt])
            .await
            .expect("insert seeded request log");
        store
            .insert_request_log(&kept_log, None)
            .await
            .expect("insert kept request log");

        let seeded_event = build_usage_ledger_record(
            "demo-req-001",
            "user:scope-demo".to_string(),
            api_key.id,
            None,
            None,
            None,
            None,
            "gpt-4o-mini",
            UsagePricingStatus::Priced,
            1_000,
            now - Duration::hours(2),
        );
        let kept_event = build_usage_ledger_record(
            "req-kept",
            "user:scope-kept".to_string(),
            api_key.id,
            None,
            None,
            None,
            None,
            "gpt-4o-mini",
            UsagePricingStatus::Priced,
            2_000,
            now,
        );
        assert!(
            store
                .insert_usage_ledger_if_absent(&seeded_event)
                .await
                .expect("insert seeded usage event")
        );
        assert!(
            store
                .insert_usage_ledger_if_absent(&kept_event)
                .await
                .expect("insert kept usage event")
        );

        let request_ids = vec!["demo-req-001".to_string()];
        assert_eq!(
            store
                .delete_request_logs_by_request_ids(&request_ids)
                .await
                .expect("delete seeded request logs"),
            1
        );
        assert_eq!(
            store
                .delete_usage_ledger_events_by_request_ids(&request_ids)
                .await
                .expect("delete seeded usage events"),
            1
        );

        assert!(
            store
                .get_request_log_detail(seeded_log.request_log_id)
                .await
                .is_err()
        );
        assert!(
            store
                .get_request_log_detail(kept_log.request_log_id)
                .await
                .is_ok()
        );
        assert!(
            store
                .get_usage_ledger_by_request_and_scope("demo-req-001", "user:scope-demo")
                .await
                .expect("seeded usage lookup")
                .is_none()
        );
        assert!(
            store
                .get_usage_ledger_by_request_and_scope("req-kept", "user:scope-kept")
                .await
                .expect("kept usage lookup")
                .is_some()
        );

        // Repeat deletes are no-ops, which is what makes reseeding idempotent.
        assert_eq!(
            store
                .delete_request_logs_by_request_ids(&request_ids)
                .await
                .expect("repeat request log delete"),
            0
        );
        assert_eq!(
            store
                .delete_usage_ledger_events_by_request_ids(&request_ids)
                .await
                .expect("repeat usage event delete"),
            0
        );
        assert_eq!(
            store
                .delete_request_logs_by_request_ids(&[])
                .await
                .expect("empty request log delete"),
            0
        );

        let db = libsql::Builder::new_local(db_path)
            .build()
            .await
            .expect("libsql db");
        let connection = db.connect().expect("libsql connection");
        let mut rows = connection
            .query(
                r#"
                SELECT
                    (SELECT COUNT(*) FROM request_log_payloads),
                    (SELECT COUNT(*) FROM request_log_tags),
                    (SELECT COUNT(*) FROM request_log_attempts)
                "#,
                (),
            )
            .await
            .expect("child counts");
        let row = rows
            .next()
            .await
            .expect("child count row")
            .expect("child count row exists");
        let payload_count: i64 = row.get(0).expect("payload count");
        let tag_count: i64 = row.get(1).expect("tag count");
        let attempt_count: i64 = row.get(2).expect("attempt count");
        assert_eq!((payload_count, tag_count, attempt_count), (0, 0, 0));
    }

    #[tokio::test]
    #[serial]
    async fn postgres_deletes_request_logs_and_usage_events_by_request_ids() {
        let Some(test_db) = create_postgres_test_database().await else {
            eprintln!(
                "skipping postgres request log delete test because TEST_POSTGRES_URL is not set"
            );
            return;
        };

        let options = StoreConnectionOptions::Postgres {
            url: test_db.database_url.clone(),
            max_connections: 4,
        };
        run_migrations_with_options(&options)
            .await
            .expect("postgres migrations");

        let store = PostgresStore::connect(&test_db.database_url, 4)
            .await
            .expect("postgres store");
        store
            .seed_from_inputs(&[SeedProvider {
                    provider_key: "openai-prod".to_string(),
                    provider_type: "openai_compat".to_string(),
                    config: json!({}),
                    secrets: None,
                }], &[SeedModel {
                    model_key: "fast".to_string(),
                    alias_target_model_key: None,
                    description: None,
                    tags: Vec::new(),
                    rank: 10,
                    routes: Vec::new(),
                }], &[SeedApiKey {
                    name: "dev".to_string(),
                    public_id: "dev123".to_string(),
                    secret_hash: "$argon2id$v=19$m=19456,t=2,p=1$8WJ6UydAx2RbDXy+zuYbAw$EF+rEtkc71VhwwvS+TS6EiZZvW6rtrjzXX4XvIsDhbU".to_string(),
            service_account_key: "seed-workloads".to_string(),
            service_account_name: "Seed Workloads".to_string(),
            service_account_team_key: "seed-workloads".to_string(),
            service_account_budget: SeedBudget {
                cadence: BudgetCadence::Daily,
                amount_usd: Money4::from_scaled(100_000),
                hard_limit: true,
                timezone: "UTC".to_string(),
            },
                    allowed_models: vec!["fast".to_string()],
                }], &[], &[], &seed_api_key_teams(), &[])
            .await
            .expect("seed");
        let api_key = store
            .get_api_key_by_public_id("dev123")
            .await
            .expect("query key")
            .expect("api key should exist");

        let now = OffsetDateTime::now_utc();
        let seeded_log = RequestLogRecord {
            request_log_id: Uuid::new_v4(),
            request_id: "demo-req-pg-001".to_string(),
            api_key_id: api_key.id,
            user_id: None,
            team_id: api_key.owner_team_id,
            service_account_id: None,
            model_key: "fast".to_string(),
            resolved_model_key: "fast".to_string(),
            provider_key: "openai-prod".to_string(),
            status_code: Some(200),
            latency_ms: Some(42),
            prompt_tokens: Some(1),
            completion_tokens: Some(2),
            total_tokens: Some(3),
            error_code: None,
            has_payload: true,
            request_payload_truncated: false,
            response_payload_truncated: false,
            request_tags: RequestTags {
                service: Some("svc".to_string()),
                component: None,
                env: None,
                bespoke: vec![RequestTag {
                    key: "tenant".to_string(),
                    value: "alpha".to_string(),
                }],
            },
            tool_cardinality: RequestToolCardinality::default(),
            user_agent_raw: None,
            agent_harness_key: "unknown".to_string(),
            agent_harness_label: "Unknown".to_string(),
            metadata: Map::new(),
            occurred_at: now - Duration::hours(2),
        };
        let kept_log = RequestLogRecord {
            request_log_id: Uuid::new_v4(),
            request_id: "req-kept-postgres".to_string(),
            has_payload: false,
            request_tags: RequestTags::default(),
            occurred_at: now,
            ..seeded_log.clone()
        };
        let payload = RequestLogPayloadRecord {
            request_log_id: seeded_log.request_log_id,
            request_json: json!({"prompt": "seeded"}),
            response_json: json!({"ok": true}),
        };
        let attempt = RequestAttemptRecord {
            request_attempt_id: Uuid::new_v4(),
            request_log_id: seeded_log.request_log_id,
            request_id: seeded_log.request_id.clone(),
            attempt_number: 1,
            route_id: Uuid::new_v4(),
            provider_key: "openai-prod".to_string(),
            upstream_model: "gpt-4o-mini".to_string(),
            status: RequestAttemptStatus::Success,
            status_code: Some(200),
            error_code: None,
            error_detail: None,
            error_detail_truncated: false,
            retryable: false,
            terminal: true,
            produced_final_response: true,
            stream: false,
            started_at: seeded_log.occurred_at,
            completed_at: Some(seeded_log.occurred_at + Duration::milliseconds(42)),
            latency_ms: Some(42),
            metadata: Map::new(),
        };

        store
            .insert_request_log_with_attempts(&seeded_log, Some(&payload), &[attempt])
            .await
            .expect("insert seeded request log");
        store
            .insert_request_log(&kept_log, None)
            .await
            .expect("insert kept request log");

        let seeded_event = build_usage_ledger_record(
            "demo-req-pg-001",
            "user:scope-demo".to_string(),
            api_key.id,
            None,
            None,
            None,
            None,
            "gpt-4o-mini",
            UsagePricingStatus::Priced,
            1_000,
            now - Duration::hours(2),
        );
        let kept_event = build_usage_ledger_record(
            "req-kept-postgres",
            "user:scope-kept".to_string(),
            api_key.id,
            None,
            None,
            None,
            None,
            "gpt-4o-mini",
            UsagePricingStatus::Priced,
            2_000,
            now,
        );
        assert!(
            store
                .insert_usage_ledger_if_absent(&seeded_event)
                .await
                .expect("insert seeded usage event")
        );
        assert!(
            store
                .insert_usage_ledger_if_absent(&kept_event)
                .await
                .expect("insert kept usage event")
        );

        let request_ids = vec!["demo-req-pg-001".to_string()];
        assert_eq!(
            store
                .delete_request_logs_by_request_ids(&request_ids)
                .await
                .expect("delete seeded request logs"),
            1
        );
        assert_eq!(
            store
                .delete_usage_ledger_events_by_request_ids(&request_ids)
                .await
                .expect("delete seeded usage events"),
            1
        );

        assert!(
            store
                .get_request_log_detail(seeded_log.request_log_id)
                .await
                .is_err()
        );
        assert!(
            store
                .get_request_log_detail(kept_log.request_log_id)
                .await
                .is_ok()
        );
        assert!(
            store
                .get_usage_ledger_by_request_and_scope("demo-req-pg-001", "user:scope-demo")
                .await
                .expect("seeded usage lookup")
                .is_none()
        );
        assert!(
            store
                .get_usage_ledger_by_request_and_scope("req-kept-postgres", "user:scope-kept")
                .await
                .expect("kept usage lookup")
                .is_some()
        );

        // Repeat deletes are no-ops, which is what makes reseeding idempotent.
        assert_eq!(
            store
                .delete_request_logs_by_request_ids(&request_ids)
                .await
                .expect("repeat request log delete"),
            0
        );
        assert_eq!(
            store
                .delete_usage_ledger_events_by_request_ids(&request_ids)
                .await
                .expect("repeat usage event delete"),
            0
        );

        let pool = sqlx::PgPool::connect(&test_db.database_url)
            .await
            .expect("postgres pool");
        let row = sqlx::query(
            r#"
            SELECT
                (SELECT COUNT(*) FROM request_log_payloads),
                (SELECT COUNT(*) FROM request_log_tags),
                (SELECT COUNT(*) FROM request_log_attempts)
            "#,
        )
        .fetch_one(&pool)
        .await
        .expect("child counts");
        let payload_count: i64 = row.try_get(0).expect("payload count");
        let tag_count: i64 = row.try_get(1).expect("tag count");
        let attempt_count: i64 = row.try_get(2).expect("attempt count");
        assert_eq!((payload_count, tag_count, attempt_count), (0, 0, 0));

        pool.close().await;
        drop_postgres_test_database(&test_db).await;
    }

    #[tokio::test]
    #[serial]
    async fn libsql_alias_backed_models_round_trip_through_store() {
        let tmp = tempdir().expect("tempdir");
        let db_path = tmp.path().join("gateway.db");
        run_migrations(&db_path).await.expect("migrations");

        let store = LibsqlStore::new_local(db_path.to_str().expect("db path"))
            .await
            .expect("store");

        let providers = vec![SeedProvider {
            provider_key: "openai-prod".to_string(),
            provider_type: "openai_compat".to_string(),
            config: json!({
                "base_url": "https://api.openai.com/v1",
                "timeout_ms": 120_000
            }),
            secrets: Some(json!({"token": "env.OPENAI_API_KEY"})),
        }];

        let models = vec![
            SeedModel {
                model_key: "fast".to_string(),
                alias_target_model_key: Some("fast-v2".to_string()),
                description: Some("alias".to_string()),
                tags: vec!["fast".to_string()],
                rank: 10,
                routes: Vec::new(),
            },
            SeedModel {
                model_key: "fast-v2".to_string(),
                alias_target_model_key: None,
                description: Some("replacement".to_string()),
                tags: vec!["fast".to_string()],
                rank: 5,
                routes: vec![SeedModelRoute {
                    provider_key: "openai-prod".to_string(),
                    upstream_model: "gpt-5".to_string(),
                    priority: 10,
                    weight: 1.0,
                    enabled: true,
                    extra_headers: Map::new(),
                    extra_body: Map::new(),
                    capabilities: ProviderCapabilities::all_enabled(),
                    compatibility: Default::default(),
                }],
            },
        ];

        let api_keys = vec![SeedApiKey {
            name: "dev".to_string(),
            public_id: "dev123".to_string(),
            secret_hash: "$argon2id$v=19$m=19456,t=2,p=1$8WJ6UydAx2RbDXy+zuYbAw$EF+rEtkc71VhwwvS+TS6EiZZvW6rtrjzXX4XvIsDhbU".to_string(),
            service_account_key: "seed-workloads".to_string(),
            service_account_name: "Seed Workloads".to_string(),
            service_account_team_key: "seed-workloads".to_string(),
            service_account_budget: SeedBudget {
                cadence: BudgetCadence::Daily,
                amount_usd: Money4::from_scaled(100_000),
                hard_limit: true,
                timezone: "UTC".to_string(),
            },
            allowed_models: vec!["fast".to_string()],
        }];

        store
            .seed_from_inputs(
                &providers,
                &models,
                &api_keys,
                &[],
                &[],
                &seed_api_key_teams(),
                &[],
            )
            .await
            .expect("seed");

        let alias_model = store
            .get_model_by_key("fast")
            .await
            .expect("query alias")
            .expect("alias model exists");
        assert_eq!(
            alias_model.alias_target_model_key.as_deref(),
            Some("fast-v2")
        );

        let api_key = store
            .get_api_key_by_public_id("dev123")
            .await
            .expect("query key")
            .expect("api key exists");
        let accessible_models = store
            .list_models_for_api_key(api_key.id)
            .await
            .expect("models by key");
        assert_eq!(accessible_models.len(), 1);
        assert_eq!(accessible_models[0].model_key, "fast");
        assert_eq!(
            accessible_models[0].alias_target_model_key.as_deref(),
            Some("fast-v2")
        );

        let target_model = store
            .get_model_by_key("fast-v2")
            .await
            .expect("query target")
            .expect("target model exists");
        let routes = store
            .list_routes_for_model(target_model.id)
            .await
            .expect("target routes");
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].upstream_model, "gpt-5");
    }

    #[tokio::test]
    #[serial]
    async fn libsql_request_log_detail_missing_returns_not_found() {
        let tmp = tempdir().expect("tempdir");
        let db_path = tmp.path().join("gateway.db");
        run_migrations(&db_path).await.expect("migrations");

        let store = LibsqlStore::new_local(db_path.to_str().expect("db path"))
            .await
            .expect("store");

        let error = store
            .get_request_log_detail(Uuid::new_v4())
            .await
            .expect_err("missing request log should fail");
        assert!(matches!(error, StoreError::NotFound(_)));
    }

    #[tokio::test]
    #[serial]
    async fn users_email_normalized_is_case_insensitive_unique() {
        let tmp = tempdir().expect("tempdir");
        let db_path = tmp.path().join("gateway.db");
        run_migrations(&db_path).await.expect("migrations");
        let db = libsql::Builder::new_local(db_path.to_str().expect("db path"))
            .build()
            .await
            .expect("db");
        let conn = db.connect().expect("connection");
        let now = time::OffsetDateTime::now_utc().unix_timestamp();

        conn.execute(
            r#"
            INSERT INTO users (
              user_id, name, email, email_normalized, global_role, auth_mode, status,
              request_logging_enabled, model_access_mode, created_at, updated_at
            ) VALUES (?1, 'User One', 'USER@EXAMPLE.COM', 'user@example.com', 'user', 'password', 'active', 1, 'all', ?2, ?2)
            "#,
            libsql::params![Uuid::new_v4().to_string(), now],
        )
        .await
        .expect("first user");

        let duplicate_result = conn
            .execute(
                r#"
                INSERT INTO users (
                  user_id, name, email, email_normalized, global_role, auth_mode, status,
                  request_logging_enabled, model_access_mode, created_at, updated_at
                ) VALUES (?1, 'User Two', 'user@example.com', 'user@example.com', 'user', 'password', 'active', 1, 'all', ?2, ?2)
                "#,
                libsql::params![Uuid::new_v4().to_string(), now],
            )
            .await;

        assert!(duplicate_result.is_err());
    }

    #[tokio::test]
    #[serial]
    async fn create_team_round_trips_and_accepts_zero_admins() {
        let tmp = tempdir().expect("tempdir");
        let db_path = tmp.path().join("gateway.db");
        run_migrations(&db_path).await.expect("migrations");

        let store = LibsqlStore::new_local(db_path.to_str().expect("db path"))
            .await
            .expect("store");

        let team = store
            .create_team("platform-ops", "Platform Ops")
            .await
            .expect("create team");

        assert_eq!(team.team_key, "platform-ops");
        assert_eq!(team.team_name, "Platform Ops");
        assert_eq!(team.status, "active");
        assert!(
            store
                .list_team_memberships(team.team_id)
                .await
                .expect("memberships")
                .is_empty()
        );
    }

    #[tokio::test]
    #[serial]
    async fn update_team_membership_role_promotes_and_demotes_admins() {
        let tmp = tempdir().expect("tempdir");
        let db_path = tmp.path().join("gateway.db");
        run_migrations(&db_path).await.expect("migrations");

        let store = LibsqlStore::new_local(db_path.to_str().expect("db path"))
            .await
            .expect("store");
        let team = store
            .create_team("core-platform", "Core Platform")
            .await
            .expect("team");

        let conn = store.connection();
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let user_id = Uuid::new_v4();
        conn.execute(
            r#"
            INSERT INTO users (
              user_id, name, email, email_normalized, global_role, auth_mode, status,
              request_logging_enabled, model_access_mode, created_at, updated_at
            ) VALUES (?1, 'Jane Admin', 'jane@example.com', 'jane@example.com', ?2, ?3, 'active', 1, 'all', ?4, ?4)
            "#,
            libsql::params![
                user_id.to_string(),
                GlobalRole::User.as_str(),
                AuthMode::Password.as_str(),
                now
            ],
        )
        .await
        .expect("user");

        store
            .assign_team_membership(user_id, team.team_id, MembershipRole::Member)
            .await
            .expect("member");
        store
            .update_team_membership_role(
                team.team_id,
                user_id,
                MembershipRole::Admin,
                time::OffsetDateTime::now_utc(),
            )
            .await
            .expect("promote");

        let memberships = store
            .list_team_memberships(team.team_id)
            .await
            .expect("memberships");
        assert_eq!(memberships.len(), 1);
        assert_eq!(memberships[0].role, MembershipRole::Admin);

        store
            .update_team_membership_role(
                team.team_id,
                user_id,
                MembershipRole::Member,
                time::OffsetDateTime::now_utc(),
            )
            .await
            .expect("demote");

        let memberships = store
            .list_team_memberships(team.team_id)
            .await
            .expect("memberships");
        assert_eq!(memberships[0].role, MembershipRole::Member);
    }

    #[tokio::test]
    #[serial]
    async fn team_membership_enforces_single_team_per_user() {
        let tmp = tempdir().expect("tempdir");
        let db_path = tmp.path().join("gateway.db");
        run_migrations(&db_path).await.expect("migrations");

        let store = LibsqlStore::new_local(db_path.to_str().expect("db path"))
            .await
            .expect("store");
        let first_team = store
            .create_team("alpha", "Alpha")
            .await
            .expect("first team");
        let second_team = store
            .create_team("beta", "Beta")
            .await
            .expect("second team");

        let conn = store.connection();
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let user_id = Uuid::new_v4();
        conn.execute(
            r#"
            INSERT INTO users (
              user_id, name, email, email_normalized, global_role, auth_mode, status,
              request_logging_enabled, model_access_mode, created_at, updated_at
            ) VALUES (?1, 'Cross Team', 'cross@example.com', 'cross@example.com', ?2, ?3, 'active', 1, 'all', ?4, ?4)
            "#,
            libsql::params![
                user_id.to_string(),
                GlobalRole::User.as_str(),
                AuthMode::Password.as_str(),
                now
            ],
        )
        .await
        .expect("user");

        store
            .assign_team_membership(user_id, first_team.team_id, MembershipRole::Member)
            .await
            .expect("first membership");

        let conflict = store
            .assign_team_membership(user_id, second_team.team_id, MembershipRole::Member)
            .await;

        assert!(conflict.is_err());
    }

    async fn assert_identity_mutation_store_helpers<S: GatewayStore>(store: &S) {
        let source_team = store
            .create_team("source", "Source")
            .await
            .expect("source team");
        let destination_team = store
            .create_team("destination", "Destination")
            .await
            .expect("destination team");
        let member = store
            .create_identity_user(
                "Member",
                "member@example.com",
                "member@example.com",
                GlobalRole::User,
                AuthMode::Password,
                UserStatus::Active,
            )
            .await
            .expect("member");
        let owner = store
            .create_identity_user(
                "Owner",
                "owner@example.com",
                "owner@example.com",
                GlobalRole::User,
                AuthMode::Password,
                UserStatus::Active,
            )
            .await
            .expect("owner");
        let admin = store
            .create_identity_user(
                "Admin",
                "admin@example.com",
                "admin@example.com",
                GlobalRole::PlatformAdmin,
                AuthMode::Password,
                UserStatus::Active,
            )
            .await
            .expect("platform admin");
        let now = OffsetDateTime::now_utc();

        assert!(
            !store
                .remove_team_membership(source_team.team_id, member.user_id)
                .await
                .expect("remove non-member")
        );

        store
            .assign_team_membership(member.user_id, source_team.team_id, MembershipRole::Admin)
            .await
            .expect("assign member");
        store
            .assign_team_membership(owner.user_id, source_team.team_id, MembershipRole::Owner)
            .await
            .expect("assign owner");

        store
            .transfer_team_membership(
                member.user_id,
                source_team.team_id,
                destination_team.team_id,
                MembershipRole::Member,
                now,
            )
            .await
            .expect("transfer member");
        let transferred_membership = store
            .get_team_membership_for_user(member.user_id)
            .await
            .expect("lookup membership")
            .expect("membership exists");
        assert_eq!(transferred_membership.team_id, destination_team.team_id);
        assert_eq!(transferred_membership.role, MembershipRole::Member);

        assert!(
            store
                .remove_team_membership(destination_team.team_id, member.user_id)
                .await
                .expect("remove transferred member")
        );
        assert!(
            store
                .get_team_membership_for_user(member.user_id)
                .await
                .expect("load membership")
                .is_none()
        );
        assert!(matches!(
            store
                .remove_team_membership(source_team.team_id, owner.user_id)
                .await,
            Err(StoreError::Conflict(_))
        ));
        assert!(matches!(
            store
                .transfer_team_membership(
                    owner.user_id,
                    source_team.team_id,
                    destination_team.team_id,
                    MembershipRole::Member,
                    now,
                )
                .await,
            Err(StoreError::Conflict(_))
        ));

        store
            .store_user_password(member.user_id, "hash", now)
            .await
            .expect("store password");
        assert!(
            store
                .get_user_password_auth(member.user_id)
                .await
                .expect("password auth")
                .is_some()
        );
        store
            .delete_user_password_auth(member.user_id)
            .await
            .expect("delete password auth");
        assert!(
            store
                .get_user_password_auth(member.user_id)
                .await
                .expect("password auth after delete")
                .is_none()
        );

        let session = store
            .create_user_session(
                Uuid::new_v4(),
                member.user_id,
                "token-hash",
                now + Duration::days(1),
                now,
            )
            .await
            .expect("create session");
        let other_session = store
            .create_user_session(
                Uuid::new_v4(),
                member.user_id,
                "other-token-hash",
                now + Duration::days(1),
                now,
            )
            .await
            .expect("create second session");
        store
            .revoke_user_session(session.session_id, now)
            .await
            .expect("revoke one session");
        assert!(
            store
                .get_user_session(session.session_id)
                .await
                .expect("load revoked session")
                .expect("session exists")
                .revoked_at
                .is_some()
        );
        assert!(
            store
                .get_user_session(other_session.session_id)
                .await
                .expect("load other session")
                .expect("other session exists")
                .revoked_at
                .is_none()
        );
        store
            .revoke_user_sessions(member.user_id, now)
            .await
            .expect("revoke sessions");
        assert!(
            store
                .get_user_session(other_session.session_id)
                .await
                .expect("load session")
                .expect("session exists")
                .revoked_at
                .is_some()
        );

        let loaded_member = store
            .get_identity_user(member.user_id)
            .await
            .expect("load identity user")
            .expect("identity user exists");
        assert_eq!(loaded_member.user.user_id, member.user_id);

        let last_admin_disable = store.deactivate_identity_user(admin.user_id, now).await;
        assert!(matches!(last_admin_disable, Err(StoreError::Conflict(_))));

        let second_admin = store
            .create_identity_user(
                "Second Admin",
                "second-admin@example.com",
                "second-admin@example.com",
                GlobalRole::PlatformAdmin,
                AuthMode::Password,
                UserStatus::Active,
            )
            .await
            .expect("second admin");
        store
            .deactivate_identity_user(admin.user_id, now)
            .await
            .expect("disable admin with backup");
        let disabled_admin = store
            .get_user_by_id(admin.user_id)
            .await
            .expect("load disabled admin")
            .expect("disabled admin exists");
        assert_eq!(disabled_admin.status, UserStatus::Disabled);

        let last_admin_demote = store
            .update_identity_user(
                second_admin.user_id,
                GlobalRole::User,
                AuthMode::Password,
                now,
            )
            .await;
        assert!(matches!(last_admin_demote, Err(StoreError::Conflict(_))));
    }

    #[tokio::test]
    #[serial]
    async fn libsql_identity_mutation_store_helpers_cover_transfer_removal_and_revocation() {
        let tmp = tempdir().expect("tempdir");
        let db_path = tmp.path().join("gateway.db");
        run_migrations(&db_path).await.expect("migrations");

        let store = LibsqlStore::new_local(db_path.to_str().expect("db path"))
            .await
            .expect("store");
        assert_identity_mutation_store_helpers(&store).await;
    }

    #[tokio::test]
    #[serial]
    async fn pricing_catalog_cache_round_trips_and_touch_updates_fetched_at() {
        let tmp = tempdir().expect("tempdir");
        let db_path = tmp.path().join("gateway.db");
        run_migrations(&db_path).await.expect("migrations");

        let store = LibsqlStore::new_local(db_path.to_str().expect("db path"))
            .await
            .expect("store");
        let fetched_at =
            time::OffsetDateTime::from_unix_timestamp(1_700_000_000).expect("timestamp");

        store
            .upsert_pricing_catalog_cache(&PricingCatalogCacheRecord {
                catalog_key: "models_dev_supported_v1".to_string(),
                source: "models_dev_api".to_string(),
                etag: Some("\"etag-1\"".to_string()),
                fetched_at,
                snapshot_json: "{\"providers\":{}}".to_string(),
            })
            .await
            .expect("insert pricing cache");

        let inserted = store
            .get_pricing_catalog_cache("models_dev_supported_v1")
            .await
            .expect("load pricing cache")
            .expect("pricing cache should exist");
        assert_eq!(inserted.source, "models_dev_api");
        assert_eq!(inserted.etag.as_deref(), Some("\"etag-1\""));
        assert_eq!(inserted.fetched_at, fetched_at);

        let touched_at = fetched_at + time::Duration::minutes(5);
        store
            .touch_pricing_catalog_cache_fetched_at("models_dev_supported_v1", touched_at)
            .await
            .expect("touch pricing cache");

        let touched = store
            .get_pricing_catalog_cache("models_dev_supported_v1")
            .await
            .expect("reload pricing cache")
            .expect("pricing cache should exist");
        assert_eq!(touched.snapshot_json, inserted.snapshot_json);
        assert_eq!(touched.fetched_at, touched_at);
    }

    #[tokio::test]
    #[serial]
    async fn api_key_owner_constraint_rejects_invalid_combinations() {
        let tmp = tempdir().expect("tempdir");
        let db_path = tmp.path().join("gateway.db");
        run_migrations(&db_path).await.expect("migrations");
        let db = libsql::Builder::new_local(db_path.to_str().expect("db path"))
            .build()
            .await
            .expect("db");
        let conn = db.connect().expect("connection");
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        conn.execute("PRAGMA foreign_keys = ON", ())
            .await
            .expect("enable foreign keys");

        let invalid_result = conn
            .execute(
                r#"
                INSERT INTO api_keys (
                  id, public_id, secret_hash, name, status, owner_kind, owner_user_id, owner_team_id, created_at
                ) VALUES (?1, 'invalid_owner', 'hash', 'invalid', 'active', 'user', NULL, NULL, ?2)
                "#,
                libsql::params![Uuid::new_v4().to_string(), now],
            )
            .await;

        assert!(invalid_result.is_err());

        let service_team_id = Uuid::new_v4();
        let other_team_id = Uuid::new_v4();
        let service_account_id = Uuid::new_v4();
        conn.execute(
            r#"
            INSERT INTO teams (
              team_id, team_key, team_name, status, model_access_mode, created_at, updated_at
            ) VALUES
              (?1, 'service-team', 'Service Team', 'active', 'all', ?3, ?3),
              (?2, 'other-team', 'Other Team', 'active', 'all', ?3, ?3)
            "#,
            libsql::params![service_team_id.to_string(), other_team_id.to_string(), now],
        )
        .await
        .expect("teams");
        conn.execute(
            r#"
            INSERT INTO service_accounts (
              service_account_id, team_id, service_account_key, service_account_name,
              status, model_access_mode, metadata_json, created_at, updated_at
            ) VALUES (?1, ?2, 'svc', 'Service', 'active', 'all', '{}', ?3, ?3)
            "#,
            libsql::params![
                service_account_id.to_string(),
                service_team_id.to_string(),
                now
            ],
        )
        .await
        .expect("service account");

        let mismatched_team_result = conn
            .execute(
                r#"
                INSERT INTO api_keys (
                  id, public_id, secret_hash, name, status, owner_kind,
                  owner_user_id, owner_team_id, owner_service_account_id, created_at
                ) VALUES (?1, 'invalid_service_team', 'hash', 'invalid', 'active',
                  'service_account', NULL, ?2, ?3, ?4)
                "#,
                libsql::params![
                    Uuid::new_v4().to_string(),
                    other_team_id.to_string(),
                    service_account_id.to_string(),
                    now
                ],
            )
            .await;
        assert!(mismatched_team_result.is_err());
    }

    #[tokio::test]
    #[serial]
    async fn libsql_service_account_migration_preserves_user_key_history() {
        let tmp = tempdir().expect("tempdir");
        let db_path = tmp.path().join("gateway.db");
        let db = libsql::Builder::new_local(db_path.to_str().expect("db path"))
            .build()
            .await
            .expect("db");
        let conn = db.connect().expect("connection");

        for migration in MIGRATION_REGISTRY
            .iter()
            .filter(|migration| migration.version < 21)
        {
            conn.execute_batch(migration.sql_for(MigrationBackend::Libsql))
                .await
                .unwrap_or_else(|error| panic!("apply migration {}: {error}", migration.version));
        }

        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let team_id = Uuid::new_v4();
        let user_id = Uuid::new_v4();
        let model_id = Uuid::new_v4();
        let api_key_id = Uuid::new_v4();
        let request_log_id = Uuid::new_v4();
        let usage_event_id = Uuid::new_v4();
        let request_attempt_id = Uuid::new_v4();

        conn.execute(
            r#"
            INSERT INTO providers (provider_key, provider_type, config_json, created_at, updated_at)
            VALUES ('openai-prod', 'openai_compat', '{}', ?1, ?1)
            "#,
            libsql::params![now],
        )
        .await
        .expect("provider");
        conn.execute(
            r#"
            INSERT INTO gateway_models (
              id, model_key, description, tags_json, rank, created_at, updated_at
            ) VALUES (?1, 'gpt-test', 'test', '[]', 1, ?2, ?2)
            "#,
            libsql::params![model_id.to_string(), now],
        )
        .await
        .expect("model");
        conn.execute(
            r#"
            INSERT INTO teams (
              team_id, team_key, team_name, status, model_access_mode, created_at, updated_at
            ) VALUES (?1, 'team', 'Team', 'active', 'all', ?2, ?2)
            "#,
            libsql::params![team_id.to_string(), now],
        )
        .await
        .expect("team");
        conn.execute(
            r#"
            INSERT INTO users (
              user_id, name, email, email_normalized, global_role, auth_mode, status,
              request_logging_enabled, model_access_mode, created_at, updated_at
            ) VALUES (?1, 'User', 'user@example.com', 'user@example.com', 'user', 'password', 'active', 1, 'all', ?2, ?2)
            "#,
            libsql::params![user_id.to_string(), now],
        )
        .await
        .expect("user");
        conn.execute(
            r#"
            INSERT INTO api_keys (
              id, public_id, secret_hash, name, status, owner_kind, owner_user_id, owner_team_id, created_at
            ) VALUES (?1, 'pub_user', 'hash', 'User key', 'active', 'user', ?2, NULL, ?3)
            "#,
            libsql::params![api_key_id.to_string(), user_id.to_string(), now],
        )
        .await
        .expect("api key");
        conn.execute(
            "INSERT INTO api_key_model_grants (api_key_id, model_id) VALUES (?1, ?2)",
            libsql::params![api_key_id.to_string(), model_id.to_string()],
        )
        .await
        .expect("grant");
        conn.execute(
            r#"
            INSERT INTO audit_logs (ts, actor_api_key_id, action, object_type, details_json)
            VALUES (?1, ?2, 'test', 'api_key', '{}')
            "#,
            libsql::params![now, api_key_id.to_string()],
        )
        .await
        .expect("audit log");
        conn.execute(
            r#"
            INSERT INTO request_logs (
              request_log_id, request_id, api_key_id, user_id, team_id, model_key, provider_key,
              status_code, latency_ms, prompt_tokens, completion_tokens, total_tokens, metadata_json,
              occurred_at, resolved_model_key, has_payload, caller_service
            ) VALUES (?1, 'req-1', ?2, ?3, ?4, 'gpt-test', 'openai-prod',
              200, 10, 1, 2, 3, '{}', ?5, 'gpt-test', 1, 'tests')
            "#,
            libsql::params![
                request_log_id.to_string(),
                api_key_id.to_string(),
                user_id.to_string(),
                team_id.to_string(),
                now
            ],
        )
        .await
        .expect("request log");
        conn.execute(
            "INSERT INTO request_log_payloads (request_log_id, request_json, response_json) VALUES (?1, '{}', '{}')",
            libsql::params![request_log_id.to_string()],
        )
        .await
        .expect("payload");
        conn.execute(
            "INSERT INTO request_log_tags (request_log_id, tag_key, tag_value) VALUES (?1, 'env', 'test')",
            libsql::params![request_log_id.to_string()],
        )
        .await
        .expect("tag");
        conn.execute(
            r#"
            INSERT INTO request_log_attempts (
              request_attempt_id, request_log_id, request_id, attempt_number, route_id,
              provider_key, upstream_model, status, retryable, terminal, produced_final_response,
              stream, started_at, completed_at, metadata_json
            ) VALUES (?1, ?2, 'req-1', 1, 'route-1', 'openai-prod', 'gpt-test',
              'success', 0, 1, 1, 0, ?3, ?3, '{}')
            "#,
            libsql::params![
                request_attempt_id.to_string(),
                request_log_id.to_string(),
                now
            ],
        )
        .await
        .expect("attempt");
        conn.execute(
            r#"
            INSERT INTO usage_cost_events (
              usage_event_id, request_id, ownership_scope_key, api_key_id, user_id, team_id,
              model_id, provider_key, upstream_model, provider_usage_json, pricing_status,
              computed_cost_10000, occurred_at
            ) VALUES (?1, 'req-1', ?2, ?3, ?4, ?5, ?6, 'openai-prod', 'gpt-test',
              '{}', 'priced', 123, ?7)
            "#,
            libsql::params![
                usage_event_id.to_string(),
                format!("user:{}", user_id),
                api_key_id.to_string(),
                user_id.to_string(),
                team_id.to_string(),
                model_id.to_string(),
                now
            ],
        )
        .await
        .expect("usage event");

        let v21 = MIGRATION_REGISTRY
            .iter()
            .find(|migration| migration.version == 21)
            .expect("v21 migration");
        conn.execute_batch(v21.sql_for(MigrationBackend::Libsql))
            .await
            .expect("apply v21");

        for table in [
            "api_key_model_grants",
            "request_logs",
            "request_log_payloads",
            "request_log_tags",
            "request_log_attempts",
            "usage_cost_events",
        ] {
            let sql = format!("SELECT COUNT(*) FROM {table}");
            let count: i64 = conn
                .query(&sql, ())
                .await
                .expect("count query")
                .next()
                .await
                .expect("count row")
                .expect("count row")
                .get(0)
                .expect("count value");
            assert_eq!(count, 1, "{table} rows should survive api_keys rebuild");
        }

        let actor_api_key_id: Option<String> = conn
            .query("SELECT actor_api_key_id FROM audit_logs", ())
            .await
            .expect("audit query")
            .next()
            .await
            .expect("audit row")
            .expect("audit row")
            .get(0)
            .expect("actor api key");
        assert_eq!(
            actor_api_key_id.as_deref(),
            Some(api_key_id.to_string().as_str())
        );
    }

    #[tokio::test]
    #[serial]
    async fn budget_scope_enforces_single_active_record_per_scope_key() {
        let tmp = tempdir().expect("tempdir");
        let db_path = tmp.path().join("gateway.db");
        run_migrations(&db_path).await.expect("migrations");
        let db = libsql::Builder::new_local(db_path.to_str().expect("db path"))
            .build()
            .await
            .expect("db");
        let conn = db.connect().expect("connection");
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let user_id = Uuid::new_v4();

        conn.execute(
            r#"
            INSERT INTO users (
              user_id, name, email, email_normalized, global_role, auth_mode, status,
              request_logging_enabled, model_access_mode, created_at, updated_at
            ) VALUES (?1, 'User One', 'user@example.com', 'user@example.com', 'user', 'password', 'active', 1, 'all', ?2, ?2)
            "#,
            libsql::params![user_id.to_string(), now],
        )
        .await
        .expect("user");

        conn.execute(
            r#"
            INSERT INTO budgets (
                budget_id, scope_kind, scope_key, user_id, cadence, amount_10000,
                hard_limit, timezone, is_active, created_at, updated_at
            ) VALUES (?1, 'user', ?2, ?3, 'daily', 100000, 1, 'UTC', 1, ?4, ?4)
            "#,
            libsql::params![
                Uuid::new_v4().to_string(),
                format!("budget:v1:user:{user_id}"),
                user_id.to_string(),
                now
            ],
        )
        .await
        .expect("first budget");

        let duplicate_active_result = conn
            .execute(
                r#"
                INSERT INTO budgets (
                    budget_id, scope_kind, scope_key, user_id, cadence, amount_10000,
                    hard_limit, timezone, is_active, created_at, updated_at
                ) VALUES (?1, 'user', ?2, ?3, 'weekly', 200000, 1, 'UTC', 1, ?4, ?4)
                "#,
                libsql::params![
                    Uuid::new_v4().to_string(),
                    format!("budget:v1:user:{user_id}"),
                    user_id.to_string(),
                    now
                ],
            )
            .await;
        assert!(duplicate_active_result.is_err());

        conn.execute(
            r#"
            INSERT INTO budgets (
                budget_id, scope_kind, scope_key, user_id, cadence, amount_10000,
                hard_limit, timezone, is_active, created_at, updated_at
            ) VALUES (?1, 'user', ?2, ?3, 'weekly', 200000, 1, 'UTC', 0, ?4, ?4)
            "#,
            libsql::params![
                Uuid::new_v4().to_string(),
                format!("budget:v1:user:{user_id}"),
                user_id.to_string(),
                now
            ],
        )
        .await
        .expect("inactive budget should be allowed");
    }

    #[tokio::test]
    #[serial]
    async fn service_account_budget_scope_enforces_single_active_record_per_scope_key() {
        let tmp = tempdir().expect("tempdir");
        let db_path = tmp.path().join("gateway.db");
        run_migrations(&db_path).await.expect("migrations");
        let db = libsql::Builder::new_local(db_path.to_str().expect("db path"))
            .build()
            .await
            .expect("db");
        let conn = db.connect().expect("connection");
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let team_id = Uuid::new_v4();
        let service_account_id = Uuid::new_v4();

        conn.execute(
            r#"
            INSERT INTO teams (
              team_id, team_key, team_name, status, model_access_mode, created_at, updated_at
            ) VALUES (?1, 'platform', 'Platform', 'active', 'all', ?2, ?2)
            "#,
            libsql::params![team_id.to_string(), now],
        )
        .await
        .expect("team");

        conn.execute(
            r#"
            INSERT INTO service_accounts (
              service_account_id, team_id, service_account_key, service_account_name,
              status, model_access_mode, metadata_json, created_at, updated_at
            ) VALUES (?1, ?2, 'svc', 'Service', 'active', 'all', '{}', ?3, ?3)
            "#,
            libsql::params![service_account_id.to_string(), team_id.to_string(), now],
        )
        .await
        .expect("service account");

        conn.execute(
            r#"
            INSERT INTO budgets (
                budget_id, scope_kind, scope_key, service_account_id, cadence, amount_10000,
                hard_limit, timezone, is_active, created_at, updated_at
            ) VALUES (?1, 'service_account', ?2, ?3, 'daily', 100000, 1, 'UTC', 1, ?4, ?4)
            "#,
            libsql::params![
                Uuid::new_v4().to_string(),
                format!("budget:v1:service_account:{service_account_id}"),
                service_account_id.to_string(),
                now
            ],
        )
        .await
        .expect("first budget");

        let duplicate_active_result = conn
            .execute(
                r#"
                INSERT INTO budgets (
                    budget_id, scope_kind, scope_key, service_account_id, cadence, amount_10000,
                    hard_limit, timezone, is_active, created_at, updated_at
                ) VALUES (?1, 'service_account', ?2, ?3, 'weekly', 200000, 1, 'UTC', 1, ?4, ?4)
                "#,
                libsql::params![
                    Uuid::new_v4().to_string(),
                    format!("budget:v1:service_account:{service_account_id}"),
                    service_account_id.to_string(),
                    now
                ],
            )
            .await;
        assert!(duplicate_active_result.is_err());

        conn.execute(
            r#"
            INSERT INTO budgets (
                budget_id, scope_kind, scope_key, service_account_id, cadence, amount_10000,
                hard_limit, timezone, is_active, created_at, updated_at
            ) VALUES (?1, 'service_account', ?2, ?3, 'weekly', 200000, 1, 'UTC', 0, ?4, ?4)
            "#,
            libsql::params![
                Uuid::new_v4().to_string(),
                format!("budget:v1:service_account:{service_account_id}"),
                service_account_id.to_string(),
                now
            ],
        )
        .await
        .expect("inactive budget should be allowed");
    }

    #[tokio::test]
    #[serial]
    async fn libsql_budget_alert_repository_tracks_history_and_delivery_lifecycle() {
        let tmp = tempdir().expect("tempdir");
        let db_path = tmp.path().join("gateway.db");
        run_migrations(&db_path).await.expect("migrations");

        let store = LibsqlStore::new_local(db_path.to_str().expect("db path"))
            .await
            .expect("store");

        exercise_budget_alert_repository(&store).await;
    }

    #[tokio::test]
    #[serial]
    async fn bootstrap_admin_password_state_round_trips() {
        let tmp = tempdir().expect("tempdir");
        let db_path = tmp.path().join("gateway.db");
        run_migrations(&db_path).await.expect("migrations");

        let store = LibsqlStore::new_local(db_path.to_str().expect("db path"))
            .await
            .expect("store");
        let created_at = time::OffsetDateTime::now_utc();

        let user = store
            .upsert_bootstrap_admin_user("Admin", "admin@local", true)
            .await
            .expect("bootstrap user");
        assert!(user.must_change_password);

        store
            .store_user_password(user.user_id, "hash-1", created_at)
            .await
            .expect("store password");

        let password_auth = store
            .get_user_password_auth(user.user_id)
            .await
            .expect("load password auth")
            .expect("password auth should exist");
        assert_eq!(password_auth.password_hash, "hash-1");

        store
            .update_user_must_change_password(user.user_id, false, created_at)
            .await
            .expect("clear forced password change");

        let refreshed = store
            .get_user_by_id(user.user_id)
            .await
            .expect("reload user")
            .expect("user should exist");
        assert!(!refreshed.must_change_password);
    }

    #[tokio::test]
    #[serial]
    async fn libsql_seed_round_trips_oauth_provider_allowed_email_domains() {
        let tmp = tempdir().expect("tempdir");
        let db_path = tmp.path().join("gateway.db");
        run_migrations(&db_path).await.expect("migrations");

        let store = LibsqlStore::new_local(db_path.to_str().expect("db path"))
            .await
            .expect("store");

        store
            .seed_from_inputs(
                &[],
                &[],
                &[],
                &[],
                &[seed_github_oauth_provider_with_domains(vec!["test.com"])],
                &[],
                &[],
            )
            .await
            .expect("seed provider");
        let providers = store
            .list_enabled_oauth_providers()
            .await
            .expect("list oauth providers");
        assert_eq!(providers.len(), 1);
        assert_eq!(
            providers[0].allowed_email_domains,
            vec!["test.com".to_string()]
        );

        store
            .seed_from_inputs(
                &[],
                &[],
                &[],
                &[],
                &[seed_github_oauth_provider_with_domains(vec![
                    "example.com",
                    "team.example.com",
                ])],
                &[],
                &[],
            )
            .await
            .expect("update provider");
        let provider = store
            .get_enabled_oauth_provider_by_key("github")
            .await
            .expect("get oauth provider")
            .expect("provider exists");
        assert_eq!(
            provider.allowed_email_domains,
            vec!["example.com".to_string(), "team.example.com".to_string()]
        );
    }

    #[tokio::test]
    #[serial]
    async fn postgres_seed_round_trips_oauth_provider_allowed_email_domains() {
        let Some(test_db) = create_postgres_test_database().await else {
            eprintln!(
                "skipping postgres oauth provider allowed email domains test because TEST_POSTGRES_URL is not set"
            );
            return;
        };

        let options = StoreConnectionOptions::Postgres {
            url: test_db.database_url.clone(),
            max_connections: 4,
        };
        run_migrations_with_options(&options)
            .await
            .expect("postgres migrations");

        let store = PostgresStore::connect(&test_db.database_url, 4)
            .await
            .expect("postgres store");

        store
            .seed_from_inputs(
                &[],
                &[],
                &[],
                &[],
                &[seed_github_oauth_provider_with_domains(vec!["test.com"])],
                &[],
                &[],
            )
            .await
            .expect("seed provider");
        let providers = store
            .list_enabled_oauth_providers()
            .await
            .expect("list oauth providers");
        assert_eq!(providers.len(), 1);
        assert_eq!(
            providers[0].allowed_email_domains,
            vec!["test.com".to_string()]
        );

        store
            .seed_from_inputs(
                &[],
                &[],
                &[],
                &[],
                &[seed_github_oauth_provider_with_domains(vec![
                    "example.com",
                    "team.example.com",
                ])],
                &[],
                &[],
            )
            .await
            .expect("update provider");
        let provider = store
            .get_enabled_oauth_provider_by_key("github")
            .await
            .expect("get oauth provider")
            .expect("provider exists");
        assert_eq!(
            provider.allowed_email_domains,
            vec!["example.com".to_string(), "team.example.com".to_string()]
        );
    }

    #[tokio::test]
    #[serial]
    async fn libsql_seed_reconciles_declarative_teams_users_memberships_and_budgets() {
        let tmp = tempdir().expect("tempdir");
        let db_path = tmp.path().join("gateway.db");
        run_migrations(&db_path).await.expect("migrations");

        let store = LibsqlStore::new_local(db_path.to_str().expect("db path"))
            .await
            .expect("store");

        let initial_teams = vec![
            SeedTeam {
                team_key: "platform".to_string(),
                team_name: "Platform".to_string(),
            },
            SeedTeam {
                team_key: "ops".to_string(),
                team_name: "Ops".to_string(),
            },
        ];
        let initial_users = vec![SeedUser {
            name: "Member".to_string(),
            email: "member@example.com".to_string(),
            email_normalized: "member@example.com".to_string(),
            global_role: GlobalRole::User,
            auth_mode: AuthMode::Password,
            request_logging_enabled: false,
            oidc_provider_key: None,
            oauth_provider_key: None,
            membership: Some(SeedUserMembership {
                team_key: "platform".to_string(),
                role: MembershipRole::Admin,
            }),
            budget: Some(SeedBudget {
                cadence: BudgetCadence::Weekly,
                amount_usd: Money4::from_scaled(750_000),
                hard_limit: true,
                timezone: "UTC".to_string(),
            }),
        }];

        store
            .seed_from_inputs(&[], &[], &[], &[], &[], &initial_teams, &initial_users)
            .await
            .expect("initial seed");

        let platform_team = store
            .get_team_by_key("platform")
            .await
            .expect("load team")
            .expect("team exists");
        let user = store
            .get_user_by_email_normalized("member@example.com")
            .await
            .expect("load user")
            .expect("user exists");
        assert!(!user.request_logging_enabled);
        assert_eq!(user.auth_mode, AuthMode::Password);
        assert_eq!(user.status, UserStatus::Invited);
        let identity_user = store
            .get_identity_user(user.user_id)
            .await
            .expect("identity user")
            .expect("identity user exists");
        assert_eq!(identity_user.team_name.as_deref(), Some("Platform"));
        assert_eq!(identity_user.membership_role, Some(MembershipRole::Admin));
        let _platform_team_id = platform_team.team_id;
        assert_eq!(
            store
                .get_active_budget_by_scope(&BudgetScope::User {
                    user_id: user.user_id,
                })
                .await
                .expect("user budget")
                .expect("user budget exists")
                .settings
                .amount_usd,
            Money4::from_scaled(750_000)
        );

        let oidc_provider_id = insert_libsql_oidc_provider(&store, "okta").await;

        let updated_teams = vec![
            SeedTeam {
                team_key: "platform".to_string(),
                team_name: "Platform Engineering".to_string(),
            },
            SeedTeam {
                team_key: "ops".to_string(),
                team_name: "Operations".to_string(),
            },
        ];
        let updated_users = vec![SeedUser {
            name: "Member".to_string(),
            email: "member@example.com".to_string(),
            email_normalized: "member@example.com".to_string(),
            global_role: GlobalRole::User,
            auth_mode: AuthMode::Oidc,
            request_logging_enabled: true,
            oidc_provider_key: Some("okta".to_string()),
            oauth_provider_key: None,
            membership: Some(SeedUserMembership {
                team_key: "ops".to_string(),
                role: MembershipRole::Member,
            }),
            budget: None,
        }];

        store
            .seed_from_inputs(&[], &[], &[], &[], &[], &updated_teams, &updated_users)
            .await
            .expect("updated seed");
        store
            .seed_from_inputs(&[], &[], &[], &[], &[], &updated_teams, &updated_users)
            .await
            .expect("updated seed idempotent");

        let refreshed_user = store
            .get_user_by_email_normalized("member@example.com")
            .await
            .expect("reload user")
            .expect("user exists");
        assert_eq!(refreshed_user.user_id, user.user_id);
        assert!(refreshed_user.request_logging_enabled);
        assert_eq!(refreshed_user.auth_mode, AuthMode::Oidc);

        let refreshed_identity = store
            .get_identity_user(user.user_id)
            .await
            .expect("reload identity user")
            .expect("identity user exists");
        assert_eq!(refreshed_identity.team_name.as_deref(), Some("Operations"));
        assert_eq!(
            refreshed_identity.membership_role,
            Some(MembershipRole::Member)
        );
        assert_eq!(
            refreshed_identity.oidc_provider_id.as_deref(),
            Some(oidc_provider_id.as_str())
        );
        assert_eq!(
            refreshed_identity.oidc_provider_key.as_deref(),
            Some("okta")
        );
        assert!(
            store
                .find_invited_oidc_user("member@example.com", &oidc_provider_id)
                .await
                .expect("find invited oidc user")
                .is_some()
        );

        let refreshed_platform = store
            .get_team_by_key("platform")
            .await
            .expect("reload platform team")
            .expect("platform team exists");
        let ops_team = store
            .get_team_by_key("ops")
            .await
            .expect("reload ops team")
            .expect("ops team exists");
        assert_eq!(refreshed_platform.team_name, "Platform Engineering");
        let _ops_team_id = ops_team.team_id;
        assert!(
            store
                .get_active_budget_by_scope(&BudgetScope::User {
                    user_id: user.user_id,
                })
                .await
                .expect("user budget after reseed")
                .is_none()
        );
    }

    #[tokio::test]
    #[serial]
    async fn libsql_seed_rejects_illegal_auth_mode_change_without_partial_profile_updates() {
        let tmp = tempdir().expect("tempdir");
        let db_path = tmp.path().join("gateway.db");
        run_migrations(&db_path).await.expect("migrations");

        let store = LibsqlStore::new_local(db_path.to_str().expect("db path"))
            .await
            .expect("store");

        let initial_teams = vec![SeedTeam {
            team_key: "platform".to_string(),
            team_name: "Platform".to_string(),
        }];
        let initial_users = vec![SeedUser {
            name: "Member".to_string(),
            email: "member@example.com".to_string(),
            email_normalized: "member@example.com".to_string(),
            global_role: GlobalRole::User,
            auth_mode: AuthMode::Password,
            request_logging_enabled: false,
            oidc_provider_key: None,
            oauth_provider_key: None,
            membership: None,
            budget: None,
        }];

        store
            .seed_from_inputs(&[], &[], &[], &[], &[], &initial_teams, &initial_users)
            .await
            .expect("initial seed");

        let platform_team = store
            .get_team_by_key("platform")
            .await
            .expect("load team")
            .expect("team exists");

        let user = store
            .get_user_by_email_normalized("member@example.com")
            .await
            .expect("load user")
            .expect("user exists");
        store
            .update_user_status(user.user_id, UserStatus::Active, OffsetDateTime::now_utc())
            .await
            .expect("activate user");

        insert_libsql_oidc_provider(&store, "okta").await;

        let invalid_users = vec![SeedUser {
            name: "Updated Member".to_string(),
            email: "member@example.com".to_string(),
            email_normalized: "member@example.com".to_string(),
            global_role: GlobalRole::User,
            auth_mode: AuthMode::Oidc,
            request_logging_enabled: true,
            oidc_provider_key: Some("okta".to_string()),
            oauth_provider_key: None,
            membership: None,
            budget: None,
        }];
        let invalid_teams = vec![SeedTeam {
            team_key: "platform".to_string(),
            team_name: "Platform Renamed".to_string(),
        }];

        let error = store
            .seed_from_inputs(&[], &[], &[], &[], &[], &invalid_teams, &invalid_users)
            .await
            .expect_err("seed should fail");
        assert!(
            matches!(error, StoreError::Conflict(message) if message == "auth mode can only change while the user is invited")
        );

        let refreshed_user = store
            .get_user_by_email_normalized("member@example.com")
            .await
            .expect("reload user")
            .expect("user exists");
        assert_eq!(refreshed_user.name, "Member");
        assert!(!refreshed_user.request_logging_enabled);
        assert_eq!(refreshed_user.auth_mode, AuthMode::Password);
        let refreshed_team = store
            .get_team_by_key("platform")
            .await
            .expect("reload team")
            .expect("team exists");
        assert_eq!(refreshed_team.team_name, "Platform");
        let _platform_team_id = platform_team.team_id;
    }

    #[tokio::test]
    #[serial]
    async fn libsql_seed_rejects_non_invited_oidc_provider_swap_without_partial_profile_updates() {
        let tmp = tempdir().expect("tempdir");
        let db_path = tmp.path().join("gateway.db");
        run_migrations(&db_path).await.expect("migrations");

        let store = LibsqlStore::new_local(db_path.to_str().expect("db path"))
            .await
            .expect("store");

        let okta_provider_id = insert_libsql_oidc_provider(&store, "okta").await;
        let auth0_provider_id = insert_libsql_oidc_provider(&store, "auth0").await;

        let initial_users = vec![SeedUser {
            name: "Member".to_string(),
            email: "member@example.com".to_string(),
            email_normalized: "member@example.com".to_string(),
            global_role: GlobalRole::User,
            auth_mode: AuthMode::Oidc,
            request_logging_enabled: false,
            oidc_provider_key: Some("okta".to_string()),
            oauth_provider_key: None,
            membership: None,
            budget: None,
        }];

        store
            .seed_from_inputs(&[], &[], &[], &[], &[], &[], &initial_users)
            .await
            .expect("initial seed");

        let user = store
            .get_user_by_email_normalized("member@example.com")
            .await
            .expect("load user")
            .expect("user exists");
        store
            .create_user_oidc_auth(
                user.user_id,
                &okta_provider_id,
                "mock:okta:member@example.com",
                Some("member@example.com"),
                OffsetDateTime::now_utc(),
            )
            .await
            .expect("create oidc auth");
        store
            .update_user_status(user.user_id, UserStatus::Active, OffsetDateTime::now_utc())
            .await
            .expect("activate user");

        let invalid_users = vec![SeedUser {
            name: "Updated Member".to_string(),
            email: "member@example.com".to_string(),
            email_normalized: "member@example.com".to_string(),
            global_role: GlobalRole::User,
            auth_mode: AuthMode::Oidc,
            request_logging_enabled: true,
            oidc_provider_key: Some("auth0".to_string()),
            oauth_provider_key: None,
            membership: None,
            budget: None,
        }];

        let error = store
            .seed_from_inputs(&[], &[], &[], &[], &[], &[], &invalid_users)
            .await
            .expect_err("seed should fail");
        assert!(
            matches!(error, StoreError::Conflict(message) if message == "oidc provider can only change while the user is invited")
        );

        let refreshed_user = store
            .get_user_by_email_normalized("member@example.com")
            .await
            .expect("reload user")
            .expect("user exists");
        assert_eq!(refreshed_user.name, "Member");
        assert!(!refreshed_user.request_logging_enabled);
        assert_eq!(refreshed_user.auth_mode, AuthMode::Oidc);

        let refreshed_identity = store
            .get_identity_user(user.user_id)
            .await
            .expect("reload identity user")
            .expect("identity user exists");
        assert_eq!(
            refreshed_identity.oidc_provider_id.as_deref(),
            Some(okta_provider_id.as_str())
        );
        assert!(
            store
                .get_user_oidc_auth_by_user(user.user_id, &okta_provider_id)
                .await
                .expect("lookup okta auth")
                .is_some()
        );
        assert!(
            store
                .get_user_oidc_auth_by_user(user.user_id, &auth0_provider_id)
                .await
                .expect("lookup auth0 auth")
                .is_none()
        );
    }

    #[tokio::test]
    #[serial]
    async fn libsql_seed_rejects_last_active_platform_admin_demotion_without_partial_profile_updates()
     {
        let tmp = tempdir().expect("tempdir");
        let db_path = tmp.path().join("gateway.db");
        run_migrations(&db_path).await.expect("migrations");

        let store = LibsqlStore::new_local(db_path.to_str().expect("db path"))
            .await
            .expect("store");

        let initial_users = vec![SeedUser {
            name: "Platform Admin".to_string(),
            email: "admin@example.com".to_string(),
            email_normalized: "admin@example.com".to_string(),
            global_role: GlobalRole::PlatformAdmin,
            auth_mode: AuthMode::Password,
            request_logging_enabled: false,
            oidc_provider_key: None,
            oauth_provider_key: None,
            membership: None,
            budget: None,
        }];

        store
            .seed_from_inputs(&[], &[], &[], &[], &[], &[], &initial_users)
            .await
            .expect("initial seed");

        let user = store
            .get_user_by_email_normalized("admin@example.com")
            .await
            .expect("load user")
            .expect("user exists");
        store
            .update_user_status(user.user_id, UserStatus::Active, OffsetDateTime::now_utc())
            .await
            .expect("activate user");

        let invalid_users = vec![SeedUser {
            name: "Renamed Admin".to_string(),
            email: "admin@example.com".to_string(),
            email_normalized: "admin@example.com".to_string(),
            global_role: GlobalRole::User,
            auth_mode: AuthMode::Password,
            request_logging_enabled: true,
            oidc_provider_key: None,
            oauth_provider_key: None,
            membership: None,
            budget: None,
        }];

        let error = store
            .seed_from_inputs(&[], &[], &[], &[], &[], &[], &invalid_users)
            .await
            .expect_err("seed should fail");
        assert!(
            matches!(error, StoreError::Conflict(message) if message == "the last active platform admin cannot be deactivated or demoted")
        );

        let refreshed_user = store
            .get_user_by_email_normalized("admin@example.com")
            .await
            .expect("reload user")
            .expect("user exists");
        assert_eq!(refreshed_user.name, "Platform Admin");
        assert!(!refreshed_user.request_logging_enabled);
        assert_eq!(refreshed_user.global_role, GlobalRole::PlatformAdmin);
    }

    #[tokio::test]
    #[serial]
    async fn postgres_seed_reconciles_declarative_teams_users_memberships_and_budgets() {
        let Some(test_db) = create_postgres_test_database().await else {
            eprintln!(
                "skipping postgres declarative seed test because TEST_POSTGRES_URL is not set"
            );
            return;
        };

        let options = StoreConnectionOptions::Postgres {
            url: test_db.database_url.clone(),
            max_connections: 4,
        };
        run_migrations_with_options(&options)
            .await
            .expect("postgres migrations");

        let store = PostgresStore::connect(&test_db.database_url, 4)
            .await
            .expect("postgres store");

        let initial_teams = vec![
            SeedTeam {
                team_key: "platform".to_string(),
                team_name: "Platform".to_string(),
            },
            SeedTeam {
                team_key: "ops".to_string(),
                team_name: "Ops".to_string(),
            },
        ];
        let initial_users = vec![SeedUser {
            name: "Member".to_string(),
            email: "member@example.com".to_string(),
            email_normalized: "member@example.com".to_string(),
            global_role: GlobalRole::User,
            auth_mode: AuthMode::Password,
            request_logging_enabled: false,
            oidc_provider_key: None,
            oauth_provider_key: None,
            membership: Some(SeedUserMembership {
                team_key: "platform".to_string(),
                role: MembershipRole::Admin,
            }),
            budget: Some(SeedBudget {
                cadence: BudgetCadence::Weekly,
                amount_usd: Money4::from_scaled(750_000),
                hard_limit: true,
                timezone: "UTC".to_string(),
            }),
        }];

        store
            .seed_from_inputs(&[], &[], &[], &[], &[], &initial_teams, &initial_users)
            .await
            .expect("initial seed");

        let platform_team = store
            .get_team_by_key("platform")
            .await
            .expect("load team")
            .expect("team exists");
        let user = store
            .get_user_by_email_normalized("member@example.com")
            .await
            .expect("load user")
            .expect("user exists");
        assert!(!user.request_logging_enabled);
        assert_eq!(user.auth_mode, AuthMode::Password);
        assert_eq!(user.status, UserStatus::Invited);
        let identity_user = store
            .get_identity_user(user.user_id)
            .await
            .expect("identity user")
            .expect("identity user exists");
        assert_eq!(identity_user.team_name.as_deref(), Some("Platform"));
        assert_eq!(identity_user.membership_role, Some(MembershipRole::Admin));
        let _platform_team_id = platform_team.team_id;
        assert_eq!(
            store
                .get_active_budget_by_scope(&BudgetScope::User {
                    user_id: user.user_id,
                })
                .await
                .expect("user budget")
                .expect("user budget exists")
                .settings
                .amount_usd,
            Money4::from_scaled(750_000)
        );

        let oidc_provider_id = insert_postgres_oidc_provider(&store, "okta").await;

        let updated_teams = vec![
            SeedTeam {
                team_key: "platform".to_string(),
                team_name: "Platform Engineering".to_string(),
            },
            SeedTeam {
                team_key: "ops".to_string(),
                team_name: "Operations".to_string(),
            },
        ];
        let updated_users = vec![SeedUser {
            name: "Member".to_string(),
            email: "member@example.com".to_string(),
            email_normalized: "member@example.com".to_string(),
            global_role: GlobalRole::User,
            auth_mode: AuthMode::Oidc,
            request_logging_enabled: true,
            oidc_provider_key: Some("okta".to_string()),
            oauth_provider_key: None,
            membership: Some(SeedUserMembership {
                team_key: "ops".to_string(),
                role: MembershipRole::Member,
            }),
            budget: None,
        }];

        store
            .seed_from_inputs(&[], &[], &[], &[], &[], &updated_teams, &updated_users)
            .await
            .expect("updated seed");
        store
            .seed_from_inputs(&[], &[], &[], &[], &[], &updated_teams, &updated_users)
            .await
            .expect("updated seed idempotent");

        let refreshed_user = store
            .get_user_by_email_normalized("member@example.com")
            .await
            .expect("reload user")
            .expect("user exists");
        assert_eq!(refreshed_user.user_id, user.user_id);
        assert!(refreshed_user.request_logging_enabled);
        assert_eq!(refreshed_user.auth_mode, AuthMode::Oidc);

        let refreshed_identity = store
            .get_identity_user(user.user_id)
            .await
            .expect("reload identity user")
            .expect("identity user exists");
        assert_eq!(refreshed_identity.team_name.as_deref(), Some("Operations"));
        assert_eq!(
            refreshed_identity.membership_role,
            Some(MembershipRole::Member)
        );
        assert_eq!(
            refreshed_identity.oidc_provider_id.as_deref(),
            Some(oidc_provider_id.as_str())
        );
        assert_eq!(
            refreshed_identity.oidc_provider_key.as_deref(),
            Some("okta")
        );
        assert!(
            store
                .find_invited_oidc_user("member@example.com", &oidc_provider_id)
                .await
                .expect("find invited oidc user")
                .is_some()
        );

        let refreshed_platform = store
            .get_team_by_key("platform")
            .await
            .expect("reload platform team")
            .expect("platform team exists");
        let ops_team = store
            .get_team_by_key("ops")
            .await
            .expect("reload ops team")
            .expect("ops team exists");
        assert_eq!(refreshed_platform.team_name, "Platform Engineering");
        let _ops_team_id = ops_team.team_id;
        assert!(
            store
                .get_active_budget_by_scope(&BudgetScope::User {
                    user_id: user.user_id,
                })
                .await
                .expect("user budget after reseed")
                .is_none()
        );

        store.pool().close().await;
        drop_postgres_test_database(&test_db).await;
    }

    #[tokio::test]
    #[serial]
    async fn postgres_seed_rejects_illegal_auth_mode_change_without_partial_profile_updates() {
        let Some(test_db) = create_postgres_test_database().await else {
            eprintln!(
                "skipping postgres auth-mode mutability seed test because TEST_POSTGRES_URL is not set"
            );
            return;
        };

        let options = StoreConnectionOptions::Postgres {
            url: test_db.database_url.clone(),
            max_connections: 4,
        };
        run_migrations_with_options(&options)
            .await
            .expect("postgres migrations");

        let store = PostgresStore::connect(&test_db.database_url, 4)
            .await
            .expect("postgres store");

        let initial_teams = vec![SeedTeam {
            team_key: "platform".to_string(),
            team_name: "Platform".to_string(),
        }];
        let initial_users = vec![SeedUser {
            name: "Member".to_string(),
            email: "member@example.com".to_string(),
            email_normalized: "member@example.com".to_string(),
            global_role: GlobalRole::User,
            auth_mode: AuthMode::Password,
            request_logging_enabled: false,
            oidc_provider_key: None,
            oauth_provider_key: None,
            membership: None,
            budget: None,
        }];

        store
            .seed_from_inputs(&[], &[], &[], &[], &[], &initial_teams, &initial_users)
            .await
            .expect("initial seed");

        let platform_team = store
            .get_team_by_key("platform")
            .await
            .expect("load team")
            .expect("team exists");

        let user = store
            .get_user_by_email_normalized("member@example.com")
            .await
            .expect("load user")
            .expect("user exists");
        store
            .update_user_status(user.user_id, UserStatus::Active, OffsetDateTime::now_utc())
            .await
            .expect("activate user");

        insert_postgres_oidc_provider(&store, "okta").await;

        let invalid_users = vec![SeedUser {
            name: "Updated Member".to_string(),
            email: "member@example.com".to_string(),
            email_normalized: "member@example.com".to_string(),
            global_role: GlobalRole::User,
            auth_mode: AuthMode::Oidc,
            request_logging_enabled: true,
            oidc_provider_key: Some("okta".to_string()),
            oauth_provider_key: None,
            membership: None,
            budget: None,
        }];
        let invalid_teams = vec![SeedTeam {
            team_key: "platform".to_string(),
            team_name: "Platform Renamed".to_string(),
        }];

        let error = store
            .seed_from_inputs(&[], &[], &[], &[], &[], &invalid_teams, &invalid_users)
            .await
            .expect_err("seed should fail");
        assert!(
            matches!(error, StoreError::Conflict(message) if message == "auth mode can only change while the user is invited")
        );

        let refreshed_user = store
            .get_user_by_email_normalized("member@example.com")
            .await
            .expect("reload user")
            .expect("user exists");
        assert_eq!(refreshed_user.name, "Member");
        assert!(!refreshed_user.request_logging_enabled);
        assert_eq!(refreshed_user.auth_mode, AuthMode::Password);
        let refreshed_team = store
            .get_team_by_key("platform")
            .await
            .expect("reload team")
            .expect("team exists");
        assert_eq!(refreshed_team.team_name, "Platform");
        let _platform_team_id = platform_team.team_id;

        store.pool().close().await;
        drop_postgres_test_database(&test_db).await;
    }

    #[tokio::test]
    #[serial]
    async fn postgres_seed_rejects_non_invited_oidc_provider_swap_without_partial_profile_updates()
    {
        let Some(test_db) = create_postgres_test_database().await else {
            eprintln!(
                "skipping postgres oidc-provider mutability seed test because TEST_POSTGRES_URL is not set"
            );
            return;
        };

        let options = StoreConnectionOptions::Postgres {
            url: test_db.database_url.clone(),
            max_connections: 4,
        };
        run_migrations_with_options(&options)
            .await
            .expect("postgres migrations");

        let store = PostgresStore::connect(&test_db.database_url, 4)
            .await
            .expect("postgres store");

        let okta_provider_id = insert_postgres_oidc_provider(&store, "okta").await;
        let auth0_provider_id = insert_postgres_oidc_provider(&store, "auth0").await;

        let initial_users = vec![SeedUser {
            name: "Member".to_string(),
            email: "member@example.com".to_string(),
            email_normalized: "member@example.com".to_string(),
            global_role: GlobalRole::User,
            auth_mode: AuthMode::Oidc,
            request_logging_enabled: false,
            oidc_provider_key: Some("okta".to_string()),
            oauth_provider_key: None,
            membership: None,
            budget: None,
        }];

        store
            .seed_from_inputs(&[], &[], &[], &[], &[], &[], &initial_users)
            .await
            .expect("initial seed");

        let user = store
            .get_user_by_email_normalized("member@example.com")
            .await
            .expect("load user")
            .expect("user exists");
        store
            .create_user_oidc_auth(
                user.user_id,
                &okta_provider_id,
                "mock:okta:member@example.com",
                Some("member@example.com"),
                OffsetDateTime::now_utc(),
            )
            .await
            .expect("create oidc auth");
        store
            .update_user_status(user.user_id, UserStatus::Active, OffsetDateTime::now_utc())
            .await
            .expect("activate user");

        let invalid_users = vec![SeedUser {
            name: "Updated Member".to_string(),
            email: "member@example.com".to_string(),
            email_normalized: "member@example.com".to_string(),
            global_role: GlobalRole::User,
            auth_mode: AuthMode::Oidc,
            request_logging_enabled: true,
            oidc_provider_key: Some("auth0".to_string()),
            oauth_provider_key: None,
            membership: None,
            budget: None,
        }];

        let error = store
            .seed_from_inputs(&[], &[], &[], &[], &[], &[], &invalid_users)
            .await
            .expect_err("seed should fail");
        assert!(
            matches!(error, StoreError::Conflict(message) if message == "oidc provider can only change while the user is invited")
        );

        let refreshed_user = store
            .get_user_by_email_normalized("member@example.com")
            .await
            .expect("reload user")
            .expect("user exists");
        assert_eq!(refreshed_user.name, "Member");
        assert!(!refreshed_user.request_logging_enabled);
        assert_eq!(refreshed_user.auth_mode, AuthMode::Oidc);

        let refreshed_identity = store
            .get_identity_user(user.user_id)
            .await
            .expect("reload identity user")
            .expect("identity user exists");
        assert_eq!(
            refreshed_identity.oidc_provider_id.as_deref(),
            Some(okta_provider_id.as_str())
        );
        assert!(
            store
                .get_user_oidc_auth_by_user(user.user_id, &okta_provider_id)
                .await
                .expect("lookup okta auth")
                .is_some()
        );
        assert!(
            store
                .get_user_oidc_auth_by_user(user.user_id, &auth0_provider_id)
                .await
                .expect("lookup auth0 auth")
                .is_none()
        );

        store.pool().close().await;
        drop_postgres_test_database(&test_db).await;
    }

    #[tokio::test]
    #[serial]
    async fn postgres_seed_rejects_last_active_platform_admin_demotion_without_partial_profile_updates()
     {
        let Some(test_db) = create_postgres_test_database().await else {
            eprintln!(
                "skipping postgres last-platform-admin seed test because TEST_POSTGRES_URL is not set"
            );
            return;
        };

        let options = StoreConnectionOptions::Postgres {
            url: test_db.database_url.clone(),
            max_connections: 4,
        };
        run_migrations_with_options(&options)
            .await
            .expect("postgres migrations");

        let store = PostgresStore::connect(&test_db.database_url, 4)
            .await
            .expect("postgres store");

        let initial_users = vec![SeedUser {
            name: "Platform Admin".to_string(),
            email: "admin@example.com".to_string(),
            email_normalized: "admin@example.com".to_string(),
            global_role: GlobalRole::PlatformAdmin,
            auth_mode: AuthMode::Password,
            request_logging_enabled: false,
            oidc_provider_key: None,
            oauth_provider_key: None,
            membership: None,
            budget: None,
        }];

        store
            .seed_from_inputs(&[], &[], &[], &[], &[], &[], &initial_users)
            .await
            .expect("initial seed");

        let user = store
            .get_user_by_email_normalized("admin@example.com")
            .await
            .expect("load user")
            .expect("user exists");
        store
            .update_user_status(user.user_id, UserStatus::Active, OffsetDateTime::now_utc())
            .await
            .expect("activate user");

        let invalid_users = vec![SeedUser {
            name: "Renamed Admin".to_string(),
            email: "admin@example.com".to_string(),
            email_normalized: "admin@example.com".to_string(),
            global_role: GlobalRole::User,
            auth_mode: AuthMode::Password,
            request_logging_enabled: true,
            oidc_provider_key: None,
            oauth_provider_key: None,
            membership: None,
            budget: None,
        }];

        let error = store
            .seed_from_inputs(&[], &[], &[], &[], &[], &[], &invalid_users)
            .await
            .expect_err("seed should fail");
        assert!(
            matches!(error, StoreError::Conflict(message) if message == "the last active platform admin cannot be deactivated or demoted")
        );

        let refreshed_user = store
            .get_user_by_email_normalized("admin@example.com")
            .await
            .expect("reload user")
            .expect("user exists");
        assert_eq!(refreshed_user.name, "Platform Admin");
        assert!(!refreshed_user.request_logging_enabled);
        assert_eq!(refreshed_user.global_role, GlobalRole::PlatformAdmin);

        store.pool().close().await;
        drop_postgres_test_database(&test_db).await;
    }

    #[tokio::test]
    #[serial]
    async fn libsql_spend_reporting_aggregates_and_service_account_window_sum_filter_chargeable_statuses()
     {
        let tmp = tempdir().expect("tempdir");
        let db_path = tmp.path().join("gateway.db");
        run_migrations(&db_path).await.expect("migrations");

        let store = LibsqlStore::new_local(db_path.to_str().expect("db path"))
            .await
            .expect("store");

        let providers = vec![SeedProvider {
            provider_key: "openai-prod".to_string(),
            provider_type: "openai_compat".to_string(),
            config: json!({
                "base_url": "https://api.openai.com/v1",
                "timeout_ms": 120_000
            }),
            secrets: Some(json!({"token": "env.OPENAI_API_KEY"})),
        }];
        let models = vec![SeedModel {
            model_key: "fast".to_string(),
            alias_target_model_key: None,
            description: Some("fast tier".to_string()),
            tags: vec!["fast".to_string()],
            rank: 10,
            routes: vec![SeedModelRoute {
                provider_key: "openai-prod".to_string(),
                upstream_model: "gpt-4o-mini".to_string(),
                priority: 10,
                weight: 1.0,
                enabled: true,
                extra_headers: Map::new(),
                extra_body: Map::new(),
                capabilities: ProviderCapabilities::all_enabled(),
                compatibility: Default::default(),
            }],
        }];
        let api_keys = vec![SeedApiKey {
            name: "dev".to_string(),
            public_id: "dev123".to_string(),
            secret_hash: "hash".to_string(),
            service_account_key: "seed-workloads".to_string(),
            service_account_name: "Seed Workloads".to_string(),
            service_account_team_key: "seed-workloads".to_string(),
            service_account_budget: SeedBudget {
                cadence: BudgetCadence::Daily,
                amount_usd: Money4::from_scaled(100_000),
                hard_limit: true,
                timezone: "UTC".to_string(),
            },
            allowed_models: vec!["fast".to_string()],
        }];
        store
            .seed_from_inputs(
                &providers,
                &models,
                &api_keys,
                &[],
                &[],
                &seed_api_key_teams(),
                &[],
            )
            .await
            .expect("seed");

        let api_key = store
            .get_api_key_by_public_id("dev123")
            .await
            .expect("load api key")
            .expect("api key");
        let model = store
            .get_model_by_key("fast")
            .await
            .expect("load model")
            .expect("model");
        let service_account_id = api_key
            .owner_service_account_id
            .expect("seed api key has service account owner");
        let team_id = api_key
            .owner_team_id
            .expect("seed service account has owning team");
        let user = store
            .create_identity_user(
                "Member",
                "member@example.com",
                "member@example.com",
                GlobalRole::User,
                AuthMode::Password,
                UserStatus::Active,
            )
            .await
            .expect("create user");
        let (user_tags, team_tags) = focus_export_owner_tags();
        store
            .update_user_tags(user.user_id, &user_tags, OffsetDateTime::now_utc())
            .await
            .expect("update user tags");
        store
            .update_team_tags(team_id, &team_tags, OffsetDateTime::now_utc())
            .await
            .expect("update team tags");

        let now = OffsetDateTime::from_unix_timestamp(1_773_484_800).expect("timestamp");
        let budget = store
            .upsert_active_budget(
                &BudgetScope::ServiceAccount { service_account_id },
                &BudgetSettings {
                    cadence: BudgetCadence::Daily,
                    amount_usd: Money4::from_scaled(100_000),
                    hard_limit: true,
                    timezone: "UTC".to_string(),
                },
                now,
            )
            .await
            .expect("upsert service account budget");
        assert_eq!(budget.settings.amount_usd, Money4::from_scaled(100_000));

        let day_one = OffsetDateTime::from_unix_timestamp(1_773_486_600).expect("day one");
        let day_two = day_one + Duration::days(1);

        for event in [
            build_usage_ledger_record(
                "req-user-priced",
                format!("user:{}", user.user_id),
                api_key.id,
                Some(user.user_id),
                None,
                None,
                Some(model.id),
                "gpt-4o-mini",
                UsagePricingStatus::Priced,
                11_000,
                day_one,
            ),
            build_usage_ledger_record(
                "req-user-unpriced",
                format!("user:{}", user.user_id),
                api_key.id,
                Some(user.user_id),
                None,
                None,
                Some(model.id),
                "gpt-4o-mini",
                UsagePricingStatus::Unpriced,
                0,
                day_one,
            ),
            build_usage_ledger_record(
                "req-service-account-legacy",
                format!("service_account:{service_account_id}"),
                api_key.id,
                None,
                Some(team_id),
                Some(service_account_id),
                None,
                "claude-3-5-sonnet",
                UsagePricingStatus::LegacyEstimated,
                22_000,
                day_two,
            ),
            build_usage_ledger_record(
                "req-service-account-unpriced",
                format!("service_account:{service_account_id}"),
                api_key.id,
                None,
                Some(team_id),
                Some(service_account_id),
                None,
                "claude-3-5-sonnet",
                UsagePricingStatus::Unpriced,
                0,
                day_two,
            ),
            build_usage_ledger_record(
                "req-service-account-usage-missing",
                format!("service_account:{service_account_id}"),
                api_key.id,
                None,
                Some(team_id),
                Some(service_account_id),
                None,
                "claude-3-5-sonnet",
                UsagePricingStatus::UsageMissing,
                0,
                day_two,
            ),
        ] {
            assert!(
                store
                    .insert_usage_ledger_if_absent(&event)
                    .await
                    .expect("insert usage ledger")
            );
        }

        let window_start = day_one - Duration::hours(1);
        let window_end = day_two + Duration::days(1);
        let service_account_sum = store
            .sum_usage_cost_for_budget_scope_in_window(
                &BudgetScope::ServiceAccount { service_account_id },
                window_start,
                window_end,
            )
            .await
            .expect("service account sum");
        assert_eq!(service_account_sum, Money4::from_scaled(22_000));

        let daily = store
            .list_usage_daily_aggregates(window_start, window_end, None)
            .await
            .expect("daily aggregates");
        assert_eq!(daily.len(), 2);
        let day_one_bucket = (day_one.unix_timestamp() / 86_400) * 86_400;
        let day_two_bucket = (day_two.unix_timestamp() / 86_400) * 86_400;
        let first = daily
            .iter()
            .find(|row| row.day_start.unix_timestamp() == day_one_bucket)
            .expect("day one aggregate");
        assert_eq!(first.priced_cost_usd, Money4::from_scaled(11_000));
        assert_eq!(first.priced_request_count, 1);
        assert_eq!(first.unpriced_request_count, 1);
        assert_eq!(first.usage_missing_request_count, 0);
        let second = daily
            .iter()
            .find(|row| row.day_start.unix_timestamp() == day_two_bucket)
            .expect("day two aggregate");
        assert_eq!(second.priced_cost_usd, Money4::from_scaled(22_000));
        assert_eq!(second.priced_request_count, 1);
        assert_eq!(second.unpriced_request_count, 1);
        assert_eq!(second.usage_missing_request_count, 1);

        let owners = store
            .list_usage_owner_aggregates(window_start, window_end, None)
            .await
            .expect("owner aggregates");
        assert_eq!(owners.len(), 2);
        let user_owner = owners
            .iter()
            .find(|row| row.owner_kind == ApiKeyOwnerKind::User)
            .expect("user owner aggregate");
        assert_eq!(user_owner.owner_id, user.user_id);
        assert_eq!(user_owner.priced_cost_usd, Money4::from_scaled(11_000));
        assert_eq!(user_owner.priced_request_count, 1);
        assert_eq!(user_owner.unpriced_request_count, 1);
        assert_eq!(user_owner.usage_missing_request_count, 0);
        let service_account_owner = owners
            .iter()
            .find(|row| row.owner_kind == ApiKeyOwnerKind::ServiceAccount)
            .expect("service account owner aggregate");
        assert_eq!(service_account_owner.owner_id, service_account_id);
        assert_eq!(
            service_account_owner.priced_cost_usd,
            Money4::from_scaled(22_000)
        );
        assert_eq!(service_account_owner.priced_request_count, 1);
        assert_eq!(service_account_owner.unpriced_request_count, 1);
        assert_eq!(service_account_owner.usage_missing_request_count, 1);

        let models = store
            .list_usage_model_aggregates(window_start, window_end, None)
            .await
            .expect("model aggregates");
        assert_eq!(models.len(), 2);
        let gateway_model = models
            .iter()
            .find(|row| row.model_key == "fast")
            .expect("gateway model aggregate");
        assert_eq!(gateway_model.priced_cost_usd, Money4::from_scaled(11_000));
        assert_eq!(gateway_model.priced_request_count, 1);
        assert_eq!(gateway_model.unpriced_request_count, 1);
        assert_eq!(gateway_model.usage_missing_request_count, 0);
        let upstream_model = models
            .iter()
            .find(|row| row.model_key == "claude-3-5-sonnet")
            .expect("upstream model aggregate");
        assert_eq!(upstream_model.priced_cost_usd, Money4::from_scaled(22_000));
        assert_eq!(upstream_model.priced_request_count, 1);
        assert_eq!(upstream_model.unpriced_request_count, 1);
        assert_eq!(upstream_model.usage_missing_request_count, 1);

        assert_focus_export_aggregates(
            &store,
            window_start,
            window_end,
            user.user_id,
            service_account_id,
        )
        .await;
    }

    #[tokio::test]
    #[serial]
    async fn libsql_usage_leaderboard_ranks_users_and_aggregates_half_day_buckets() {
        let tmp = tempdir().expect("tempdir");
        let db_path = tmp.path().join("gateway.db");
        run_migrations(&db_path).await.expect("migrations");

        let store = LibsqlStore::new_local(db_path.to_str().expect("db path"))
            .await
            .expect("store");

        exercise_usage_leaderboard_reporting(&store).await;
    }

    #[tokio::test]
    #[serial]
    async fn postgres_store_supports_migrations_and_core_operations() {
        let Some(test_db) = create_postgres_test_database().await else {
            eprintln!("skipping postgres store test because TEST_POSTGRES_URL is not set");
            return;
        };

        let options = StoreConnectionOptions::Postgres {
            url: test_db.database_url.clone(),
            max_connections: 4,
        };
        run_migrations_with_options(&options)
            .await
            .expect("postgres migrations");

        let store = PostgresStore::connect(&test_db.database_url, 4)
            .await
            .expect("postgres store");
        store.ping().await.expect("ping");

        let providers = vec![SeedProvider {
            provider_key: "openai-prod".to_string(),
            provider_type: "openai_compat".to_string(),
            config: json!({
                "base_url": "https://api.openai.com/v1",
                "timeout_ms": 120_000
            }),
            secrets: Some(json!({"token": "env.OPENAI_API_KEY"})),
        }];
        let models = vec![SeedModel {
            model_key: "fast".to_string(),
            alias_target_model_key: None,
            description: Some("fast tier".to_string()),
            tags: vec!["fast".to_string(), "cheap".to_string()],
            rank: 10,
            routes: vec![SeedModelRoute {
                provider_key: "openai-prod".to_string(),
                upstream_model: "gpt-4o-mini".to_string(),
                priority: 10,
                weight: 1.0,
                enabled: true,
                extra_headers: Map::new(),
                extra_body: Map::new(),
                capabilities: ProviderCapabilities::with_dimensions(
                    true, false, true, true, false, true, true,
                ),
                compatibility: Default::default(),
            }],
        }];
        let api_keys = vec![SeedApiKey {
            name: "dev".to_string(),
            public_id: "dev123".to_string(),
            secret_hash: "hash".to_string(),
            service_account_key: "seed-workloads".to_string(),
            service_account_name: "Seed Workloads".to_string(),
            service_account_team_key: "seed-workloads".to_string(),
            service_account_budget: SeedBudget {
                cadence: BudgetCadence::Daily,
                amount_usd: Money4::from_scaled(100_000),
                hard_limit: true,
                timezone: "UTC".to_string(),
            },
            allowed_models: vec!["fast".to_string()],
        }];

        store
            .seed_from_inputs(
                &providers,
                &models,
                &api_keys,
                &[],
                &[],
                &seed_api_key_teams(),
                &[],
            )
            .await
            .expect("seed");

        let key = store
            .get_api_key_by_public_id("dev123")
            .await
            .expect("get key")
            .expect("api key exists");
        let seed_team_id = store
            .get_team_by_key("seed-workloads")
            .await
            .expect("load seed team")
            .expect("seed team")
            .team_id;
        let seed_service_account_id =
            Uuid::new_v5(&Uuid::NAMESPACE_OID, b"service_account:seed-workloads");
        assert_eq!(key.owner_kind, ApiKeyOwnerKind::ServiceAccount);
        assert_eq!(key.owner_team_id, Some(seed_team_id));
        assert_eq!(key.owner_service_account_id, Some(seed_service_account_id));
        assert!(
            store
                .list_models_for_api_key(key.id)
                .await
                .expect("list models")
                .iter()
                .any(|model| model.model_key == "fast")
        );

        let model_id = store
            .list_models_for_api_key(key.id)
            .await
            .expect("list models")
            .into_iter()
            .find(|model| model.model_key == "fast")
            .expect("fast model")
            .id;
        let routes = store
            .list_routes_for_model(model_id)
            .await
            .expect("list routes");
        assert_eq!(routes.len(), 1);
        assert!(!routes[0].capabilities.vision);

        let user = store
            .upsert_bootstrap_admin_user("Admin", "admin@local", true)
            .await
            .expect("bootstrap admin");
        store
            .store_user_password(user.user_id, "hash-1", OffsetDateTime::now_utc())
            .await
            .expect("password");
        assert!(
            store
                .get_user_password_auth(user.user_id)
                .await
                .expect("password auth")
                .is_some()
        );

        let team = store
            .create_team("platform", "Platform")
            .await
            .expect("create team");
        let member = store
            .create_identity_user(
                "Member",
                "member@example.com",
                "member@example.com",
                GlobalRole::User,
                AuthMode::Password,
                UserStatus::Invited,
            )
            .await
            .expect("create user");
        store
            .assign_team_membership(member.user_id, team.team_id, MembershipRole::Admin)
            .await
            .expect("membership");
        assert_eq!(
            store
                .list_team_memberships(team.team_id)
                .await
                .expect("memberships")
                .len(),
            1
        );

        let invitation = store
            .create_password_invitation(
                Uuid::new_v4(),
                member.user_id,
                "token-hash",
                OffsetDateTime::now_utc() + time::Duration::days(7),
                OffsetDateTime::now_utc(),
            )
            .await
            .expect("invitation");
        assert!(
            store
                .get_password_invitation(invitation.invitation_id)
                .await
                .expect("load invitation")
                .is_some()
        );

        let cache = PricingCatalogCacheRecord {
            catalog_key: "catalog".to_string(),
            source: "test".to_string(),
            etag: Some("etag-1".to_string()),
            fetched_at: OffsetDateTime::now_utc(),
            snapshot_json: "{\"providers\":[]}".to_string(),
        };
        store
            .upsert_pricing_catalog_cache(&cache)
            .await
            .expect("upsert cache");
        assert_eq!(
            store
                .get_pricing_catalog_cache("catalog")
                .await
                .expect("cache lookup")
                .expect("cache row")
                .etag
                .as_deref(),
            Some("etag-1")
        );

        let pricing_time = OffsetDateTime::now_utc();
        let pricing_record = ModelPricingRecord {
            model_pricing_id: Uuid::new_v4(),
            pricing_provider_id: "openai".to_string(),
            pricing_model_id: "gpt-5".to_string(),
            display_name: "GPT-5".to_string(),
            input_cost_per_million_tokens: Some(Money4::from_scaled(1_250)),
            output_cost_per_million_tokens: Some(Money4::from_scaled(10_000)),
            cache_read_cost_per_million_tokens: None,
            cache_write_cost_per_million_tokens: None,
            input_audio_cost_per_million_tokens: None,
            output_audio_cost_per_million_tokens: None,
            release_date: "2025-01-01".to_string(),
            last_updated: "2025-01-01".to_string(),
            effective_start_at: pricing_time,
            effective_end_at: None,
            limits: PricingLimits {
                context: Some(128_000),
                input: None,
                output: None,
            },
            modalities: PricingModalities {
                input: vec!["text".to_string()],
                output: vec!["text".to_string()],
            },
            provenance: PricingProvenance {
                source: "test".to_string(),
                etag: Some("etag-1".to_string()),
                fetched_at: pricing_time,
            },
            created_at: pricing_time,
            updated_at: pricing_time,
        };
        store
            .insert_model_pricing(&pricing_record)
            .await
            .expect("insert model pricing");
        assert!(
            store
                .resolve_model_pricing_at("openai", "gpt-5", pricing_time)
                .await
                .expect("resolve model pricing")
                .is_some()
        );

        let priced_event = UsageLedgerRecord {
            usage_event_id: Uuid::new_v4(),
            request_id: "req-postgres-priced".to_string(),
            ownership_scope_key: format!("user:{}", member.user_id),
            api_key_id: key.id,
            user_id: Some(member.user_id),
            team_id: Some(team.team_id),
            service_account_id: None,
            actor_user_id: None,
            model_id: None,
            provider_key: "openai-prod".to_string(),
            upstream_model: "gpt-5".to_string(),
            prompt_tokens: Some(10),
            completion_tokens: Some(5),
            total_tokens: Some(15),
            provider_usage: json!({"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}),
            pricing_status: UsagePricingStatus::Priced,
            unpriced_reason: None,
            pricing_row_id: Some(pricing_record.model_pricing_id),
            pricing_provider_id: Some("openai".to_string()),
            pricing_model_id: Some("gpt-5".to_string()),
            pricing_source: Some("test".to_string()),
            pricing_source_etag: Some("etag-1".to_string()),
            pricing_source_fetched_at: Some(pricing_time),
            pricing_last_updated: Some("2025-01-01".to_string()),
            input_cost_per_million_tokens: pricing_record.input_cost_per_million_tokens,
            output_cost_per_million_tokens: pricing_record.output_cost_per_million_tokens,
            computed_cost_usd: Money4::from_scaled(25),
            occurred_at: pricing_time,
        };
        let unpriced_event = UsageLedgerRecord {
            usage_event_id: Uuid::new_v4(),
            request_id: "req-postgres-unpriced".to_string(),
            ownership_scope_key: format!("user:{}", member.user_id),
            api_key_id: key.id,
            user_id: Some(member.user_id),
            team_id: Some(team.team_id),
            service_account_id: None,
            actor_user_id: None,
            model_id: None,
            provider_key: "openai-prod".to_string(),
            upstream_model: "gpt-5".to_string(),
            prompt_tokens: Some(10),
            completion_tokens: Some(5),
            total_tokens: Some(15),
            provider_usage: json!({"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}),
            pricing_status: UsagePricingStatus::Unpriced,
            unpriced_reason: Some("missing_pricing".to_string()),
            pricing_row_id: None,
            pricing_provider_id: None,
            pricing_model_id: None,
            pricing_source: None,
            pricing_source_etag: None,
            pricing_source_fetched_at: None,
            pricing_last_updated: None,
            input_cost_per_million_tokens: None,
            output_cost_per_million_tokens: None,
            computed_cost_usd: Money4::ZERO,
            occurred_at: pricing_time,
        };
        assert!(
            store
                .insert_usage_ledger_if_absent(&priced_event)
                .await
                .expect("insert priced ledger")
        );
        assert!(
            !store
                .insert_usage_ledger_if_absent(&priced_event)
                .await
                .expect("insert duplicate ledger")
        );
        assert!(
            store
                .insert_usage_ledger_if_absent(&unpriced_event)
                .await
                .expect("insert unpriced ledger")
        );
        assert!(
            store
                .get_usage_ledger_by_request_and_scope(
                    &priced_event.request_id,
                    &priced_event.ownership_scope_key,
                )
                .await
                .expect("load usage ledger")
                .is_some()
        );
        assert_eq!(
            store
                .sum_usage_cost_for_budget_scope_in_window(
                    &BudgetScope::User {
                        user_id: member.user_id,
                    },
                    pricing_time - time::Duration::minutes(1),
                    pricing_time + time::Duration::minutes(1),
                )
                .await
                .expect("sum usage cost"),
            Money4::from_scaled(25)
        );

        let log = RequestLogRecord {
            request_log_id: Uuid::new_v4(),
            request_id: "req-1".to_string(),
            api_key_id: key.id,
            user_id: Some(member.user_id),
            team_id: Some(team.team_id),
            service_account_id: None,
            model_key: "fast".to_string(),
            resolved_model_key: "fast-v2".to_string(),
            provider_key: "openai-prod".to_string(),
            status_code: Some(200),
            latency_ms: Some(42),
            prompt_tokens: Some(10),
            completion_tokens: Some(20),
            total_tokens: Some(30),
            error_code: None,
            has_payload: false,
            request_payload_truncated: false,
            response_payload_truncated: false,
            request_tags: RequestTags::default(),
            tool_cardinality: gateway_core::RequestToolCardinality::default(),
            user_agent_raw: None,
            agent_harness_key: "unknown".to_string(),
            agent_harness_label: "Unknown".to_string(),
            metadata: Map::new(),
            occurred_at: OffsetDateTime::now_utc(),
        };
        store
            .insert_request_log(&log, None)
            .await
            .expect("insert request log");

        let row = sqlx::query(
            "SELECT COUNT(*), MIN(model_key), MIN(resolved_model_key) FROM request_logs",
        )
        .fetch_one(store.pool())
        .await
        .expect("request log count");
        let count: i64 = row.try_get(0).expect("count");
        assert_eq!(count, 1);
        let model_key: String = row.try_get(1).expect("model key");
        let resolved_model_key: String = row.try_get(2).expect("resolved model key");
        assert_eq!(model_key, "fast");
        assert_eq!(resolved_model_key, "fast-v2");

        drop(store);
        drop_postgres_test_database(&test_db).await;
    }

    #[tokio::test]
    #[serial]
    async fn postgres_identity_mutation_store_helpers_cover_transfer_removal_and_revocation() {
        let Some(test_db) = create_postgres_test_database().await else {
            eprintln!("skipping postgres store test because TEST_POSTGRES_URL is not set");
            return;
        };

        let options = StoreConnectionOptions::Postgres {
            url: test_db.database_url.clone(),
            max_connections: 4,
        };
        run_migrations_with_options(&options)
            .await
            .expect("postgres migrations");

        let store = PostgresStore::connect(&test_db.database_url, 4)
            .await
            .expect("postgres store");
        assert_identity_mutation_store_helpers(&store).await;

        drop(store);
        drop_postgres_test_database(&test_db).await;
    }

    #[tokio::test]
    #[serial]
    async fn postgres_service_account_budget_enforces_single_active_record_per_scope_key() {
        let Some(test_db) = create_postgres_test_database().await else {
            eprintln!(
                "skipping postgres service account budget uniqueness test because TEST_POSTGRES_URL is not set"
            );
            return;
        };

        let options = StoreConnectionOptions::Postgres {
            url: test_db.database_url.clone(),
            max_connections: 2,
        };
        run_migrations_with_options(&options)
            .await
            .expect("postgres migrations");

        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&test_db.database_url)
            .await
            .expect("postgres pool");
        let now = OffsetDateTime::now_utc().unix_timestamp();
        let team_id = Uuid::new_v4();
        let service_account_id = Uuid::new_v4();

        sqlx::query(
            r#"
            INSERT INTO teams (
              team_id, team_key, team_name, status, model_access_mode, created_at, updated_at
            ) VALUES ($1, 'platform', 'Platform', 'active', 'all', $2, $2)
            "#,
        )
        .bind(team_id.to_string())
        .bind(now)
        .execute(&pool)
        .await
        .expect("team");

        sqlx::query(
            r#"
            INSERT INTO service_accounts (
              service_account_id, team_id, service_account_key, service_account_name,
              status, model_access_mode, metadata_json, created_at, updated_at
            ) VALUES ($1, $2, 'svc', 'Service', 'active', 'all', '{}', $3, $3)
            "#,
        )
        .bind(service_account_id.to_string())
        .bind(team_id.to_string())
        .bind(now)
        .execute(&pool)
        .await
        .expect("service account");

        sqlx::query(
            r#"
            INSERT INTO budgets (
                budget_id, scope_kind, scope_key, service_account_id, cadence, amount_10000,
                hard_limit, timezone, is_active, created_at, updated_at
            ) VALUES ($1, 'service_account', $2, $3, 'daily', 100000, 1, 'UTC', 1, $4, $4)
            "#,
        )
        .bind(Uuid::new_v4().to_string())
        .bind(format!("budget:v1:service_account:{service_account_id}"))
        .bind(service_account_id.to_string())
        .bind(now)
        .execute(&pool)
        .await
        .expect("first budget");

        let duplicate_active_result = sqlx::query(
            r#"
            INSERT INTO budgets (
                budget_id, scope_kind, scope_key, service_account_id, cadence, amount_10000,
                hard_limit, timezone, is_active, created_at, updated_at
            ) VALUES ($1, 'service_account', $2, $3, 'weekly', 200000, 1, 'UTC', 1, $4, $4)
            "#,
        )
        .bind(Uuid::new_v4().to_string())
        .bind(format!("budget:v1:service_account:{service_account_id}"))
        .bind(service_account_id.to_string())
        .bind(now)
        .execute(&pool)
        .await;
        assert!(duplicate_active_result.is_err());

        sqlx::query(
            r#"
            INSERT INTO budgets (
                budget_id, scope_kind, scope_key, service_account_id, cadence, amount_10000,
                hard_limit, timezone, is_active, created_at, updated_at
            ) VALUES ($1, 'service_account', $2, $3, 'weekly', 200000, 1, 'UTC', 0, $4, $4)
            "#,
        )
        .bind(Uuid::new_v4().to_string())
        .bind(format!("budget:v1:service_account:{service_account_id}"))
        .bind(service_account_id.to_string())
        .bind(now)
        .execute(&pool)
        .await
        .expect("inactive budget should be allowed");

        pool.close().await;
        drop_postgres_test_database(&test_db).await;
    }

    #[tokio::test]
    #[serial]
    async fn postgres_budget_alert_repository_tracks_history_and_delivery_lifecycle() {
        let Some(test_db) = create_postgres_test_database().await else {
            eprintln!(
                "skipping postgres budget alert repository test because TEST_POSTGRES_URL is not set"
            );
            return;
        };

        let options = StoreConnectionOptions::Postgres {
            url: test_db.database_url.clone(),
            max_connections: 4,
        };
        run_migrations_with_options(&options)
            .await
            .expect("postgres migrations");

        let store = PostgresStore::connect(&test_db.database_url, 4)
            .await
            .expect("postgres store");

        exercise_budget_alert_repository(&store).await;

        drop(store);
        drop_postgres_test_database(&test_db).await;
    }

    #[tokio::test]
    #[serial]
    async fn postgres_spend_reporting_aggregates_and_service_account_window_sum_filter_chargeable_statuses()
     {
        let Some(test_db) = create_postgres_test_database().await else {
            eprintln!(
                "skipping postgres spend reporting parity test because TEST_POSTGRES_URL is not set"
            );
            return;
        };

        let options = StoreConnectionOptions::Postgres {
            url: test_db.database_url.clone(),
            max_connections: 4,
        };
        run_migrations_with_options(&options)
            .await
            .expect("postgres migrations");

        let store = PostgresStore::connect(&test_db.database_url, 4)
            .await
            .expect("postgres store");

        let providers = vec![SeedProvider {
            provider_key: "openai-prod".to_string(),
            provider_type: "openai_compat".to_string(),
            config: json!({
                "base_url": "https://api.openai.com/v1",
                "timeout_ms": 120_000
            }),
            secrets: Some(json!({"token": "env.OPENAI_API_KEY"})),
        }];
        let models = vec![SeedModel {
            model_key: "fast".to_string(),
            alias_target_model_key: None,
            description: Some("fast tier".to_string()),
            tags: vec!["fast".to_string()],
            rank: 10,
            routes: vec![SeedModelRoute {
                provider_key: "openai-prod".to_string(),
                upstream_model: "gpt-4o-mini".to_string(),
                priority: 10,
                weight: 1.0,
                enabled: true,
                extra_headers: Map::new(),
                extra_body: Map::new(),
                capabilities: ProviderCapabilities::all_enabled(),
                compatibility: Default::default(),
            }],
        }];
        let api_keys = vec![SeedApiKey {
            name: "dev".to_string(),
            public_id: "dev123".to_string(),
            secret_hash: "hash".to_string(),
            service_account_key: "seed-workloads".to_string(),
            service_account_name: "Seed Workloads".to_string(),
            service_account_team_key: "seed-workloads".to_string(),
            service_account_budget: SeedBudget {
                cadence: BudgetCadence::Daily,
                amount_usd: Money4::from_scaled(100_000),
                hard_limit: true,
                timezone: "UTC".to_string(),
            },
            allowed_models: vec!["fast".to_string()],
        }];
        store
            .seed_from_inputs(
                &providers,
                &models,
                &api_keys,
                &[],
                &[],
                &seed_api_key_teams(),
                &[],
            )
            .await
            .expect("seed");

        let api_key = store
            .get_api_key_by_public_id("dev123")
            .await
            .expect("load api key")
            .expect("api key");
        let model = store
            .get_model_by_key("fast")
            .await
            .expect("load model")
            .expect("model");
        let service_account_id = api_key
            .owner_service_account_id
            .expect("seed api key has service account owner");
        let team_id = api_key
            .owner_team_id
            .expect("seed service account has owning team");
        let user = store
            .create_identity_user(
                "Member",
                "member@example.com",
                "member@example.com",
                GlobalRole::User,
                AuthMode::Password,
                UserStatus::Active,
            )
            .await
            .expect("create user");
        let (user_tags, team_tags) = focus_export_owner_tags();
        store
            .update_user_tags(user.user_id, &user_tags, OffsetDateTime::now_utc())
            .await
            .expect("update user tags");
        store
            .update_team_tags(team_id, &team_tags, OffsetDateTime::now_utc())
            .await
            .expect("update team tags");

        let now = OffsetDateTime::from_unix_timestamp(1_773_484_800).expect("timestamp");
        let budget = store
            .upsert_active_budget(
                &BudgetScope::ServiceAccount { service_account_id },
                &BudgetSettings {
                    cadence: BudgetCadence::Daily,
                    amount_usd: Money4::from_scaled(100_000),
                    hard_limit: true,
                    timezone: "UTC".to_string(),
                },
                now,
            )
            .await
            .expect("upsert service account budget");
        assert_eq!(budget.settings.amount_usd, Money4::from_scaled(100_000));

        let day_one = OffsetDateTime::from_unix_timestamp(1_773_486_600).expect("day one");
        let day_two = day_one + Duration::days(1);

        for event in [
            build_usage_ledger_record(
                "req-user-priced",
                format!("user:{}", user.user_id),
                api_key.id,
                Some(user.user_id),
                None,
                None,
                Some(model.id),
                "gpt-4o-mini",
                UsagePricingStatus::Priced,
                11_000,
                day_one,
            ),
            build_usage_ledger_record(
                "req-user-unpriced",
                format!("user:{}", user.user_id),
                api_key.id,
                Some(user.user_id),
                None,
                None,
                Some(model.id),
                "gpt-4o-mini",
                UsagePricingStatus::Unpriced,
                0,
                day_one,
            ),
            build_usage_ledger_record(
                "req-service-account-legacy",
                format!("service_account:{service_account_id}"),
                api_key.id,
                None,
                Some(team_id),
                Some(service_account_id),
                None,
                "claude-3-5-sonnet",
                UsagePricingStatus::LegacyEstimated,
                22_000,
                day_two,
            ),
            build_usage_ledger_record(
                "req-service-account-unpriced",
                format!("service_account:{service_account_id}"),
                api_key.id,
                None,
                Some(team_id),
                Some(service_account_id),
                None,
                "claude-3-5-sonnet",
                UsagePricingStatus::Unpriced,
                0,
                day_two,
            ),
            build_usage_ledger_record(
                "req-service-account-usage-missing",
                format!("service_account:{service_account_id}"),
                api_key.id,
                None,
                Some(team_id),
                Some(service_account_id),
                None,
                "claude-3-5-sonnet",
                UsagePricingStatus::UsageMissing,
                0,
                day_two,
            ),
        ] {
            assert!(
                store
                    .insert_usage_ledger_if_absent(&event)
                    .await
                    .expect("insert usage ledger")
            );
        }

        let window_start = day_one - Duration::hours(1);
        let window_end = day_two + Duration::days(1);
        let service_account_sum = store
            .sum_usage_cost_for_budget_scope_in_window(
                &BudgetScope::ServiceAccount { service_account_id },
                window_start,
                window_end,
            )
            .await
            .expect("service account sum");
        assert_eq!(service_account_sum, Money4::from_scaled(22_000));

        let daily = store
            .list_usage_daily_aggregates(window_start, window_end, None)
            .await
            .expect("daily aggregates");
        assert_eq!(daily.len(), 2);
        let day_one_bucket = (day_one.unix_timestamp() / 86_400) * 86_400;
        let day_two_bucket = (day_two.unix_timestamp() / 86_400) * 86_400;
        let first = daily
            .iter()
            .find(|row| row.day_start.unix_timestamp() == day_one_bucket)
            .expect("day one aggregate");
        assert_eq!(first.priced_cost_usd, Money4::from_scaled(11_000));
        assert_eq!(first.priced_request_count, 1);
        assert_eq!(first.unpriced_request_count, 1);
        assert_eq!(first.usage_missing_request_count, 0);
        let second = daily
            .iter()
            .find(|row| row.day_start.unix_timestamp() == day_two_bucket)
            .expect("day two aggregate");
        assert_eq!(second.priced_cost_usd, Money4::from_scaled(22_000));
        assert_eq!(second.priced_request_count, 1);
        assert_eq!(second.unpriced_request_count, 1);
        assert_eq!(second.usage_missing_request_count, 1);

        let owners = store
            .list_usage_owner_aggregates(window_start, window_end, None)
            .await
            .expect("owner aggregates");
        assert_eq!(owners.len(), 2);
        let user_owner = owners
            .iter()
            .find(|row| row.owner_kind == ApiKeyOwnerKind::User)
            .expect("user owner aggregate");
        assert_eq!(user_owner.owner_id, user.user_id);
        assert_eq!(user_owner.priced_cost_usd, Money4::from_scaled(11_000));
        assert_eq!(user_owner.priced_request_count, 1);
        assert_eq!(user_owner.unpriced_request_count, 1);
        assert_eq!(user_owner.usage_missing_request_count, 0);
        let service_account_owner = owners
            .iter()
            .find(|row| row.owner_kind == ApiKeyOwnerKind::ServiceAccount)
            .expect("service account owner aggregate");
        assert_eq!(service_account_owner.owner_id, service_account_id);
        assert_eq!(
            service_account_owner.priced_cost_usd,
            Money4::from_scaled(22_000)
        );
        assert_eq!(service_account_owner.priced_request_count, 1);
        assert_eq!(service_account_owner.unpriced_request_count, 1);
        assert_eq!(service_account_owner.usage_missing_request_count, 1);

        let models = store
            .list_usage_model_aggregates(window_start, window_end, None)
            .await
            .expect("model aggregates");
        assert_eq!(models.len(), 2);
        let gateway_model = models
            .iter()
            .find(|row| row.model_key == "fast")
            .expect("gateway model aggregate");
        assert_eq!(gateway_model.priced_cost_usd, Money4::from_scaled(11_000));
        assert_eq!(gateway_model.priced_request_count, 1);
        assert_eq!(gateway_model.unpriced_request_count, 1);
        assert_eq!(gateway_model.usage_missing_request_count, 0);
        let upstream_model = models
            .iter()
            .find(|row| row.model_key == "claude-3-5-sonnet")
            .expect("upstream model aggregate");
        assert_eq!(upstream_model.priced_cost_usd, Money4::from_scaled(22_000));
        assert_eq!(upstream_model.priced_request_count, 1);
        assert_eq!(upstream_model.unpriced_request_count, 1);
        assert_eq!(upstream_model.usage_missing_request_count, 1);

        assert_focus_export_aggregates(
            &store,
            window_start,
            window_end,
            user.user_id,
            service_account_id,
        )
        .await;

        drop(store);
        drop_postgres_test_database(&test_db).await;
    }

    #[tokio::test]
    #[serial]
    async fn postgres_usage_leaderboard_ranks_users_and_aggregates_half_day_buckets() {
        let Some(test_db) = create_postgres_test_database().await else {
            eprintln!(
                "skipping postgres leaderboard parity test because TEST_POSTGRES_URL is not set"
            );
            return;
        };

        let options = StoreConnectionOptions::Postgres {
            url: test_db.database_url.clone(),
            max_connections: 4,
        };
        run_migrations_with_options(&options)
            .await
            .expect("postgres migrations");

        let store = PostgresStore::connect(&test_db.database_url, 4)
            .await
            .expect("postgres store");

        exercise_usage_leaderboard_reporting(&store).await;

        drop(store);
        drop_postgres_test_database(&test_db).await;
    }

    #[tokio::test]
    #[serial]
    async fn postgres_alias_backed_models_round_trip_through_store() {
        let Some(test_db) = create_postgres_test_database().await else {
            eprintln!("skipping postgres alias store test because TEST_POSTGRES_URL is not set");
            return;
        };

        let options = StoreConnectionOptions::Postgres {
            url: test_db.database_url.clone(),
            max_connections: 4,
        };
        run_migrations_with_options(&options)
            .await
            .expect("postgres migrations");

        let store = PostgresStore::connect(&test_db.database_url, 4)
            .await
            .expect("postgres store");

        let providers = vec![SeedProvider {
            provider_key: "openai-prod".to_string(),
            provider_type: "openai_compat".to_string(),
            config: json!({
                "base_url": "https://api.openai.com/v1",
                "timeout_ms": 120_000
            }),
            secrets: Some(json!({"token": "env.OPENAI_API_KEY"})),
        }];
        let models = vec![
            SeedModel {
                model_key: "fast".to_string(),
                alias_target_model_key: Some("fast-v2".to_string()),
                description: Some("alias".to_string()),
                tags: vec!["fast".to_string()],
                rank: 10,
                routes: Vec::new(),
            },
            SeedModel {
                model_key: "fast-v2".to_string(),
                alias_target_model_key: None,
                description: Some("replacement".to_string()),
                tags: vec!["fast".to_string()],
                rank: 5,
                routes: vec![SeedModelRoute {
                    provider_key: "openai-prod".to_string(),
                    upstream_model: "gpt-5".to_string(),
                    priority: 10,
                    weight: 1.0,
                    enabled: true,
                    extra_headers: Map::new(),
                    extra_body: Map::new(),
                    capabilities: ProviderCapabilities::all_enabled(),
                    compatibility: Default::default(),
                }],
            },
        ];
        let api_keys = vec![SeedApiKey {
            name: "dev".to_string(),
            public_id: "dev123".to_string(),
            secret_hash: "hash".to_string(),
            service_account_key: "seed-workloads".to_string(),
            service_account_name: "Seed Workloads".to_string(),
            service_account_team_key: "seed-workloads".to_string(),
            service_account_budget: SeedBudget {
                cadence: BudgetCadence::Daily,
                amount_usd: Money4::from_scaled(100_000),
                hard_limit: true,
                timezone: "UTC".to_string(),
            },
            allowed_models: vec!["fast".to_string()],
        }];

        store
            .seed_from_inputs(
                &providers,
                &models,
                &api_keys,
                &[],
                &[],
                &seed_api_key_teams(),
                &[],
            )
            .await
            .expect("seed");

        let alias_model = store
            .get_model_by_key("fast")
            .await
            .expect("query alias")
            .expect("alias model exists");
        assert_eq!(
            alias_model.alias_target_model_key.as_deref(),
            Some("fast-v2")
        );

        let api_key = store
            .get_api_key_by_public_id("dev123")
            .await
            .expect("query key")
            .expect("api key exists");
        let accessible_models = store
            .list_models_for_api_key(api_key.id)
            .await
            .expect("models by key");
        assert_eq!(accessible_models.len(), 1);
        assert_eq!(accessible_models[0].model_key, "fast");
        assert_eq!(
            accessible_models[0].alias_target_model_key.as_deref(),
            Some("fast-v2")
        );

        let target_model = store
            .get_model_by_key("fast-v2")
            .await
            .expect("query target")
            .expect("target model exists");
        let routes = store
            .list_routes_for_model(target_model.id)
            .await
            .expect("target routes");
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].upstream_model, "gpt-5");

        drop(store);
        drop_postgres_test_database(&test_db).await;
    }

    #[tokio::test]
    #[serial]
    async fn postgres_request_log_detail_missing_returns_not_found() {
        let Some(test_db) = create_postgres_test_database().await else {
            eprintln!(
                "skipping postgres request log detail missing test because TEST_POSTGRES_URL is not set"
            );
            return;
        };

        run_migrations_with_options(&StoreConnectionOptions::Postgres {
            url: test_db.database_url.clone(),
            max_connections: 2,
        })
        .await
        .expect("apply postgres migrations");

        let store = PostgresStore::connect(&test_db.database_url, 2)
            .await
            .expect("postgres store");

        let error = store
            .get_request_log_detail(Uuid::new_v4())
            .await
            .expect_err("missing request log should fail");
        assert!(matches!(error, StoreError::NotFound(_)));

        drop(store);
        drop_postgres_test_database(&test_db).await;
    }

    #[tokio::test]
    #[serial]
    async fn postgres_mcp_tool_invocations_round_trip_and_filter() {
        let Some(test_db) = create_postgres_test_database().await else {
            eprintln!("skipping postgres MCP invocation test because TEST_POSTGRES_URL is not set");
            return;
        };

        let options = StoreConnectionOptions::Postgres {
            url: test_db.database_url.clone(),
            max_connections: 4,
        };
        run_migrations_with_options(&options)
            .await
            .expect("postgres migrations");

        let store = PostgresStore::connect(&test_db.database_url, 4)
            .await
            .expect("postgres store");

        exercise_mcp_tool_invocation_repository(&store).await;

        drop(store);
        drop_postgres_test_database(&test_db).await;
    }

    #[tokio::test]
    #[serial]
    async fn postgres_external_mcp_registry_round_trip_and_rediscovery() {
        let Some(test_db) = create_postgres_test_database().await else {
            eprintln!("skipping postgres MCP registry test because TEST_POSTGRES_URL is not set");
            return;
        };

        let options = StoreConnectionOptions::Postgres {
            url: test_db.database_url.clone(),
            max_connections: 4,
        };
        run_migrations_with_options(&options)
            .await
            .expect("postgres migrations");

        let store = PostgresStore::connect(&test_db.database_url, 4)
            .await
            .expect("postgres store");

        exercise_mcp_registry_repository(&store).await;

        drop(store);
        drop_postgres_test_database(&test_db).await;
    }

    #[tokio::test]
    #[serial]
    async fn postgres_mcp_upstream_credentials_round_trip_and_revoke() {
        let Some(test_db) = create_postgres_test_database().await else {
            eprintln!("skipping postgres MCP credential test because TEST_POSTGRES_URL is not set");
            return;
        };

        let options = StoreConnectionOptions::Postgres {
            url: test_db.database_url.clone(),
            max_connections: 4,
        };
        run_migrations_with_options(&options)
            .await
            .expect("postgres migrations");

        let store = PostgresStore::connect(&test_db.database_url, 4)
            .await
            .expect("postgres store");

        exercise_mcp_upstream_credential_repository(&store).await;

        drop(store);
        drop_postgres_test_database(&test_db).await;
    }

    #[tokio::test]
    #[serial]
    async fn postgres_migration_status_reports_pending_and_applied_versions() {
        let Some(test_db) = create_postgres_test_database().await else {
            eprintln!(
                "skipping postgres migration status test because TEST_POSTGRES_URL is not set"
            );
            return;
        };

        let options = StoreConnectionOptions::Postgres {
            url: test_db.database_url.clone(),
            max_connections: 2,
        };

        let initial_status = status_migrations_with_options(&options)
            .await
            .expect("initial postgres status");
        assert_eq!(initial_status.backend, "postgres");
        assert_eq!(initial_status.pending_count(), MIGRATION_REGISTRY.len());
        assert!(initial_status.entries.iter().all(|entry| !entry.applied));

        run_migrations_with_options(&options)
            .await
            .expect("postgres migrations");

        let applied_status = status_migrations_with_options(&options)
            .await
            .expect("applied postgres status");
        assert_eq!(applied_status.backend, "postgres");
        assert_eq!(applied_status.pending_count(), 0);
        assert!(applied_status.entries.iter().all(|entry| entry.applied));

        drop_postgres_test_database(&test_db).await;
    }

    #[tokio::test]
    #[serial]
    async fn postgres_migration_commands_reject_legacy_history() {
        let Some(test_db) = create_postgres_test_database().await else {
            eprintln!(
                "skipping postgres legacy migration history test because TEST_POSTGRES_URL is not set"
            );
            return;
        };

        let options = StoreConnectionOptions::Postgres {
            url: test_db.database_url.clone(),
            max_connections: 2,
        };

        insert_postgres_history_entry(&test_db.database_url, 1, "init", "V1__init.sql")
            .await
            .expect("legacy history row");

        assert_database_reset_required(
            status_migrations_with_options(&options)
                .await
                .expect_err("status should reject legacy history"),
        );
        assert_database_reset_required(
            check_migrations_with_options(&options)
                .await
                .expect_err("check should reject legacy history"),
        );
        assert_database_reset_required(
            run_migrations_with_options(&options)
                .await
                .expect_err("apply should reject legacy history"),
        );

        drop_postgres_test_database(&test_db).await;
    }

    #[tokio::test]
    #[serial]
    async fn postgres_migration_commands_reject_empty_history_when_app_tables_exist() {
        let Some(test_db) = create_postgres_test_database().await else {
            eprintln!(
                "skipping postgres empty-history migration test because TEST_POSTGRES_URL is not set"
            );
            return;
        };

        let options = StoreConnectionOptions::Postgres {
            url: test_db.database_url.clone(),
            max_connections: 2,
        };

        insert_postgres_application_table_without_history(&test_db.database_url)
            .await
            .expect("application table");

        assert_database_reset_required(
            status_migrations_with_options(&options)
                .await
                .expect_err("status should reject empty history with app tables"),
        );
        assert_database_reset_required(
            check_migrations_with_options(&options)
                .await
                .expect_err("check should reject empty history with app tables"),
        );
        assert_database_reset_required(
            run_migrations_with_options(&options)
                .await
                .expect_err("apply should reject empty history with app tables"),
        );

        drop_postgres_test_database(&test_db).await;
    }

    #[tokio::test]
    #[serial]
    async fn postgres_migrations_rollback_when_history_write_fails() {
        let Some(test_db) = create_postgres_test_database().await else {
            eprintln!(
                "skipping postgres migration rollback test because TEST_POSTGRES_URL is not set"
            );
            return;
        };

        let options = StoreConnectionOptions::Postgres {
            url: test_db.database_url.clone(),
            max_connections: 2,
        };
        let baseline_version = MIGRATION_REGISTRY[0].version;
        run_migrations_with_options_for_test(
            &options,
            MigrationTestHook {
                fail_after_apply_version: Some(baseline_version),
                ..MigrationTestHook::default()
            },
        )
        .await
        .expect_err("postgres migration should fail");

        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&test_db.database_url)
            .await
            .expect("postgres pool");
        let history_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM refinery_schema_history")
            .fetch_one(&pool)
            .await
            .expect("history count");
        assert_eq!(history_count, 0);

        let providers_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM information_schema.tables WHERE table_name = 'providers')",
        )
        .fetch_one(&pool)
        .await
        .expect("providers table exists");
        assert!(!providers_exists);

        pool.close().await;
        drop_postgres_test_database(&test_db).await;
    }

    #[tokio::test]
    #[serial]
    async fn postgres_migrations_rollback_when_schema_history_insert_fails() {
        let Some(test_db) = create_postgres_test_database().await else {
            eprintln!(
                "skipping postgres migration rollback test because TEST_POSTGRES_URL is not set"
            );
            return;
        };

        let options = StoreConnectionOptions::Postgres {
            url: test_db.database_url.clone(),
            max_connections: 2,
        };
        let baseline_version = MIGRATION_REGISTRY[0].version;
        run_migrations_with_options_for_test(
            &options,
            MigrationTestHook {
                fail_history_insert_version: Some(baseline_version),
                ..MigrationTestHook::default()
            },
        )
        .await
        .expect_err("postgres migration should fail when history insert fails");

        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&test_db.database_url)
            .await
            .expect("postgres pool");
        let history_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM refinery_schema_history")
            .fetch_one(&pool)
            .await
            .expect("history count");
        assert_eq!(history_count, 0);

        let providers_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM information_schema.tables WHERE table_name = 'providers')",
        )
        .fetch_one(&pool)
        .await
        .expect("providers table exists");
        assert!(!providers_exists);

        pool.close().await;
        drop_postgres_test_database(&test_db).await;
    }

    #[tokio::test]
    #[serial]
    async fn postgres_migration_status_recovers_after_failure_and_retry() {
        let Some(test_db) = create_postgres_test_database().await else {
            eprintln!(
                "skipping postgres migration status retry test because TEST_POSTGRES_URL is not set"
            );
            return;
        };

        let options = StoreConnectionOptions::Postgres {
            url: test_db.database_url.clone(),
            max_connections: 2,
        };
        let baseline_version = MIGRATION_REGISTRY[0].version;

        let initial_status = status_migrations_with_options(&options)
            .await
            .expect("initial postgres status");
        assert_eq!(initial_status.pending_count(), MIGRATION_REGISTRY.len());
        assert!(initial_status.entries.iter().all(|entry| !entry.applied));

        run_migrations_with_options_for_test(
            &options,
            MigrationTestHook {
                fail_history_insert_version: Some(baseline_version),
                ..MigrationTestHook::default()
            },
        )
        .await
        .expect_err("postgres migration should fail when history insert fails");

        let failed_status = status_migrations_with_options(&options)
            .await
            .expect("status after failed migration");
        assert_eq!(failed_status.pending_count(), MIGRATION_REGISTRY.len());
        assert!(failed_status.entries.iter().all(|entry| !entry.applied));

        run_migrations_with_options(&options)
            .await
            .expect("postgres retry migrations");

        let applied_status = status_migrations_with_options(&options)
            .await
            .expect("status after retry");
        assert_eq!(applied_status.pending_count(), 0);
        assert!(applied_status.entries.iter().all(|entry| entry.applied));

        drop_postgres_test_database(&test_db).await;
    }

    async fn insert_libsql_history_entry(
        db_path: &std::path::Path,
        version: i64,
        name: &str,
        checksum: &str,
    ) -> anyhow::Result<()> {
        let db = libsql::Builder::new_local(db_path)
            .build()
            .await
            .expect("libsql db");
        let conn = db.connect().expect("libsql connection");
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
        .expect("schema history");
        conn.execute(
            r#"
            INSERT INTO refinery_schema_history (version, name, applied_on, checksum)
            VALUES (?1, ?2, unixepoch(), ?3)
            "#,
            libsql::params![version, name, checksum],
        )
        .await
        .expect("history row");

        Ok(())
    }

    async fn insert_postgres_history_entry(
        database_url: &str,
        version: i64,
        name: &str,
        checksum: &str,
    ) -> anyhow::Result<()> {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(database_url)
            .await
            .expect("postgres pool");
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
        .expect("schema history");
        sqlx::query(
            r#"
            INSERT INTO refinery_schema_history (version, name, applied_on, checksum)
            VALUES ($1, $2, extract(epoch from now())::bigint, $3)
            "#,
        )
        .bind(version)
        .bind(name)
        .bind(checksum)
        .execute(&pool)
        .await
        .expect("history row");

        pool.close().await;
        Ok(())
    }

    async fn insert_libsql_application_table_without_history(
        db_path: &std::path::Path,
    ) -> anyhow::Result<()> {
        let db = libsql::Builder::new_local(db_path)
            .build()
            .await
            .expect("libsql db");
        let conn = db.connect().expect("libsql connection");
        conn.execute(
            r#"
            CREATE TABLE providers (
                provider_key TEXT PRIMARY KEY
            )
            "#,
            (),
        )
        .await
        .expect("providers table");
        Ok(())
    }

    async fn insert_postgres_application_table_without_history(
        database_url: &str,
    ) -> anyhow::Result<()> {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(database_url)
            .await
            .expect("postgres pool");
        sqlx::query(
            r#"
            CREATE TABLE providers (
                provider_key TEXT PRIMARY KEY
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("providers table");
        pool.close().await;
        Ok(())
    }

    fn assert_database_reset_required(error: anyhow::Error) {
        let message = error.to_string();
        assert!(
            message.contains("database reset required"),
            "expected reset-required error, got: {message}"
        );
        assert!(
            message.contains("recreate the database"),
            "expected recreation guidance, got: {message}"
        );
    }

    struct PostgresTestDatabase {
        admin_url: String,
        database_url: String,
        database_name: String,
    }

    async fn create_postgres_test_database() -> Option<PostgresTestDatabase> {
        let base_url = env::var("TEST_POSTGRES_URL").ok()?;
        let mut admin_url = Url::parse(&base_url).expect("valid postgres url");
        admin_url.set_path("/postgres");

        let admin_pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(admin_url.as_str())
            .await
            .expect("admin postgres pool");

        let database_name = format!("gateway_store_test_{}", Uuid::new_v4().simple());
        sqlx::query(&format!("CREATE DATABASE {database_name}"))
            .execute(&admin_pool)
            .await
            .expect("create test database");
        admin_pool.close().await;

        let mut database_url = Url::parse(&base_url).expect("valid postgres url");
        database_url.set_path(&format!("/{database_name}"));

        Some(PostgresTestDatabase {
            admin_url: admin_url.to_string(),
            database_url: database_url.to_string(),
            database_name,
        })
    }

    async fn drop_postgres_test_database(database: &PostgresTestDatabase) {
        let admin_pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&database.admin_url)
            .await
            .expect("admin postgres pool");

        sqlx::query(
            r#"
            SELECT pg_terminate_backend(pid)
            FROM pg_stat_activity
            WHERE datname = $1
              AND pid <> pg_backend_pid()
            "#,
        )
        .bind(database.database_name.as_str())
        .execute(&admin_pool)
        .await
        .expect("terminate sessions");

        sqlx::query(&format!(
            "DROP DATABASE IF EXISTS {}",
            database.database_name
        ))
        .execute(&admin_pool)
        .await
        .expect("drop test database");
        admin_pool.close().await;
    }
}
