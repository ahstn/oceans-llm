use crate::tests::{create_postgres_test_database, drop_postgres_test_database};
use crate::{
    GatewayStore, LibsqlStore, PostgresStore, StoreConnectionOptions, run_migrations,
    run_migrations_with_options,
};
use gateway_core::{
    AuthMode, BudgetCadence, BudgetModelSelector, BudgetScope, BudgetSettings, BudgetSource,
    BudgetSourceKind, GlobalRole, Money4, SeedBudget, SeedHumanBudgetDefaults, SeedModel, SeedUser,
    SeedUserModelBudgetDefault, UserStatus,
};
use serial_test::serial;
use tempfile::tempdir;
use time::{Duration, OffsetDateTime};

#[tokio::test]
#[serial]
async fn libsql_human_budget_defaults_inherit_until_manual_override_or_deactivation() {
    let tmp = tempdir().expect("tempdir");
    let db_path = tmp.path().join("gateway.db");
    run_migrations(&db_path).await.expect("migrations");
    let store = LibsqlStore::new_local(db_path.to_str().expect("db path"))
        .await
        .expect("store");
    exercise_human_budget_defaults(&store).await;
    exercise_batch_source_guards(&store).await;
}

#[tokio::test]
#[serial]
async fn postgres_human_budget_defaults_inherit_until_manual_override_or_deactivation() {
    let Some(test_db) = create_postgres_test_database().await else {
        eprintln!("skipping postgres budget defaults test: TEST_POSTGRES_URL is not set");
        return;
    };
    run_migrations_with_options(&StoreConnectionOptions::Postgres {
        url: test_db.database_url.clone(),
        max_connections: 4,
    })
    .await
    .expect("migrations");
    let store = PostgresStore::connect(&test_db.database_url, 4)
        .await
        .expect("store");
    exercise_human_budget_defaults(&store).await;
    exercise_batch_source_guards(&store).await;
    drop(store);
    drop_postgres_test_database(&test_db).await;
}

async fn exercise_human_budget_defaults<S: GatewayStore + ?Sized>(store: &S) {
    let users = vec![SeedUser {
        name: "Member".to_string(),
        email: "member@example.com".to_string(),
        email_normalized: "member@example.com".to_string(),
        global_role: GlobalRole::User,
        auth_mode: AuthMode::Password,
        request_logging_enabled: true,
        tags: None,
        oidc_provider_key: None,
        oauth_provider_key: None,
        membership: None,
        budget: None,
    }];
    let models = vec![SeedModel {
        model_key: "fable-5".to_string(),
        alias_target_model_key: None,
        max_reasoning_effort: None,
        description: None,
        tags: Vec::new(),
        rank: 10,
        routes: Vec::new(),
        allowlist: None,
    }];

    store
        .seed_from_inputs(&[], &models, &[], &[], &[], &[], &[], &users)
        .await
        .expect("seed user");
    let bootstrap_user = store
        .upsert_bootstrap_admin_user("Admin", "admin@local", true)
        .await
        .expect("bootstrap admin");
    let user = store
        .get_user_by_email_normalized("member@example.com")
        .await
        .expect("load user")
        .expect("user exists");
    let model_id = crate::seed::model_uuid("fable-5");
    let defaults = SeedHumanBudgetDefaults {
        default_user_budget: Some(SeedBudget {
            cadence: BudgetCadence::Daily,
            amount_usd: Money4::from_scaled(700_000),
            hard_limit: true,
            timezone: "UTC".to_string(),
        }),
        model_defaults: vec![SeedUserModelBudgetDefault {
            model_key: "fable-5".to_string(),
            model_id,
            budget: SeedBudget {
                cadence: BudgetCadence::Daily,
                amount_usd: Money4::from_scaled(400_000),
                hard_limit: true,
                timezone: "UTC".to_string(),
            },
        }],
    };
    let now = OffsetDateTime::now_utc();

    store
        .reconcile_human_budget_defaults(&defaults, now)
        .await
        .expect("apply defaults");

    let user_scope = BudgetScope::User {
        user_id: user.user_id,
    };
    let inherited_user_budget = store
        .get_active_budget_by_scope(&user_scope)
        .await
        .expect("load inherited user budget")
        .expect("inherited user budget exists");
    assert_eq!(
        inherited_user_budget.source.kind,
        BudgetSourceKind::ConfigUserDefault
    );
    assert_eq!(
        inherited_user_budget.settings.amount_usd,
        Money4::from_scaled(700_000)
    );
    let bootstrap_scope = BudgetScope::User {
        user_id: bootstrap_user.user_id,
    };
    let bootstrap_budget = store
        .get_active_budget_by_scope(&bootstrap_scope)
        .await
        .expect("load bootstrap budget")
        .expect("bootstrap budget exists");
    assert_eq!(
        bootstrap_budget.source.kind,
        BudgetSourceKind::ConfigUserDefault
    );
    assert_eq!(
        bootstrap_budget.settings.amount_usd,
        Money4::from_scaled(700_000)
    );

    let model_scope = BudgetScope::UserModel {
        user_id: user.user_id,
        selector: BudgetModelSelector::Model { model_id },
    };
    let inherited_model_budget = store
        .get_active_budget_by_scope(&model_scope)
        .await
        .expect("load inherited model budget")
        .expect("inherited model budget exists");
    assert_eq!(
        inherited_model_budget.source.kind,
        BudgetSourceKind::ConfigUserModelDefault
    );
    assert_eq!(
        inherited_model_budget.settings.amount_usd,
        Money4::from_scaled(400_000)
    );

    let defaults_without_model = SeedHumanBudgetDefaults {
        default_user_budget: defaults.default_user_budget.clone(),
        model_defaults: Vec::new(),
    };
    store
        .reconcile_human_budget_defaults(&defaults_without_model, now + Duration::seconds(1))
        .await
        .expect("remove model default");
    assert!(
        store
            .get_active_budget_by_scope(&model_scope)
            .await
            .expect("load model budget after default removal")
            .is_none()
    );
    assert_eq!(
        store
            .get_latest_budget_by_scope(&model_scope)
            .await
            .expect("latest model budget")
            .expect("inactive model budget remains")
            .source
            .kind,
        BudgetSourceKind::ConfigUserModelDefault
    );
    store
        .reconcile_human_budget_defaults(&defaults, now + Duration::seconds(2))
        .await
        .expect("restore model default");
    let restored_model_budget = store
        .get_active_budget_by_scope(&model_scope)
        .await
        .expect("load restored model budget")
        .expect("restored model budget exists");
    assert_eq!(
        restored_model_budget.source.kind,
        BudgetSourceKind::ConfigUserModelDefault
    );
    assert_eq!(
        restored_model_budget.settings.amount_usd,
        Money4::from_scaled(400_000)
    );

    let override_users = vec![SeedUser {
        name: "Override".to_string(),
        email: "override@example.com".to_string(),
        email_normalized: "override@example.com".to_string(),
        global_role: GlobalRole::User,
        auth_mode: AuthMode::Password,
        request_logging_enabled: true,
        tags: None,
        oidc_provider_key: None,
        oauth_provider_key: None,
        membership: None,
        budget: Some(SeedBudget {
            cadence: BudgetCadence::Daily,
            amount_usd: Money4::from_scaled(250_000),
            hard_limit: true,
            timezone: "UTC".to_string(),
        }),
    }];
    store
        .seed_from_inputs(&[], &[], &[], &[], &[], &[], &[], &override_users)
        .await
        .expect("seed explicit user override");
    store
        .reconcile_human_budget_defaults(&defaults, now + Duration::seconds(3))
        .await
        .expect("reconcile defaults with explicit override");
    let override_user = store
        .get_user_by_email_normalized("override@example.com")
        .await
        .expect("load override user")
        .expect("override user exists");
    let override_scope = BudgetScope::User {
        user_id: override_user.user_id,
    };
    let config_override_budget = store
        .get_active_budget_by_scope(&override_scope)
        .await
        .expect("load config override budget")
        .expect("config override budget exists");
    assert_eq!(
        config_override_budget.source.kind,
        BudgetSourceKind::ConfigUserOverride
    );
    assert_eq!(
        config_override_budget.settings.amount_usd,
        Money4::from_scaled(250_000)
    );

    let inherited_users = vec![SeedUser {
        budget: None,
        ..override_users[0].clone()
    }];
    store
        .seed_from_inputs_with_user_budget_default(
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &inherited_users,
            defaults.default_user_budget.as_ref(),
        )
        .await
        .expect("remove explicit user override");
    // Stop before the next startup phase: the replacement must already be
    // active, and the same row must have remained active throughout the swap.
    let before_reconciliation = store
        .get_active_budget_by_scope(&override_scope)
        .await
        .expect("read between startup phases")
        .expect("no enforcement gap");
    assert_eq!(
        before_reconciliation.budget_id,
        config_override_budget.budget_id
    );
    assert_eq!(
        before_reconciliation.source.kind,
        BudgetSourceKind::ConfigUserDefault
    );
    assert_eq!(
        before_reconciliation.settings.amount_usd,
        Money4::from_scaled(700_000)
    );
    // Force a database failure in the next startup phase. The already-swapped
    // budget must remain active when reconciliation cannot complete.
    let invalid_defaults = SeedHumanBudgetDefaults {
        default_user_budget: Some(SeedBudget {
            amount_usd: Money4::from_scaled(-1),
            ..defaults
                .default_user_budget
                .as_ref()
                .expect("default")
                .clone()
        }),
        model_defaults: Vec::new(),
    };
    store
        .reconcile_human_budget_defaults(&invalid_defaults, now + Duration::seconds(4))
        .await
        .expect_err("injected constraint failure");
    let after_failure = store
        .get_active_budget_by_scope(&override_scope)
        .await
        .expect("read after failure")
        .expect("default remains active");
    assert_eq!(after_failure, before_reconciliation);
    store
        .reconcile_human_budget_defaults(&defaults, now + Duration::seconds(4))
        .await
        .expect("reconcile defaults after override removal");
    let inherited_override_budget = store
        .get_active_budget_by_scope(&override_scope)
        .await
        .expect("load inherited budget after override removal")
        .expect("inherited budget exists");
    assert_eq!(
        inherited_override_budget.source.kind,
        BudgetSourceKind::ConfigUserDefault
    );
    assert_eq!(
        inherited_override_budget.settings.amount_usd,
        Money4::from_scaled(700_000)
    );

    store
        .upsert_active_budget(
            &user_scope,
            &BudgetSettings {
                cadence: BudgetCadence::Daily,
                amount_usd: Money4::from_scaled(250_000),
                hard_limit: true,
                timezone: "UTC".to_string(),
            },
            now + Duration::seconds(5),
        )
        .await
        .expect("manual user override");
    store
        .deactivate_active_budget(&model_scope, now + Duration::seconds(5))
        .await
        .expect("manual model deactivation");
    store
        .reconcile_human_budget_defaults(&defaults, now + Duration::seconds(6))
        .await
        .expect("reapply defaults");

    let manual_user_budget = store
        .get_active_budget_by_scope(&user_scope)
        .await
        .expect("load manual user budget")
        .expect("manual user budget remains");
    assert_eq!(manual_user_budget.source.kind, BudgetSourceKind::Manual);
    assert_eq!(
        manual_user_budget.settings.amount_usd,
        Money4::from_scaled(250_000)
    );
    assert!(
        store
            .get_active_budget_by_scope(&model_scope)
            .await
            .expect("load model budget after deactivation")
            .is_none()
    );
}

async fn exercise_batch_source_guards<S: GatewayStore + ?Sized>(store: &S) {
    let now = OffsetDateTime::now_utc();
    let settings = BudgetSettings {
        cadence: BudgetCadence::Daily,
        amount_usd: Money4::from_scaled(100),
        hard_limit: true,
        timezone: "UTC".into(),
    };
    let source = BudgetSource::config_user_default();
    let mut scopes = Vec::new();
    for index in 0..4 {
        let email = format!("batch-guard-{index}@example.com");
        let user = store
            .create_identity_user(
                "Batch",
                &email,
                &email,
                GlobalRole::User,
                AuthMode::Password,
                UserStatus::Active,
            )
            .await
            .expect("create user");
        scopes.push(BudgetScope::User {
            user_id: user.user_id,
        });
    }
    let initial = scopes[..3]
        .iter()
        .map(|scope| gateway_core::BudgetUpsert {
            scope: scope.clone(),
            settings: &settings,
            source: &source,
            expected_current_source: None,
        })
        .collect::<Vec<_>>();
    store
        .upsert_active_budgets_with_source_guard(&initial, now)
        .await
        .expect("initial batch");
    let keys = scopes
        .iter()
        .map(BudgetScope::scope_key)
        .collect::<Vec<_>>();
    let before = store
        .get_budget_states_by_scope_keys(&keys)
        .await
        .expect("read batch");
    assert_eq!(before.len(), 3);
    store
        .upsert_active_budget(&scopes[0], &settings, now + Duration::seconds(1))
        .await
        .expect("admin edit");
    store
        .deactivate_active_budget(&scopes[1], now + Duration::seconds(1))
        .await
        .expect("admin deactivate");
    let changed = BudgetSettings {
        amount_usd: Money4::from_scaled(200),
        ..settings.clone()
    };
    let stale_writes = scopes
        .iter()
        .enumerate()
        .map(|(index, scope)| gateway_core::BudgetUpsert {
            scope: scope.clone(),
            settings: &changed,
            source: &source,
            expected_current_source: (index < 3).then_some(&source),
        })
        .collect::<Vec<_>>();
    store
        .upsert_active_budgets_with_source_guard(&stale_writes, now + Duration::seconds(2))
        .await
        .expect("stale batch");
    let states = store
        .get_budget_states_by_scope_keys(&keys)
        .await
        .expect("states");
    assert_eq!(states.len(), 4);
    for (index, key) in keys.iter().enumerate() {
        let row = states
            .iter()
            .find(|row| &row.scope_key == key)
            .expect("scope state");
        match index {
            0 => {
                assert_eq!(row.source.kind, BudgetSourceKind::Manual);
                assert_eq!(row.settings.amount_usd, settings.amount_usd);
            }
            1 => {
                assert!(!row.is_active);
                assert!(row.source.is_manual_deactivation());
            }
            _ => {
                assert!(row.is_active);
                assert_eq!(row.settings.amount_usd, changed.amount_usd);
            }
        }
    }
    // An absent-row plan must also respect a sentinel created since the read.
    store
        .upsert_active_budgets_with_source_guard(
            &[gateway_core::BudgetUpsert {
                scope: scopes[1].clone(),
                settings: &changed,
                source: &source,
                expected_current_source: None,
            }],
            now + Duration::seconds(3),
        )
        .await
        .expect("sentinel guard");
    assert!(
        store
            .get_active_budget_by_scope(&scopes[1])
            .await
            .expect("active")
            .is_none()
    );
    // Stale cleanup may remove config rows, but must leave the admin-owned row.
    store
        .deactivate_budgets_by_source(
            &before.iter().collect::<Vec<_>>(),
            now + Duration::seconds(4),
        )
        .await
        .expect("stale cleanup");
    assert_eq!(
        store
            .get_active_budget_by_scope(&scopes[0])
            .await
            .expect("active")
            .expect("manual retained")
            .source
            .kind,
        BudgetSourceKind::Manual
    );
    assert!(
        store
            .get_active_budget_by_scope(&scopes[2])
            .await
            .expect("active")
            .is_none()
    );
    assert!(
        store
            .get_active_budget_by_scope(&scopes[3])
            .await
            .expect("active")
            .is_some()
    );
}
