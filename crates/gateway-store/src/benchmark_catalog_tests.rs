use gateway_core::{
    BenchmarkCatalogRepository, BenchmarkSyncState, ModelBenchmarkBinding, ModelBenchmarkScore,
    ModelRepository, SeedModel,
};
use tempfile::tempdir;
use time::{Duration, OffsetDateTime};

use crate::{LibsqlStore, run_migrations};

#[tokio::test]
async fn libsql_replaces_current_benchmark_scores_atomically() {
    let directory = tempdir().expect("database directory");
    let database_path = directory.path().join("gateway.db");
    run_migrations(&database_path).await.expect("migrations");
    let store = LibsqlStore::new_local(database_path.to_str().expect("database path"))
        .await
        .expect("store");
    store
        .seed_from_inputs(&[], &[seed_model()], &[], &[], &[], &[], &[], &[])
        .await
        .expect("seed model");
    let model_id = store.list_models().await.expect("models")[0].id;
    let binding = ModelBenchmarkBinding {
        model_id,
        source: "artificial_analysis".to_string(),
        source_model_id: "aa-model-1".to_string(),
    };
    store
        .replace_model_benchmark_bindings("artificial_analysis", std::slice::from_ref(&binding))
        .await
        .expect("bindings");

    let fetched_at = OffsetDateTime::UNIX_EPOCH + Duration::hours(2);
    let state = sync_state(fetched_at, "4.3");
    assert!(
        store
            .replace_model_benchmark_scores(&[score(&binding, fetched_at, "4.3")], &state)
            .await
            .expect("replace scores")
    );
    let scores = store
        .list_model_benchmark_scores()
        .await
        .expect("list scores");
    assert_eq!(scores.len(), 1);
    assert_eq!(scores[0].value, 39.0);

    let older_state = sync_state(fetched_at - Duration::hours(1), "4.2");
    assert!(
        !store
            .replace_model_benchmark_scores(&[], &older_state)
            .await
            .expect("reject stale replacement")
    );
    assert_eq!(
        store
            .list_model_benchmark_scores()
            .await
            .expect("scores after stale replacement"),
        scores
    );

    let changed_binding = ModelBenchmarkBinding {
        source_model_id: "aa-model-2".to_string(),
        ..binding
    };
    store
        .replace_model_benchmark_bindings("artificial_analysis", &[changed_binding])
        .await
        .expect("replace binding");
    assert!(
        store
            .list_model_benchmark_scores()
            .await
            .expect("scores after binding change")
            .is_empty()
    );
}

fn seed_model() -> SeedModel {
    SeedModel {
        model_key: "test-model".to_string(),
        alias_target_model_key: None,
        max_reasoning_effort: None,
        description: None,
        tags: Vec::new(),
        rank: 100,
        routes: Vec::new(),
        allowlist: None,
    }
}

fn score(
    binding: &ModelBenchmarkBinding,
    fetched_at: OffsetDateTime,
    version: &str,
) -> ModelBenchmarkScore {
    ModelBenchmarkScore {
        model_id: binding.model_id,
        metric_key: "artificial_analysis_intelligence_index".to_string(),
        label: "Artificial Analysis Intelligence Index".to_string(),
        value: 39.0,
        unit: "index_points".to_string(),
        benchmark_version: version.to_string(),
        source: binding.source.clone(),
        source_model_id: binding.source_model_id.clone(),
        source_url: "https://artificialanalysis.ai/models/test-model".to_string(),
        fetched_at,
    }
}

fn sync_state(fetched_at: OffsetDateTime, version: &str) -> BenchmarkSyncState {
    BenchmarkSyncState {
        source: "artificial_analysis".to_string(),
        benchmark_version: version.to_string(),
        last_successful_refresh_at: fetched_at,
        updated_at: fetched_at,
    }
}
