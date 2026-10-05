use gateway_core::{
    ModelRoutingPolicy, RoutingStrategy, SeedApiKey, SeedModel, SessionAffinityPolicy,
};
use serial_test::serial;
use tempfile::tempdir;

use super::{
    create_postgres_test_database, drop_postgres_test_database, seed_api_key_service_accounts,
    seed_api_key_teams,
};
use crate::{
    GatewayStore, LibsqlStore, PostgresStore, StoreConnectionOptions, run_migrations,
    run_migrations_with_options,
};

async fn exercise_model_routing_policy<S: GatewayStore + Sync>(store: &S) {
    let mut model = SeedModel {
        model_key: "pooled".to_string(),
        alias_target_model_key: None,
        max_reasoning_effort: None,
        description: None,
        tags: Vec::new(),
        rank: 10,
        routing: None,
        routes: Vec::new(),
        allowlist: None,
    };
    let api_keys = [SeedApiKey {
        name: "routing test".to_string(),
        public_id: "routing-test".to_string(),
        secret_hash: "unused-test-hash".to_string(),
        service_account_key: "seed-workloads".to_string(),
        allowed_models: vec![model.model_key.clone()],
    }];
    // Repeated seeds cover existing defaults, policy replacement, and policy removal.
    for routing in [
        None,
        Some(ModelRoutingPolicy {
            strategy: RoutingStrategy::Preferred,
            affinity: Some(SessionAffinityPolicy::default()),
            failover: None,
        }),
        Some(ModelRoutingPolicy {
            strategy: RoutingStrategy::RoundRobin,
            affinity: Some(SessionAffinityPolicy {
                idle_timeout_seconds: 900,
            }),
            failover: Some(gateway_core::ProviderFailoverPolicy {
                max_retries_per_route: 1,
                ..Default::default()
            }),
        }),
        None,
    ] {
        model.routing = routing.clone();
        store
            .seed_from_inputs(
                &[],
                std::slice::from_ref(&model),
                &api_keys,
                &seed_api_key_service_accounts(),
                &[],
                &[],
                &seed_api_key_teams(),
                &[],
            )
            .await
            .expect("seed routing policy");
        let key = store
            .get_api_key_by_public_id("routing-test")
            .await
            .expect("load API key")
            .expect("API key");
        let by_key = store
            .get_model_by_key(&model.model_key)
            .await
            .expect("load model")
            .expect("model");
        assert_eq!(by_key.routing, routing);

        let model_keys = [model.model_key.clone()];
        let listed = store.list_models().await.expect("list models");
        let by_keys = store
            .list_models_by_keys(&model_keys)
            .await
            .expect("load models by keys");
        let granted = store
            .list_models_for_api_key(key.id)
            .await
            .expect("load granted models");
        let mut grants = store
            .list_models_for_api_keys(&[key.id])
            .await
            .expect("load model grants");
        let batch_granted = grants.remove(&key.id).expect("key grants");
        for models in [listed, by_keys, granted, batch_granted] {
            assert_eq!(models.len(), 1);
            assert_eq!(models[0].routing, routing);
        }
    }
}

#[tokio::test]
#[serial]
async fn libsql_model_routing_policy_round_trips_and_clears() {
    let tmp = tempdir().expect("tempdir");
    let db_path = tmp.path().join("gateway.db");
    run_migrations(&db_path).await.expect("migrations");
    let store = LibsqlStore::new_local(db_path.to_str().expect("db path"))
        .await
        .expect("store");
    exercise_model_routing_policy(&store).await;
}

#[tokio::test]
#[serial]
async fn postgres_model_routing_policy_round_trips_and_clears() {
    let Some(test_db) = create_postgres_test_database().await else {
        eprintln!("skipping postgres routing policy test: TEST_POSTGRES_URL is not set");
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
    exercise_model_routing_policy(&store).await;
    drop(store);
    drop_postgres_test_database(&test_db).await;
}
