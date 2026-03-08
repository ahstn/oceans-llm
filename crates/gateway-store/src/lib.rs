mod libsql_store;
mod migrate;
mod seed;

pub use libsql_store::LibsqlStore;
pub use migrate::run_migrations;

#[cfg(test)]
mod tests {
    use gateway_core::{
        ApiKeyOwnerKind, ApiKeyRepository, AuthMode, GlobalRole, MembershipRole, ModelRepository,
        PricingCatalogCacheRecord, PricingCatalogRepository, SYSTEM_LEGACY_TEAM_ID, SeedApiKey,
        SeedModel, SeedModelRoute, SeedProvider, StoreHealth,
    };
    use serde_json::{Map, json};
    use serial_test::serial;
    use tempfile::tempdir;
    use uuid::Uuid;

    use crate::{LibsqlStore, run_migrations};

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
        assert!(store
            .list_team_memberships(team.team_id)
            .await
            .expect("memberships")
            .is_empty());
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
        let first_team = store.create_team("alpha", "Alpha").await.expect("first team");
        let second_team = store.create_team("beta", "Beta").await.expect("second team");

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
                "SELECT estimated_cost_10000 FROM usage_cost_events WHERE usage_event_id = ?1",
                [usage_event_id.to_string()],
            )
            .await
            .expect("query usage amount");
        let usage_row = usage_rows
            .next()
            .await
            .expect("fetch usage row")
            .expect("usage row");
        let estimated_cost_10000: i64 = usage_row.get(0).expect("decode usage amount");
        assert_eq!(estimated_cost_10000, 6_789);

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
    }
}
