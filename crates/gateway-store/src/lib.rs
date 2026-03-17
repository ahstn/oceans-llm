mod libsql_store;
mod migrate;
mod migration_registry;
mod postgres_store;
mod seed;
mod shared;
mod store;

pub use libsql_store::LibsqlStore;
pub use migrate::{
    MigrationStatus, MigrationStatusEntry, MigrationTestHook, check_migrations_with_options,
    run_migrations, run_migrations_with_options, status_migrations_with_options,
};
pub use postgres_store::PostgresStore;
pub use store::{AnyStore, GatewayStore, StoreConnectionOptions};

#[cfg(test)]
mod tests {
    use std::env;

    use gateway_core::{
        ApiKeyOwnerKind, ApiKeyRepository, AuthMode, BudgetCadence, BudgetRepository, GlobalRole,
        IdentityRepository, MembershipRole, ModelPricingRecord, ModelRepository, Money4,
        PricingCatalogCacheRecord, PricingCatalogRepository, PricingLimits, PricingModalities,
        PricingProvenance, ProviderCapabilities, RequestLogRecord, RequestLogRepository,
        SYSTEM_LEGACY_TEAM_ID, SYSTEM_LEGACY_TEAM_KEY, SeedApiKey, SeedModel, SeedModelRoute,
        SeedProvider, StoreHealth, UsageLedgerRecord, UsagePricingStatus,
    };
    use serde_json::{Map, json};
    use serial_test::serial;
    use sqlx::Row;
    use tempfile::tempdir;
    use time::{Duration, OffsetDateTime};
    use url::Url;
    use uuid::Uuid;

    use crate::{
        LibsqlStore, MigrationTestHook, PostgresStore, StoreConnectionOptions,
        check_migrations_with_options,
        migration_registry::{BackendMigrationStep, MIGRATION_REGISTRY, MigrationBackend},
        run_migrations, run_migrations_with_options, status_migrations_with_options,
    };

    #[allow(clippy::too_many_arguments)]
    fn build_usage_ledger_record(
        request_id: &str,
        ownership_scope_key: String,
        api_key_id: Uuid,
        user_id: Option<Uuid>,
        team_id: Option<Uuid>,
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
    async fn migrations_rollback_when_history_write_fails() {
        let tmp = tempdir().expect("tempdir");
        let db_path = tmp.path().join("gateway.db");

        run_migrations_with_options(
            &StoreConnectionOptions::Libsql {
                path: db_path.clone(),
            },
            MigrationTestHook {
                fail_after_apply_version: Some(1),
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

        run_migrations_with_options(
            &StoreConnectionOptions::Libsql {
                path: db_path.clone(),
            },
            MigrationTestHook {
                fail_history_insert_version: Some(1),
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

        let initial_status = status_migrations_with_options(&options)
            .await
            .expect("initial migration status");
        assert_eq!(initial_status.backend, "libsql");
        assert_eq!(initial_status.pending_count(), MIGRATION_REGISTRY.len());
        assert!(initial_status.entries.iter().all(|entry| !entry.applied));

        run_migrations_with_options(
            &options,
            MigrationTestHook {
                fail_history_insert_version: Some(1),
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

        run_migrations_with_options(&options, MigrationTestHook::default())
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
            }],
        }];

        let api_keys = vec![SeedApiKey {
            name: "dev".to_string(),
            public_id: "dev123".to_string(),
            secret_hash: "$argon2id$v=19$m=19456,t=2,p=1$8WJ6UydAx2RbDXy+zuYbAw$EF+rEtkc71VhwwvS+TS6EiZZvW6rtrjzXX4XvIsDhbU".to_string(),
            allowed_models: vec!["fast".to_string()],
        }];

        store
            .seed_from_inputs(&providers, &models, &api_keys)
            .await
            .expect("seed #1");

        store
            .seed_from_inputs(&providers, &models, &api_keys)
            .await
            .expect("seed #2 idempotent");

        let api_key = store
            .get_api_key_by_public_id("dev123")
            .await
            .expect("query key")
            .expect("api key should exist");
        assert_eq!(api_key.owner_kind, ApiKeyOwnerKind::Team);
        assert_eq!(
            api_key.owner_team_id,
            Some(Uuid::parse_str(SYSTEM_LEGACY_TEAM_ID).expect("legacy team uuid"))
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
                }],
            },
        ];

        let api_keys = vec![SeedApiKey {
            name: "dev".to_string(),
            public_id: "dev123".to_string(),
            secret_hash: "$argon2id$v=19$m=19456,t=2,p=1$8WJ6UydAx2RbDXy+zuYbAw$EF+rEtkc71VhwwvS+TS6EiZZvW6rtrjzXX4XvIsDhbU".to_string(),
            allowed_models: vec!["fast".to_string()],
        }];

        store
            .seed_from_inputs(&providers, &models, &api_keys)
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
    async fn libsql_request_log_migration_backfills_resolved_model_key() {
        let tmp = tempdir().expect("tempdir");
        let db_path = tmp.path().join("gateway.db");

        apply_libsql_migrations_through(&db_path, 10)
            .await
            .expect("migrations through v10");

        let db = libsql::Builder::new_local(db_path.to_str().expect("db path"))
            .build()
            .await
            .expect("db");
        let conn = db.connect().expect("connection");

        conn.execute(
            r#"
            INSERT INTO api_keys (
                id, public_id, secret_hash, name, status,
                owner_kind, owner_user_id, owner_team_id, created_at
            ) VALUES (?1, 'legacy', 'hash', 'Legacy', 'active', 'team', NULL, ?2, unixepoch())
            "#,
            libsql::params!["api-key-legacy", SYSTEM_LEGACY_TEAM_ID],
        )
        .await
        .expect("insert api key");

        conn.execute(
            r#"
            INSERT INTO request_logs (
                request_log_id, request_id, api_key_id, user_id, team_id, model_key,
                provider_key, status_code, latency_ms, prompt_tokens, completion_tokens,
                total_tokens, error_code, metadata_json, occurred_at
            ) VALUES (?1, ?2, ?3, NULL, NULL, ?4, ?5, 200, 42, 10, 20, 30, NULL, '{}', unixepoch())
            "#,
            libsql::params![
                Uuid::new_v4().to_string(),
                "req-legacy",
                "api-key-legacy",
                "fast",
                "openai-prod"
            ],
        )
        .await
        .expect("insert legacy row");

        run_migrations_with_options(
            &StoreConnectionOptions::Libsql {
                path: db_path.clone(),
            },
            MigrationTestHook::default(),
        )
        .await
        .expect("apply v11");

        let mut rows = conn
            .query(
                "SELECT model_key, resolved_model_key FROM request_logs WHERE request_id = 'req-legacy'",
                (),
            )
            .await
            .expect("query request logs");
        let row = rows
            .next()
            .await
            .expect("row fetch")
            .expect("row should exist");
        let model_key: String = row.get(0).expect("model key");
        let resolved_model_key: Option<String> = row.get(1).expect("resolved model key");
        assert_eq!(model_key, "fast");
        assert_eq!(resolved_model_key.as_deref(), Some("fast"));
    }

    #[tokio::test]
    #[serial]
    async fn migration_backfills_legacy_api_keys_with_reserved_team_owner() {
        let tmp = tempdir().expect("tempdir");
        let db_path = tmp.path().join("gateway.db");
        let db = libsql::Builder::new_local(db_path.to_str().expect("db path"))
            .build()
            .await
            .expect("db");
        let conn = db.connect().expect("connection");

        conn.execute(
            r#"
            CREATE TABLE refinery_schema_history (
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
        conn.execute_batch(include_str!("../migrations/V1__init.sql"))
            .await
            .expect("v1 schema");
        conn.execute_batch(include_str!("../migrations/V2__audit_baseline.sql"))
            .await
            .expect("v2 schema");
        conn.execute(
            "INSERT INTO refinery_schema_history (version, name, applied_on, checksum) VALUES (1, 'init', unixepoch(), 'v1')",
            (),
        )
        .await
        .expect("history v1");
        conn.execute(
            "INSERT INTO refinery_schema_history (version, name, applied_on, checksum) VALUES (2, 'audit_baseline', unixepoch(), 'v2')",
            (),
        )
        .await
        .expect("history v2");
        conn.execute(
            r#"
            INSERT INTO api_keys (id, public_id, secret_hash, name, status, created_at)
            VALUES (?1, 'legacy', 'hash', 'legacy key', 'active', unixepoch())
            "#,
            [Uuid::new_v4().to_string()],
        )
        .await
        .expect("legacy api key");

        run_migrations(&db_path).await.expect("migrate to v3");

        let store = LibsqlStore::new_local(db_path.to_str().expect("db path"))
            .await
            .expect("store");
        let key = store
            .get_api_key_by_public_id("legacy")
            .await
            .expect("query")
            .expect("legacy key should exist");
        assert_eq!(key.owner_kind, ApiKeyOwnerKind::Team);
        assert_eq!(
            key.owner_team_id,
            Some(Uuid::parse_str(SYSTEM_LEGACY_TEAM_ID).expect("legacy team uuid"))
        );
        assert_eq!(key.owner_user_id, None);
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
    }

    #[tokio::test]
    #[serial]
    async fn user_budget_enforces_single_active_record_per_user() {
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
            INSERT INTO user_budgets (
                user_budget_id, user_id, cadence, amount_10000, hard_limit, timezone, is_active, created_at, updated_at
            ) VALUES (?1, ?2, 'daily', 100000, 1, 'UTC', 1, ?3, ?3)
            "#,
            libsql::params![Uuid::new_v4().to_string(), user_id.to_string(), now],
        )
        .await
        .expect("first budget");

        let duplicate_active_result = conn
            .execute(
                r#"
                INSERT INTO user_budgets (
                    user_budget_id, user_id, cadence, amount_10000, hard_limit, timezone, is_active, created_at, updated_at
                ) VALUES (?1, ?2, 'weekly', 200000, 1, 'UTC', 1, ?3, ?3)
                "#,
                libsql::params![Uuid::new_v4().to_string(), user_id.to_string(), now],
            )
            .await;
        assert!(duplicate_active_result.is_err());

        conn.execute(
            r#"
            INSERT INTO user_budgets (
                user_budget_id, user_id, cadence, amount_10000, hard_limit, timezone, is_active, created_at, updated_at
            ) VALUES (?1, ?2, 'weekly', 200000, 1, 'UTC', 0, ?3, ?3)
            "#,
            libsql::params![Uuid::new_v4().to_string(), user_id.to_string(), now],
        )
        .await
        .expect("inactive budget should be allowed");
    }

    #[tokio::test]
    #[serial]
    async fn team_budget_enforces_single_active_record_per_team() {
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
            INSERT INTO team_budgets (
                team_budget_id, team_id, cadence, amount_10000, hard_limit, timezone, is_active, created_at, updated_at
            ) VALUES (?1, ?2, 'daily', 100000, 1, 'UTC', 1, ?3, ?3)
            "#,
            libsql::params![Uuid::new_v4().to_string(), team_id.to_string(), now],
        )
        .await
        .expect("first budget");

        let duplicate_active_result = conn
            .execute(
                r#"
                INSERT INTO team_budgets (
                    team_budget_id, team_id, cadence, amount_10000, hard_limit, timezone, is_active, created_at, updated_at
                ) VALUES (?1, ?2, 'weekly', 200000, 1, 'UTC', 1, ?3, ?3)
                "#,
                libsql::params![Uuid::new_v4().to_string(), team_id.to_string(), now],
            )
            .await;
        assert!(duplicate_active_result.is_err());

        conn.execute(
            r#"
            INSERT INTO team_budgets (
                team_budget_id, team_id, cadence, amount_10000, hard_limit, timezone, is_active, created_at, updated_at
            ) VALUES (?1, ?2, 'weekly', 200000, 1, 'UTC', 0, ?3, ?3)
            "#,
            libsql::params![Uuid::new_v4().to_string(), team_id.to_string(), now],
        )
        .await
        .expect("inactive budget should be allowed");
    }

    #[tokio::test]
    #[serial]
    async fn v4_migration_converts_money_columns_to_scaled_integers() {
        let tmp = tempdir().expect("tempdir");
        let db_path = tmp.path().join("gateway.db");
        let db = libsql::Builder::new_local(db_path.to_str().expect("db path"))
            .build()
            .await
            .expect("db");
        let conn = db.connect().expect("connection");
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let user_id = Uuid::new_v4();
        let api_key_id = Uuid::new_v4();
        let budget_id = Uuid::new_v4();
        let usage_event_id = Uuid::new_v4();

        conn.execute(
            r#"
            CREATE TABLE refinery_schema_history (
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
        conn.execute_batch(include_str!("../migrations/V1__init.sql"))
            .await
            .expect("v1 schema");
        conn.execute_batch(include_str!("../migrations/V2__audit_baseline.sql"))
            .await
            .expect("v2 schema");
        conn.execute_batch(include_str!("../migrations/V3__identity_foundation.sql"))
            .await
            .expect("v3 schema");
        conn.execute(
            "INSERT INTO refinery_schema_history (version, name, applied_on, checksum) VALUES (1, 'init', unixepoch(), 'v1')",
            (),
        )
        .await
        .expect("history v1");
        conn.execute(
            "INSERT INTO refinery_schema_history (version, name, applied_on, checksum) VALUES (2, 'audit_baseline', unixepoch(), 'v2')",
            (),
        )
        .await
        .expect("history v2");
        conn.execute(
            "INSERT INTO refinery_schema_history (version, name, applied_on, checksum) VALUES (3, 'identity_foundation', unixepoch(), 'v3')",
            (),
        )
        .await
        .expect("history v3");

        conn.execute(
            r#"
            INSERT INTO users (
              user_id, name, email, email_normalized, global_role, auth_mode, status,
              request_logging_enabled, model_access_mode, created_at, updated_at
            ) VALUES (?1, 'Money User', 'money@example.com', 'money@example.com', 'user', 'password', 'active', 1, 'all', ?2, ?2)
            "#,
            libsql::params![user_id.to_string(), now],
        )
        .await
        .expect("user");
        conn.execute(
            r#"
            INSERT INTO api_keys (
                id, public_id, secret_hash, name, status, owner_kind, owner_user_id, owner_team_id, created_at
            ) VALUES (?1, 'money_key', 'hash', 'money key', 'active', 'user', ?2, NULL, ?3)
            "#,
            libsql::params![api_key_id.to_string(), user_id.to_string(), now],
        )
        .await
        .expect("api key");
        conn.execute(
            r#"
            INSERT INTO user_budgets (
                user_budget_id, user_id, cadence, amount_usd, hard_limit, timezone, is_active, created_at, updated_at
            ) VALUES (?1, ?2, 'weekly', 12.3456, 1, 'UTC', 1, ?3, ?3)
            "#,
            libsql::params![budget_id.to_string(), user_id.to_string(), now],
        )
        .await
        .expect("budget");
        conn.execute(
            r#"
            INSERT INTO usage_cost_events (
                usage_event_id, request_id, api_key_id, user_id, team_id, model_id, estimated_cost_usd, occurred_at
            ) VALUES (?1, 'req_money', ?2, ?3, NULL, NULL, 0.6789, ?4)
            "#,
            libsql::params![
                usage_event_id.to_string(),
                api_key_id.to_string(),
                user_id.to_string(),
                now
            ],
        )
        .await
        .expect("usage event");

        run_migrations(&db_path).await.expect("migrate to v4");

        let mut budget_rows = conn
            .query(
                "SELECT amount_10000 FROM user_budgets WHERE user_budget_id = ?1",
                [budget_id.to_string()],
            )
            .await
            .expect("query budget amount");
        let budget_row = budget_rows
            .next()
            .await
            .expect("fetch budget row")
            .expect("budget row");
        let amount_10000: i64 = budget_row.get(0).expect("decode budget amount");
        assert_eq!(amount_10000, 123_456);

        let mut usage_rows = conn
            .query(
                "SELECT computed_cost_10000, pricing_status, ownership_scope_key, provider_key, upstream_model FROM usage_cost_events WHERE usage_event_id = ?1",
                [usage_event_id.to_string()],
            )
            .await
            .expect("query usage amount");
        let usage_row = usage_rows
            .next()
            .await
            .expect("fetch usage row")
            .expect("usage row");
        let computed_cost_10000: i64 = usage_row.get(0).expect("decode usage amount");
        let pricing_status: String = usage_row.get(1).expect("decode pricing status");
        let ownership_scope_key: String = usage_row.get(2).expect("decode scope key");
        let provider_key: String = usage_row.get(3).expect("decode provider key");
        let upstream_model: String = usage_row.get(4).expect("decode upstream model");
        assert_eq!(computed_cost_10000, 6_789);
        assert_eq!(pricing_status, "legacy_estimated");
        assert_eq!(ownership_scope_key, format!("user:{user_id}"));
        assert_eq!(provider_key, "legacy");
        assert_eq!(upstream_model, "legacy");

        let mut budget_columns = conn
            .query("PRAGMA table_info(user_budgets)", ())
            .await
            .expect("budget table info");
        while let Some(column) = budget_columns.next().await.expect("column row") {
            let column_name: String = column.get(1).expect("column name");
            assert_ne!(column_name, "amount_usd");
        }

        let mut usage_columns = conn
            .query("PRAGMA table_info(usage_cost_events)", ())
            .await
            .expect("usage table info");
        while let Some(column) = usage_columns.next().await.expect("column row") {
            let column_name: String = column.get(1).expect("column name");
            assert_ne!(column_name, "estimated_cost_usd");
        }

        let mut pricing_columns = conn
            .query("PRAGMA table_info(model_pricing)", ())
            .await
            .expect("pricing table info");
        let mut saw_effective_start_at = false;
        while let Some(column) = pricing_columns.next().await.expect("column row") {
            let column_name: String = column.get(1).expect("column name");
            if column_name == "effective_start_at" {
                saw_effective_start_at = true;
            }
        }
        assert!(
            saw_effective_start_at,
            "model_pricing should be created by v8"
        );
    }

    #[tokio::test]
    #[serial]
    async fn v8_migration_deduplicates_legacy_usage_rows_into_archive() {
        let tmp = tempdir().expect("tempdir");
        let db_path = tmp.path().join("gateway.db");
        let db = libsql::Builder::new_local(db_path.to_str().expect("db path"))
            .build()
            .await
            .expect("db");
        let conn = db.connect().expect("connection");
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let user_id = Uuid::new_v4();
        let api_key_id = Uuid::new_v4();
        let first_usage_event_id = Uuid::new_v4();
        let second_usage_event_id = Uuid::new_v4();

        conn.execute(
            r#"
            CREATE TABLE refinery_schema_history (
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
        conn.execute_batch(include_str!("../migrations/V1__init.sql"))
            .await
            .expect("v1 schema");
        conn.execute_batch(include_str!("../migrations/V2__audit_baseline.sql"))
            .await
            .expect("v2 schema");
        conn.execute_batch(include_str!("../migrations/V3__identity_foundation.sql"))
            .await
            .expect("v3 schema");
        conn.execute_batch(include_str!("../migrations/V4__money_fixed_point.sql"))
            .await
            .expect("v4 schema");
        for (version, name, checksum) in [
            (1_i64, "init", "v1"),
            (2_i64, "audit_baseline", "v2"),
            (3_i64, "identity_foundation", "v3"),
            (4_i64, "money_fixed_point", "v4"),
        ] {
            conn.execute(
                "INSERT INTO refinery_schema_history (version, name, applied_on, checksum) VALUES (?1, ?2, unixepoch(), ?3)",
                libsql::params![version, name, checksum],
            )
            .await
            .expect("history row");
        }

        conn.execute(
            r#"
            INSERT INTO users (
              user_id, name, email, email_normalized, global_role, auth_mode, status,
              request_logging_enabled, model_access_mode, created_at, updated_at
            ) VALUES (?1, 'Money User', 'money@example.com', 'money@example.com', 'user', 'password', 'active', 1, 'all', ?2, ?2)
            "#,
            libsql::params![user_id.to_string(), now],
        )
        .await
        .expect("user");
        conn.execute(
            r#"
            INSERT INTO api_keys (
                id, public_id, secret_hash, name, status, owner_kind, owner_user_id, owner_team_id, created_at
            ) VALUES (?1, 'money_key', 'hash', 'money key', 'active', 'user', ?2, NULL, ?3)
            "#,
            libsql::params![api_key_id.to_string(), user_id.to_string(), now],
        )
        .await
        .expect("api key");
        for usage_event_id in [first_usage_event_id, second_usage_event_id] {
            conn.execute(
                r#"
                INSERT INTO usage_cost_events (
                    usage_event_id, request_id, api_key_id, user_id, team_id, model_id,
                    estimated_cost_10000, occurred_at
                ) VALUES (?1, 'req_dupe', ?2, ?3, NULL, NULL, 6789, ?4)
                "#,
                libsql::params![
                    usage_event_id.to_string(),
                    api_key_id.to_string(),
                    user_id.to_string(),
                    now
                ],
            )
            .await
            .expect("usage event");
        }

        run_migrations(&db_path).await.expect("migrate to v8");

        let mut deduped_rows = conn
            .query(
                "SELECT request_id, ownership_scope_key, pricing_status FROM usage_cost_events",
                (),
            )
            .await
            .expect("deduped rows");
        let deduped = deduped_rows
            .next()
            .await
            .expect("fetch deduped")
            .expect("deduped row");
        let request_id: String = deduped.get(0).expect("request id");
        let ownership_scope_key: String = deduped.get(1).expect("scope key");
        let pricing_status: String = deduped.get(2).expect("pricing status");
        assert_eq!(request_id, "req_dupe");
        assert_eq!(ownership_scope_key, format!("user:{user_id}"));
        assert_eq!(pricing_status, "legacy_estimated");
        assert!(
            deduped_rows
                .next()
                .await
                .expect("second deduped row")
                .is_none()
        );

        let mut archived_rows = conn
            .query(
                "SELECT original_usage_event_id, request_id, ownership_scope_key FROM usage_cost_event_duplicates_archive",
                (),
            )
            .await
            .expect("archive rows");
        let archived = archived_rows
            .next()
            .await
            .expect("fetch archive")
            .expect("archive row");
        let archived_request_id: String = archived.get(1).expect("archived request id");
        let archived_scope_key: String = archived.get(2).expect("archived scope key");
        assert_eq!(archived_request_id, "req_dupe");
        assert_eq!(archived_scope_key, format!("user:{user_id}"));
        assert!(
            archived_rows
                .next()
                .await
                .expect("second archive row")
                .is_none()
        );
    }

    #[tokio::test]
    #[serial]
    async fn postgres_v8_migration_deduplicates_legacy_usage_rows_into_archive() {
        let Some(test_db) = create_postgres_test_database().await else {
            eprintln!("skipping postgres v8 migration test because TEST_POSTGRES_URL is not set");
            return;
        };

        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&test_db.database_url)
            .await
            .expect("postgres pool");
        let now = OffsetDateTime::now_utc().unix_timestamp();
        let user_id = Uuid::new_v4();
        let api_key_id = Uuid::new_v4();
        let first_usage_event_id = Uuid::new_v4();
        let second_usage_event_id = Uuid::new_v4();

        sqlx::raw_sql(include_str!("../migrations/postgres/V1__init.sql"))
            .execute(&pool)
            .await
            .expect("v1 schema");
        sqlx::query(
            r#"
            CREATE TABLE refinery_schema_history (
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
        for (version, name, checksum) in [
            (1_i64, "init", "V1__init.sql"),
            (2_i64, "audit_baseline", "V2__audit_baseline.sql"),
            (3_i64, "identity_foundation", "V3__identity_foundation.sql"),
            (4_i64, "money_fixed_point", "V4__money_fixed_point.sql"),
            (
                5_i64,
                "pricing_catalog_cache",
                "V5__pricing_catalog_cache.sql",
            ),
            (6_i64, "identity_onboarding", "V6__identity_onboarding.sql"),
            (
                7_i64,
                "user_password_rotation",
                "V7__user_password_rotation.sql",
            ),
        ] {
            sqlx::query(
                "INSERT INTO refinery_schema_history (version, name, applied_on, checksum) VALUES ($1, $2, $3, $4)",
            )
            .bind(version)
            .bind(name)
            .bind(now)
            .bind(checksum)
            .execute(&pool)
            .await
            .expect("history row");
        }
        sqlx::query(
            r#"
            INSERT INTO users (
                user_id, name, email, email_normalized, global_role, auth_mode, status,
                must_change_password, request_logging_enabled, model_access_mode, created_at, updated_at
            ) VALUES ($1, 'Money User', 'money@example.com', 'money@example.com', 'user', 'password', 'active', 0, 1, 'all', $2, $2)
            "#,
        )
        .bind(user_id.to_string())
        .bind(now)
        .execute(&pool)
        .await
        .expect("user");
        sqlx::query(
            r#"
            INSERT INTO api_keys (
                id, public_id, secret_hash, name, status, owner_kind, owner_user_id, owner_team_id, created_at
            ) VALUES ($1, 'money_key', 'hash', 'money key', 'active', 'user', $2, NULL, $3)
            "#,
        )
        .bind(api_key_id.to_string())
        .bind(user_id.to_string())
        .bind(now)
        .execute(&pool)
        .await
        .expect("api key");
        for usage_event_id in [first_usage_event_id, second_usage_event_id] {
            sqlx::query(
                r#"
                INSERT INTO usage_cost_events (
                    usage_event_id, request_id, api_key_id, user_id, team_id, model_id,
                    estimated_cost_10000, occurred_at
                ) VALUES ($1, 'req_dupe', $2, $3, NULL, NULL, 6789, $4)
                "#,
            )
            .bind(usage_event_id.to_string())
            .bind(api_key_id.to_string())
            .bind(user_id.to_string())
            .bind(now)
            .execute(&pool)
            .await
            .expect("usage row");
        }
        pool.close().await;

        let options = StoreConnectionOptions::Postgres {
            url: test_db.database_url.clone(),
            max_connections: 2,
        };
        run_migrations_with_options(&options, MigrationTestHook::default())
            .await
            .expect("migrate to v8");

        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&test_db.database_url)
            .await
            .expect("postgres pool");
        let deduped = sqlx::query(
            "SELECT request_id, ownership_scope_key, pricing_status FROM usage_cost_events",
        )
        .fetch_all(&pool)
        .await
        .expect("deduped rows");
        assert_eq!(deduped.len(), 1);
        assert_eq!(
            deduped[0].try_get::<String, _>(0).expect("request id"),
            "req_dupe"
        );
        assert_eq!(
            deduped[0]
                .try_get::<String, _>(1)
                .expect("ownership scope key"),
            format!("user:{user_id}")
        );
        assert_eq!(
            deduped[0].try_get::<String, _>(2).expect("pricing status"),
            "legacy_estimated"
        );

        let archived = sqlx::query(
            "SELECT request_id, ownership_scope_key FROM usage_cost_event_duplicates_archive",
        )
        .fetch_all(&pool)
        .await
        .expect("archived rows");
        assert_eq!(archived.len(), 1);
        assert_eq!(
            archived[0].try_get::<String, _>(0).expect("request id"),
            "req_dupe"
        );
        assert_eq!(
            archived[0]
                .try_get::<String, _>(1)
                .expect("ownership scope key"),
            format!("user:{user_id}")
        );
        let model_pricing_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM information_schema.tables WHERE table_name = 'model_pricing')",
        )
        .fetch_one(&pool)
        .await
        .expect("model_pricing exists");
        assert!(model_pricing_exists);

        pool.close().await;
        drop_postgres_test_database(&test_db).await;
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
    async fn libsql_spend_reporting_aggregates_and_team_window_sum_filter_chargeable_statuses() {
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
            }],
        }];
        let api_keys = vec![SeedApiKey {
            name: "dev".to_string(),
            public_id: "dev123".to_string(),
            secret_hash: "hash".to_string(),
            allowed_models: vec!["fast".to_string()],
        }];
        store
            .seed_from_inputs(&providers, &models, &api_keys)
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
        let team = store
            .create_team("platform", "Platform")
            .await
            .expect("create team");
        let user = store
            .create_identity_user(
                "Member",
                "member@example.com",
                "member@example.com",
                GlobalRole::User,
                AuthMode::Password,
                "active",
            )
            .await
            .expect("create user");

        let now = OffsetDateTime::from_unix_timestamp(1_773_484_800).expect("timestamp");
        let budget = store
            .upsert_active_budget_for_team(
                team.team_id,
                BudgetCadence::Daily,
                Money4::from_scaled(100_000),
                true,
                "UTC",
                now,
            )
            .await
            .expect("upsert team budget");
        assert_eq!(budget.amount_usd, Money4::from_scaled(100_000));

        let day_one = OffsetDateTime::from_unix_timestamp(1_773_486_600).expect("day one");
        let day_two = day_one + Duration::days(1);

        for event in [
            build_usage_ledger_record(
                "req-user-priced",
                format!("user:{}", user.user_id),
                api_key.id,
                Some(user.user_id),
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
                Some(model.id),
                "gpt-4o-mini",
                UsagePricingStatus::Unpriced,
                0,
                day_one,
            ),
            build_usage_ledger_record(
                "req-team-legacy",
                format!("team:{}:actor:none", team.team_id),
                api_key.id,
                None,
                Some(team.team_id),
                None,
                "claude-3-5-sonnet",
                UsagePricingStatus::LegacyEstimated,
                22_000,
                day_two,
            ),
            build_usage_ledger_record(
                "req-team-unpriced",
                format!("team:{}:actor:none", team.team_id),
                api_key.id,
                None,
                Some(team.team_id),
                None,
                "claude-3-5-sonnet",
                UsagePricingStatus::Unpriced,
                0,
                day_two,
            ),
            build_usage_ledger_record(
                "req-team-usage-missing",
                format!("team:{}:actor:none", team.team_id),
                api_key.id,
                None,
                Some(team.team_id),
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
        let team_sum = store
            .sum_usage_cost_for_team_in_window(team.team_id, window_start, window_end)
            .await
            .expect("team sum");
        assert_eq!(team_sum, Money4::from_scaled(22_000));

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
        let team_owner = owners
            .iter()
            .find(|row| row.owner_kind == ApiKeyOwnerKind::Team)
            .expect("team owner aggregate");
        assert_eq!(team_owner.owner_id, team.team_id);
        assert_eq!(team_owner.priced_cost_usd, Money4::from_scaled(22_000));
        assert_eq!(team_owner.priced_request_count, 1);
        assert_eq!(team_owner.unpriced_request_count, 1);
        assert_eq!(team_owner.usage_missing_request_count, 1);

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
        run_migrations_with_options(&options, MigrationTestHook::default())
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
            }],
        }];
        let api_keys = vec![SeedApiKey {
            name: "dev".to_string(),
            public_id: "dev123".to_string(),
            secret_hash: "hash".to_string(),
            allowed_models: vec!["fast".to_string()],
        }];

        store
            .seed_from_inputs(&providers, &models, &api_keys)
            .await
            .expect("seed");

        let key = store
            .get_api_key_by_public_id("dev123")
            .await
            .expect("get key")
            .expect("api key exists");
        assert_eq!(key.owner_kind, ApiKeyOwnerKind::Team);
        assert_eq!(
            key.owner_team_id.expect("team owner").to_string(),
            SYSTEM_LEGACY_TEAM_ID
        );
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
                "invited",
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
                .sum_usage_cost_for_user_in_window(
                    member.user_id,
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
    async fn postgres_team_budget_enforces_single_active_record_per_team() {
        let Some(test_db) = create_postgres_test_database().await else {
            eprintln!(
                "skipping postgres team budget uniqueness test because TEST_POSTGRES_URL is not set"
            );
            return;
        };

        let options = StoreConnectionOptions::Postgres {
            url: test_db.database_url.clone(),
            max_connections: 2,
        };
        run_migrations_with_options(&options, MigrationTestHook::default())
            .await
            .expect("postgres migrations");

        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&test_db.database_url)
            .await
            .expect("postgres pool");
        let now = OffsetDateTime::now_utc().unix_timestamp();
        let team_id = Uuid::new_v4();

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
            INSERT INTO team_budgets (
                team_budget_id, team_id, cadence, amount_10000, hard_limit, timezone, is_active, created_at, updated_at
            ) VALUES ($1, $2, 'daily', 100000, 1, 'UTC', 1, $3, $3)
            "#,
        )
        .bind(Uuid::new_v4().to_string())
        .bind(team_id.to_string())
        .bind(now)
        .execute(&pool)
        .await
        .expect("first budget");

        let duplicate_active_result = sqlx::query(
            r#"
            INSERT INTO team_budgets (
                team_budget_id, team_id, cadence, amount_10000, hard_limit, timezone, is_active, created_at, updated_at
            ) VALUES ($1, $2, 'weekly', 200000, 1, 'UTC', 1, $3, $3)
            "#,
        )
        .bind(Uuid::new_v4().to_string())
        .bind(team_id.to_string())
        .bind(now)
        .execute(&pool)
        .await;
        assert!(duplicate_active_result.is_err());

        sqlx::query(
            r#"
            INSERT INTO team_budgets (
                team_budget_id, team_id, cadence, amount_10000, hard_limit, timezone, is_active, created_at, updated_at
            ) VALUES ($1, $2, 'weekly', 200000, 1, 'UTC', 0, $3, $3)
            "#,
        )
        .bind(Uuid::new_v4().to_string())
        .bind(team_id.to_string())
        .bind(now)
        .execute(&pool)
        .await
        .expect("inactive budget should be allowed");

        pool.close().await;
        drop_postgres_test_database(&test_db).await;
    }

    #[tokio::test]
    #[serial]
    async fn postgres_spend_reporting_aggregates_and_team_window_sum_filter_chargeable_statuses() {
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
        run_migrations_with_options(&options, MigrationTestHook::default())
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
            }],
        }];
        let api_keys = vec![SeedApiKey {
            name: "dev".to_string(),
            public_id: "dev123".to_string(),
            secret_hash: "hash".to_string(),
            allowed_models: vec!["fast".to_string()],
        }];
        store
            .seed_from_inputs(&providers, &models, &api_keys)
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
        let team = store
            .create_team("platform", "Platform")
            .await
            .expect("create team");
        let user = store
            .create_identity_user(
                "Member",
                "member@example.com",
                "member@example.com",
                GlobalRole::User,
                AuthMode::Password,
                "active",
            )
            .await
            .expect("create user");

        let now = OffsetDateTime::from_unix_timestamp(1_773_484_800).expect("timestamp");
        let budget = store
            .upsert_active_budget_for_team(
                team.team_id,
                BudgetCadence::Daily,
                Money4::from_scaled(100_000),
                true,
                "UTC",
                now,
            )
            .await
            .expect("upsert team budget");
        assert_eq!(budget.amount_usd, Money4::from_scaled(100_000));

        let day_one = OffsetDateTime::from_unix_timestamp(1_773_486_600).expect("day one");
        let day_two = day_one + Duration::days(1);

        for event in [
            build_usage_ledger_record(
                "req-user-priced",
                format!("user:{}", user.user_id),
                api_key.id,
                Some(user.user_id),
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
                Some(model.id),
                "gpt-4o-mini",
                UsagePricingStatus::Unpriced,
                0,
                day_one,
            ),
            build_usage_ledger_record(
                "req-team-legacy",
                format!("team:{}:actor:none", team.team_id),
                api_key.id,
                None,
                Some(team.team_id),
                None,
                "claude-3-5-sonnet",
                UsagePricingStatus::LegacyEstimated,
                22_000,
                day_two,
            ),
            build_usage_ledger_record(
                "req-team-unpriced",
                format!("team:{}:actor:none", team.team_id),
                api_key.id,
                None,
                Some(team.team_id),
                None,
                "claude-3-5-sonnet",
                UsagePricingStatus::Unpriced,
                0,
                day_two,
            ),
            build_usage_ledger_record(
                "req-team-usage-missing",
                format!("team:{}:actor:none", team.team_id),
                api_key.id,
                None,
                Some(team.team_id),
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
        let team_sum = store
            .sum_usage_cost_for_team_in_window(team.team_id, window_start, window_end)
            .await
            .expect("team sum");
        assert_eq!(team_sum, Money4::from_scaled(22_000));

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
        let team_owner = owners
            .iter()
            .find(|row| row.owner_kind == ApiKeyOwnerKind::Team)
            .expect("team owner aggregate");
        assert_eq!(team_owner.owner_id, team.team_id);
        assert_eq!(team_owner.priced_cost_usd, Money4::from_scaled(22_000));
        assert_eq!(team_owner.priced_request_count, 1);
        assert_eq!(team_owner.unpriced_request_count, 1);
        assert_eq!(team_owner.usage_missing_request_count, 1);

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
        run_migrations_with_options(&options, MigrationTestHook::default())
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
                }],
            },
        ];
        let api_keys = vec![SeedApiKey {
            name: "dev".to_string(),
            public_id: "dev123".to_string(),
            secret_hash: "hash".to_string(),
            allowed_models: vec!["fast".to_string()],
        }];

        store
            .seed_from_inputs(&providers, &models, &api_keys)
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
    async fn postgres_request_log_migration_backfills_resolved_model_key() {
        let Some(test_db) = create_postgres_test_database().await else {
            eprintln!(
                "skipping postgres request log backfill test because TEST_POSTGRES_URL is not set"
            );
            return;
        };

        apply_postgres_migrations_through(&test_db.database_url, 10)
            .await
            .expect("migrations through v10");

        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&test_db.database_url)
            .await
            .expect("postgres pool");

        sqlx::query(
            r#"
            INSERT INTO teams (
                team_id, team_key, team_name, status, model_access_mode, created_at, updated_at
            ) VALUES ($1, $2, $3, 'active', 'all', extract(epoch from now())::bigint, extract(epoch from now())::bigint)
            "#,
        )
        .bind(SYSTEM_LEGACY_TEAM_ID)
        .bind(SYSTEM_LEGACY_TEAM_KEY)
        .bind("System Legacy")
        .execute(&pool)
        .await
        .expect("insert team");

        sqlx::query(
            r#"
            INSERT INTO api_keys (
                id, public_id, secret_hash, name, status,
                owner_kind, owner_user_id, owner_team_id, created_at
            ) VALUES ($1, 'legacy', 'hash', 'Legacy', 'active', 'team', NULL, $2, extract(epoch from now())::bigint)
            "#,
        )
        .bind("api-key-legacy")
        .bind(SYSTEM_LEGACY_TEAM_ID)
        .execute(&pool)
        .await
        .expect("insert api key");

        sqlx::query(
            r#"
            INSERT INTO request_logs (
                request_log_id, request_id, api_key_id, user_id, team_id, model_key,
                provider_key, status_code, latency_ms, prompt_tokens, completion_tokens,
                total_tokens, error_code, metadata_json, occurred_at
            ) VALUES ($1, $2, $3, NULL, NULL, $4, $5, 200, 42, 10, 20, 30, NULL, '{}', extract(epoch from now())::bigint)
            "#,
        )
        .bind(Uuid::new_v4().to_string())
        .bind("req-legacy")
        .bind("api-key-legacy")
        .bind("fast")
        .bind("openai-prod")
        .execute(&pool)
        .await
        .expect("insert legacy row");

        run_migrations_with_options(
            &StoreConnectionOptions::Postgres {
                url: test_db.database_url.clone(),
                max_connections: 2,
            },
            MigrationTestHook::default(),
        )
        .await
        .expect("apply v11");

        let row = sqlx::query(
            "SELECT model_key, resolved_model_key FROM request_logs WHERE request_id = $1",
        )
        .bind("req-legacy")
        .fetch_one(&pool)
        .await
        .expect("load request log");
        let model_key: String = row.try_get(0).expect("model key");
        let resolved_model_key: Option<String> = row.try_get(1).expect("resolved model key");
        assert_eq!(model_key, "fast");
        assert_eq!(resolved_model_key.as_deref(), Some("fast"));

        pool.close().await;
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

        run_migrations_with_options(&options, MigrationTestHook::default())
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
        run_migrations_with_options(
            &options,
            MigrationTestHook {
                fail_after_apply_version: Some(1),
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
        run_migrations_with_options(
            &options,
            MigrationTestHook {
                fail_history_insert_version: Some(1),
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

        let initial_status = status_migrations_with_options(&options)
            .await
            .expect("initial postgres status");
        assert_eq!(initial_status.pending_count(), MIGRATION_REGISTRY.len());
        assert!(initial_status.entries.iter().all(|entry| !entry.applied));

        run_migrations_with_options(
            &options,
            MigrationTestHook {
                fail_history_insert_version: Some(1),
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

        run_migrations_with_options(&options, MigrationTestHook::default())
            .await
            .expect("postgres retry migrations");

        let applied_status = status_migrations_with_options(&options)
            .await
            .expect("status after retry");
        assert_eq!(applied_status.pending_count(), 0);
        assert!(applied_status.entries.iter().all(|entry| entry.applied));

        drop_postgres_test_database(&test_db).await;
    }

    async fn apply_libsql_migrations_through(
        db_path: &std::path::Path,
        target_version: u32,
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

        for migration in MIGRATION_REGISTRY
            .iter()
            .filter(|migration| migration.version <= target_version)
        {
            let tx = conn.transaction().await.expect("migration tx");
            if let BackendMigrationStep::Sql(sql) = migration.step_for(MigrationBackend::Libsql) {
                tx.execute_batch(sql).await.expect("apply migration");
            }
            tx.execute(
                r#"
                INSERT INTO refinery_schema_history (version, name, applied_on, checksum)
                VALUES (?1, ?2, unixepoch(), ?3)
                "#,
                libsql::params![migration.version as i64, migration.name, migration.checksum],
            )
            .await
            .expect("history row");
            tx.commit().await.expect("commit");
        }

        Ok(())
    }

    async fn apply_postgres_migrations_through(
        database_url: &str,
        target_version: u32,
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

        for migration in MIGRATION_REGISTRY
            .iter()
            .filter(|migration| migration.version <= target_version)
        {
            let mut tx = pool.begin().await.expect("migration tx");
            if let BackendMigrationStep::Sql(sql) = migration.step_for(MigrationBackend::Postgres) {
                sqlx::raw_sql(sql)
                    .execute(&mut *tx)
                    .await
                    .expect("apply migration");
            }
            sqlx::query(
                r#"
                INSERT INTO refinery_schema_history (version, name, applied_on, checksum)
                VALUES ($1, $2, extract(epoch from now())::bigint, $3)
                "#,
            )
            .bind(i64::from(migration.version))
            .bind(migration.name)
            .bind(migration.checksum)
            .execute(&mut *tx)
            .await
            .expect("history row");
            tx.commit().await.expect("commit");
        }

        pool.close().await;
        Ok(())
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
