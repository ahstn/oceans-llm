use gateway_core::{
    ModelRoutingPolicy, ProviderCapabilities, SeedModel, SeedModelRoute, SeedProvider,
};
use serde_json::{Map, json};
use serial_test::serial;
use tempfile::tempdir;
use uuid::Uuid;

use super::{create_postgres_test_database, drop_postgres_test_database};
use crate::{
    GatewayStore, LibsqlStore, PostgresStore, StoreConnectionOptions, run_migrations,
    run_migrations_with_options, seed::route_uuid,
};

#[test]
fn explicit_route_identity_ignores_order_and_priority_but_tracks_its_target() {
    let route_id = route_uuid("pooled", Some("primary"), "provider", "upstream", 10, 0);
    assert_eq!(
        route_id,
        route_uuid("pooled", Some("primary"), "provider", "upstream", 20, 1)
    );
    for (model, route, provider, upstream) in [
        ("other", "primary", "provider", "upstream"),
        ("pooled", "other", "provider", "upstream"),
        ("pooled", "primary", "other", "upstream"),
        ("pooled", "primary", "provider", "other"),
    ] {
        assert_ne!(
            route_id,
            route_uuid(model, Some(route), provider, upstream, 10, 0)
        );
    }
    assert_ne!(
        route_uuid("pooled", Some("primary"), "a:b", "c", 10, 0),
        route_uuid("pooled", Some("primary"), "a", "b:c", 10, 0)
    );
}

#[test]
fn routes_without_explicit_keys_keep_the_legacy_identity() {
    assert_eq!(
        route_uuid("pooled", None, "provider", "upstream", 10, 0),
        Uuid::new_v5(&Uuid::NAMESPACE_OID, b"route:pooled:provider:upstream:10:0")
    );
}

async fn exercise_route_identity<S: GatewayStore + Sync>(store: &S) {
    let providers: Vec<_> = ["first", "second"]
        .into_iter()
        .map(|key| SeedProvider {
            provider_key: key.to_string(),
            provider_type: "openai_compat".to_string(),
            config: json!({"base_url": "https://example.invalid/v1"}),
            secrets: None,
        })
        .collect();
    let mut model = SeedModel {
        model_key: "pooled".to_string(),
        alias_target_model_key: None,
        max_reasoning_effort: None,
        description: None,
        tags: Vec::new(),
        rank: 10,
        routing: Some(ModelRoutingPolicy::default()),
        routes: ["first", "second"]
            .into_iter()
            .map(|key| SeedModelRoute {
                route_key: Some(format!("{key}-route")),
                provider_key: key.to_string(),
                upstream_model: format!("{key}-model"),
                priority: 10,
                weight: 1.0,
                enabled: true,
                context_window_tokens: None,
                pricing_override: None,
                extra_headers: Map::new(),
                extra_body: Map::new(),
                capabilities: ProviderCapabilities::all_enabled(),
                compatibility: Default::default(),
            })
            .collect(),
        allowlist: None,
    };
    seed_model(store, &providers, &model).await;
    let model_id = store
        .get_model_by_key("pooled")
        .await
        .expect("load model")
        .expect("model")
        .id;
    let original = store.list_routes_for_model(model_id).await.expect("routes");

    model.routes.swap(0, 1);
    model.routes[0].priority = 5;
    model.routes[1].priority = 20;
    seed_model(store, &providers, &model).await;
    let reordered = store.list_routes_for_model(model_id).await.expect("routes");
    assert_eq!(reordered.len(), 2);
    for route in &reordered {
        let prior = original
            .iter()
            .find(|prior| prior.upstream_model == route.upstream_model)
            .expect("original route");
        assert_eq!(route.id, prior.id);
        assert_eq!(
            route.priority,
            if route.provider_key == "second" {
                5
            } else {
                20
            }
        );
    }

    model.routes[0].upstream_model = "replacement-model".to_string();
    model.routes[1].provider_key = "second".to_string();
    seed_model(store, &providers, &model).await;
    let replaced = store.list_routes_for_model(model_id).await.expect("routes");
    assert_eq!(replaced.len(), 2);
    assert!(
        replaced
            .iter()
            .all(|route| original.iter().all(|prior| prior.id != route.id))
    );
}

async fn seed_model<S: GatewayStore + Sync>(
    store: &S,
    providers: &[SeedProvider],
    model: &SeedModel,
) {
    store
        .seed_from_inputs(
            providers,
            std::slice::from_ref(model),
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
        )
        .await
        .expect("seed model routes");
}

#[tokio::test]
#[serial]
async fn libsql_reseed_keeps_route_ids_after_order_and_priority_changes() {
    let tmp = tempdir().expect("tempdir");
    let db_path = tmp.path().join("gateway.db");
    run_migrations(&db_path).await.expect("migrations");
    let store = LibsqlStore::new_local(db_path.to_str().expect("db path"))
        .await
        .expect("store");
    exercise_route_identity(&store).await;
}

#[tokio::test]
#[serial]
async fn postgres_reseed_keeps_route_ids_after_order_and_priority_changes() {
    let Some(test_db) = create_postgres_test_database().await else {
        eprintln!("skipping postgres route identity test: TEST_POSTGRES_URL is not set");
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
    exercise_route_identity(&store).await;
    drop(store);
    drop_postgres_test_database(&test_db).await;
}
