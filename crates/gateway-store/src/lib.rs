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
        BudgetAlertRepository, BudgetCadence, BudgetRepository, GlobalRole, IdentityRepository,
        MembershipRole, ModelPricingRecord, ModelRepository, Money4, PricingCatalogCacheRecord,
        PricingCatalogRepository, PricingLimits, PricingModalities, PricingProvenance,
        ProviderCapabilities, RequestLogRecord, RequestLogRepository, RequestTags,
        SYSTEM_LEGACY_TEAM_ID, SeedApiKey, SeedBudget, SeedModel, SeedModelRoute, SeedProvider,
        SeedTeam, SeedUser, SeedUserMembership, StoreError, StoreHealth, UsageLedgerRecord,
        UsagePricingStatus, UserStatus,
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
        check_migrations_with_options, migration_registry::MIGRATION_REGISTRY, run_migrations,
        run_migrations_with_options, run_migrations_with_options_for_test,
        status_migrations_with_options,
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

    async fn exercise_usage_leaderboard_reporting<S>(store: &S)
    where
        S: ApiKeyRepository
            + BudgetRepository
            + GatewayStore
            + IdentityRepository
            + ModelRepository
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
                }],
            },
        ];
        let api_keys = vec![SeedApiKey {
            name: "dev".to_string(),
            public_id: "dev123".to_string(),
            secret_hash: "hash".to_string(),
            allowed_models: vec!["fast".to_string(), "reasoning".to_string()],
        }];
        store
            .seed_from_inputs(&providers, &models, &api_keys, &[], &[])
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

        let leaders = store
            .list_usage_user_leaderboard(window_start, window_end, 30)
            .await
            .expect("list usage leaderboard");
        assert_eq!(leaders.len(), 3);
        assert_eq!(leaders[0].user_name, "Ada");
        assert_eq!(leaders[0].priced_cost_usd.as_scaled_i64(), 30_000);
        assert_eq!(leaders[0].total_request_count, 3);
        assert_eq!(leaders[0].top_model_key.as_deref(), Some("fast"));
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
            ownership_scope_key: format!("team:{}:actor:none", Uuid::new_v4()),
            owner_kind: ApiKeyOwnerKind::Team,
            owner_id: Uuid::new_v4(),
            owner_name: "Ops".to_string(),
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

        let team_only = repo
            .list_budget_alert_history(&BudgetAlertHistoryQuery {
                page: 1,
                page_size: 10,
                owner_kind: Some(ApiKeyOwnerKind::Team),
                channel: Some(BudgetAlertChannel::Email),
                delivery_status: Some(BudgetAlertDeliveryStatus::Failed),
            })
            .await
            .expect("filter alert history");
        assert_eq!(team_only.total, 1);
        assert_eq!(
            team_only.items[0].budget_alert_id,
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
            }],
        }];

        let api_keys = vec![SeedApiKey {
            name: "dev".to_string(),
            public_id: "dev123".to_string(),
            secret_hash: "$argon2id$v=19$m=19456,t=2,p=1$8WJ6UydAx2RbDXy+zuYbAw$EF+rEtkc71VhwwvS+TS6EiZZvW6rtrjzXX4XvIsDhbU".to_string(),
            allowed_models: vec!["fast".to_string()],
        }];

        store
            .seed_from_inputs(&providers, &models, &api_keys, &[], &[])
            .await
            .expect("seed #1");

        store
            .seed_from_inputs(&providers, &models, &api_keys, &[], &[])
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
            .seed_from_inputs(&providers, &models, &api_keys, &[], &[])
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
                budget: Some(SeedBudget {
                    cadence: BudgetCadence::Monthly,
                    amount_usd: Money4::from_scaled(2_500_000),
                    hard_limit: true,
                    timezone: "UTC".to_string(),
                }),
            },
            SeedTeam {
                team_key: "ops".to_string(),
                team_name: "Ops".to_string(),
                budget: None,
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
            .seed_from_inputs(&[], &[], &[], &initial_teams, &initial_users)
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
        assert_eq!(
            store
                .get_active_budget_for_team(platform_team.team_id)
                .await
                .expect("team budget")
                .expect("platform budget")
                .amount_usd,
            Money4::from_scaled(2_500_000)
        );
        assert_eq!(
            store
                .get_active_budget_for_user(user.user_id)
                .await
                .expect("user budget")
                .expect("user budget exists")
                .amount_usd,
            Money4::from_scaled(750_000)
        );

        let oidc_provider_id = insert_libsql_oidc_provider(&store, "okta").await;

        let updated_teams = vec![
            SeedTeam {
                team_key: "platform".to_string(),
                team_name: "Platform Engineering".to_string(),
                budget: None,
            },
            SeedTeam {
                team_key: "ops".to_string(),
                team_name: "Operations".to_string(),
                budget: Some(SeedBudget {
                    cadence: BudgetCadence::Daily,
                    amount_usd: Money4::from_scaled(125_000),
                    hard_limit: false,
                    timezone: "UTC".to_string(),
                }),
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
            membership: Some(SeedUserMembership {
                team_key: "ops".to_string(),
                role: MembershipRole::Member,
            }),
            budget: None,
        }];

        store
            .seed_from_inputs(&[], &[], &[], &updated_teams, &updated_users)
            .await
            .expect("updated seed");
        store
            .seed_from_inputs(&[], &[], &[], &updated_teams, &updated_users)
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
        assert!(
            store
                .get_active_budget_for_team(refreshed_platform.team_id)
                .await
                .expect("platform budget after reseed")
                .is_none()
        );
        assert_eq!(
            store
                .get_active_budget_for_team(ops_team.team_id)
                .await
                .expect("ops budget")
                .expect("ops budget exists")
                .amount_usd,
            Money4::from_scaled(125_000)
        );
        assert!(
            store
                .get_active_budget_for_user(user.user_id)
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
            budget: Some(SeedBudget {
                cadence: BudgetCadence::Monthly,
                amount_usd: Money4::from_scaled(500_000),
                hard_limit: true,
                timezone: "UTC".to_string(),
            }),
        }];
        let initial_users = vec![SeedUser {
            name: "Member".to_string(),
            email: "member@example.com".to_string(),
            email_normalized: "member@example.com".to_string(),
            global_role: GlobalRole::User,
            auth_mode: AuthMode::Password,
            request_logging_enabled: false,
            oidc_provider_key: None,
            membership: None,
            budget: None,
        }];

        store
            .seed_from_inputs(&[], &[], &[], &initial_teams, &initial_users)
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
            membership: None,
            budget: None,
        }];
        let invalid_teams = vec![SeedTeam {
            team_key: "platform".to_string(),
            team_name: "Platform Renamed".to_string(),
            budget: None,
        }];

        let error = store
            .seed_from_inputs(&[], &[], &[], &invalid_teams, &invalid_users)
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
        assert_eq!(
            store
                .get_active_budget_for_team(platform_team.team_id)
                .await
                .expect("team budget")
                .expect("team budget exists")
                .amount_usd,
            Money4::from_scaled(500_000)
        );
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
            membership: None,
            budget: None,
        }];

        store
            .seed_from_inputs(&[], &[], &[], &[], &initial_users)
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
            membership: None,
            budget: None,
        }];

        let error = store
            .seed_from_inputs(&[], &[], &[], &[], &invalid_users)
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
            membership: None,
            budget: None,
        }];

        store
            .seed_from_inputs(&[], &[], &[], &[], &initial_users)
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
            membership: None,
            budget: None,
        }];

        let error = store
            .seed_from_inputs(&[], &[], &[], &[], &invalid_users)
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
                budget: Some(SeedBudget {
                    cadence: BudgetCadence::Monthly,
                    amount_usd: Money4::from_scaled(2_500_000),
                    hard_limit: true,
                    timezone: "UTC".to_string(),
                }),
            },
            SeedTeam {
                team_key: "ops".to_string(),
                team_name: "Ops".to_string(),
                budget: None,
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
            .seed_from_inputs(&[], &[], &[], &initial_teams, &initial_users)
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
        assert_eq!(
            store
                .get_active_budget_for_team(platform_team.team_id)
                .await
                .expect("team budget")
                .expect("platform budget")
                .amount_usd,
            Money4::from_scaled(2_500_000)
        );
        assert_eq!(
            store
                .get_active_budget_for_user(user.user_id)
                .await
                .expect("user budget")
                .expect("user budget exists")
                .amount_usd,
            Money4::from_scaled(750_000)
        );

        let oidc_provider_id = insert_postgres_oidc_provider(&store, "okta").await;

        let updated_teams = vec![
            SeedTeam {
                team_key: "platform".to_string(),
                team_name: "Platform Engineering".to_string(),
                budget: None,
            },
            SeedTeam {
                team_key: "ops".to_string(),
                team_name: "Operations".to_string(),
                budget: Some(SeedBudget {
                    cadence: BudgetCadence::Daily,
                    amount_usd: Money4::from_scaled(125_000),
                    hard_limit: false,
                    timezone: "UTC".to_string(),
                }),
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
            membership: Some(SeedUserMembership {
                team_key: "ops".to_string(),
                role: MembershipRole::Member,
            }),
            budget: None,
        }];

        store
            .seed_from_inputs(&[], &[], &[], &updated_teams, &updated_users)
            .await
            .expect("updated seed");
        store
            .seed_from_inputs(&[], &[], &[], &updated_teams, &updated_users)
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
        assert!(
            store
                .get_active_budget_for_team(refreshed_platform.team_id)
                .await
                .expect("platform budget after reseed")
                .is_none()
        );
        assert_eq!(
            store
                .get_active_budget_for_team(ops_team.team_id)
                .await
                .expect("ops budget")
                .expect("ops budget exists")
                .amount_usd,
            Money4::from_scaled(125_000)
        );
        assert!(
            store
                .get_active_budget_for_user(user.user_id)
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
            budget: Some(SeedBudget {
                cadence: BudgetCadence::Monthly,
                amount_usd: Money4::from_scaled(500_000),
                hard_limit: true,
                timezone: "UTC".to_string(),
            }),
        }];
        let initial_users = vec![SeedUser {
            name: "Member".to_string(),
            email: "member@example.com".to_string(),
            email_normalized: "member@example.com".to_string(),
            global_role: GlobalRole::User,
            auth_mode: AuthMode::Password,
            request_logging_enabled: false,
            oidc_provider_key: None,
            membership: None,
            budget: None,
        }];

        store
            .seed_from_inputs(&[], &[], &[], &initial_teams, &initial_users)
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
            membership: None,
            budget: None,
        }];
        let invalid_teams = vec![SeedTeam {
            team_key: "platform".to_string(),
            team_name: "Platform Renamed".to_string(),
            budget: None,
        }];

        let error = store
            .seed_from_inputs(&[], &[], &[], &invalid_teams, &invalid_users)
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
        assert_eq!(
            store
                .get_active_budget_for_team(platform_team.team_id)
                .await
                .expect("team budget")
                .expect("team budget exists")
                .amount_usd,
            Money4::from_scaled(500_000)
        );

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
            membership: None,
            budget: None,
        }];

        store
            .seed_from_inputs(&[], &[], &[], &[], &initial_users)
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
            membership: None,
            budget: None,
        }];

        let error = store
            .seed_from_inputs(&[], &[], &[], &[], &invalid_users)
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
            membership: None,
            budget: None,
        }];

        store
            .seed_from_inputs(&[], &[], &[], &[], &initial_users)
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
            membership: None,
            budget: None,
        }];

        let error = store
            .seed_from_inputs(&[], &[], &[], &[], &invalid_users)
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
            .seed_from_inputs(&providers, &models, &api_keys, &[], &[])
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
                UserStatus::Active,
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
            }],
        }];
        let api_keys = vec![SeedApiKey {
            name: "dev".to_string(),
            public_id: "dev123".to_string(),
            secret_hash: "hash".to_string(),
            allowed_models: vec!["fast".to_string()],
        }];

        store
            .seed_from_inputs(&providers, &models, &api_keys, &[], &[])
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
            request_tags: RequestTags::default(),
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
            }],
        }];
        let api_keys = vec![SeedApiKey {
            name: "dev".to_string(),
            public_id: "dev123".to_string(),
            secret_hash: "hash".to_string(),
            allowed_models: vec!["fast".to_string()],
        }];
        store
            .seed_from_inputs(&providers, &models, &api_keys, &[], &[])
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
                UserStatus::Active,
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
            .seed_from_inputs(&providers, &models, &api_keys, &[], &[])
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
