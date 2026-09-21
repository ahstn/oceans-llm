use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use async_trait::async_trait;
use axum::{
    Json, Router,
    extract::Query,
    http::{HeaderMap, StatusCode},
    routing::get,
};
use gateway_core::{
    BenchmarkCatalogRepository, BenchmarkSyncState, ModelBenchmarkBinding, ModelBenchmarkScore,
    StoreError,
};
use serde::Deserialize;
use serde_json::json;
use time::OffsetDateTime;
use uuid::Uuid;

use super::{ARTIFICIAL_ANALYSIS_SOURCE, BenchmarkCatalog, INTELLIGENCE_INDEX_METRIC_KEY};

#[derive(Default)]
struct InMemoryRepo {
    bindings: Vec<ModelBenchmarkBinding>,
    snapshot: Mutex<StoredSnapshot>,
}

#[derive(Clone, Default)]
struct StoredSnapshot {
    scores: Vec<ModelBenchmarkScore>,
    state: Option<BenchmarkSyncState>,
}

#[async_trait]
impl BenchmarkCatalogRepository for InMemoryRepo {
    async fn replace_model_benchmark_bindings(
        &self,
        _source: &str,
        _bindings: &[ModelBenchmarkBinding],
    ) -> Result<(), StoreError> {
        Ok(())
    }

    async fn list_model_benchmark_bindings(
        &self,
        source: &str,
    ) -> Result<Vec<ModelBenchmarkBinding>, StoreError> {
        Ok(self
            .bindings
            .iter()
            .filter(|binding| binding.source == source)
            .cloned()
            .collect())
    }

    async fn list_model_benchmark_scores(&self) -> Result<Vec<ModelBenchmarkScore>, StoreError> {
        Ok(self.snapshot.lock().expect("snapshot lock").scores.clone())
    }

    async fn get_benchmark_sync_state(
        &self,
        source: &str,
    ) -> Result<Option<BenchmarkSyncState>, StoreError> {
        Ok(self
            .snapshot
            .lock()
            .expect("snapshot lock")
            .state
            .clone()
            .filter(|state| state.source == source))
    }

    async fn replace_model_benchmark_scores(
        &self,
        scores: &[ModelBenchmarkScore],
        state: &BenchmarkSyncState,
    ) -> Result<bool, StoreError> {
        let mut snapshot = self.snapshot.lock().expect("snapshot lock");
        if snapshot.state.as_ref().is_some_and(|current| {
            current.last_successful_refresh_at >= state.last_successful_refresh_at
        }) {
            return Ok(false);
        }
        snapshot.scores = scores.to_vec();
        snapshot.state = Some(state.clone());
        Ok(true)
    }
}

#[derive(Deserialize)]
struct PageQuery {
    page: u32,
}

#[tokio::test]
async fn skips_remote_fetch_without_model_bindings() {
    let repo = Arc::new(InMemoryRepo::default());

    BenchmarkCatalog::with_options(
        repo,
        "test-key".to_string(),
        "http://127.0.0.1:1/models".to_string(),
        Duration::ZERO,
    )
    .refresh_now()
    .await
    .expect("empty catalogue needs no remote request");
}

#[tokio::test]
async fn fetches_all_pages_and_maps_only_explicit_stable_ids() {
    let bound_model_id = Uuid::new_v4();
    let repo = Arc::new(InMemoryRepo {
        bindings: vec![binding(bound_model_id, "aa-bound")],
        ..Default::default()
    });
    let app = Router::new().route(
        "/models",
        get(
            |headers: HeaderMap, Query(query): Query<PageQuery>| async move {
                assert_eq!(
                    headers
                        .get("x-api-key")
                        .and_then(|value| value.to_str().ok()),
                    Some("test-key")
                );
                let (has_more, data) = if query.page == 1 {
                    (
                        true,
                        vec![json!({
                            "id": "aa-unbound",
                            "slug": "unbound",
                            "evaluations": {"artificial_analysis_intelligence_index": 99.0}
                        })],
                    )
                } else {
                    (
                        false,
                        vec![json!({
                            "id": "aa-bound",
                            "slug": "bound-model",
                            "evaluations": {"artificial_analysis_intelligence_index": 39.0}
                        })],
                    )
                };
                Json(json!({
                    "tier": "free",
                    "intelligence_index_version": 4.3,
                    "pagination": {
                        "page": query.page,
                        "page_size": 1,
                        "total_pages": 2,
                        "has_more": has_more
                    },
                    "data": data
                }))
            },
        ),
    );
    let (source_url, server) = serve(app).await;

    BenchmarkCatalog::with_options(
        repo.clone(),
        "test-key".to_string(),
        source_url,
        Duration::ZERO,
    )
    .refresh_now()
    .await
    .expect("refresh benchmark scores");

    let snapshot = repo.snapshot.lock().expect("snapshot lock").clone();
    assert_eq!(snapshot.scores.len(), 1);
    let score = &snapshot.scores[0];
    assert_eq!(score.model_id, bound_model_id);
    assert_eq!(score.metric_key, INTELLIGENCE_INDEX_METRIC_KEY);
    assert_eq!(score.value, 39.0);
    assert_eq!(score.unit, "index_points");
    assert_eq!(score.benchmark_version, "4.3");
    assert_eq!(score.source_model_id, "aa-bound");
    assert_eq!(
        score.source_url,
        "https://artificialanalysis.ai/models/bound-model"
    );
    server.abort();
}

#[tokio::test]
async fn failed_refresh_keeps_the_last_successful_snapshot() {
    let model_id = Uuid::new_v4();
    let previous = score(model_id, 38.0, "4.2", OffsetDateTime::UNIX_EPOCH);
    let repo = Arc::new(InMemoryRepo {
        bindings: vec![binding(model_id, "aa-bound")],
        snapshot: Mutex::new(StoredSnapshot {
            scores: vec![previous.clone()],
            state: Some(sync_state("4.2", OffsetDateTime::UNIX_EPOCH)),
        }),
    });
    let app = Router::new().route(
        "/models",
        get(|| async { (StatusCode::BAD_GATEWAY, "upstream unavailable") }),
    );
    let (source_url, server) = serve(app).await;

    let error = BenchmarkCatalog::with_options(
        repo.clone(),
        "test-key".to_string(),
        source_url,
        Duration::ZERO,
    )
    .refresh_now()
    .await
    .expect_err("refresh should fail");

    assert!(error.to_string().contains("HTTP 502"));
    assert_eq!(
        repo.snapshot.lock().expect("snapshot lock").scores,
        vec![previous]
    );
    server.abort();
}

#[tokio::test]
async fn successful_refresh_removes_a_score_that_is_now_null() {
    let model_id = Uuid::new_v4();
    let previous = score(model_id, 38.0, "4.2", OffsetDateTime::UNIX_EPOCH);
    let repo = Arc::new(InMemoryRepo {
        bindings: vec![binding(model_id, "aa-bound")],
        snapshot: Mutex::new(StoredSnapshot {
            scores: vec![previous],
            state: Some(sync_state("4.2", OffsetDateTime::UNIX_EPOCH)),
        }),
    });
    let app = Router::new().route(
        "/models",
        get(|| async {
            Json(json!({
                "tier": "free",
                "intelligence_index_version": 4.3,
                "pagination": {"page": 1, "page_size": 200, "total_pages": 1, "has_more": false},
                "data": [{
                    "id": "aa-bound",
                    "slug": "bound-model",
                    "evaluations": {"artificial_analysis_intelligence_index": null}
                }]
            }))
        }),
    );
    let (source_url, server) = serve(app).await;

    BenchmarkCatalog::with_options(
        repo.clone(),
        "test-key".to_string(),
        source_url,
        Duration::ZERO,
    )
    .refresh_now()
    .await
    .expect("refresh benchmark scores");

    assert!(
        repo.snapshot
            .lock()
            .expect("snapshot lock")
            .scores
            .is_empty()
    );
    server.abort();
}

fn binding(model_id: Uuid, source_model_id: &str) -> ModelBenchmarkBinding {
    ModelBenchmarkBinding {
        model_id,
        source: ARTIFICIAL_ANALYSIS_SOURCE.to_string(),
        source_model_id: source_model_id.to_string(),
    }
}

fn score(
    model_id: Uuid,
    value: f64,
    benchmark_version: &str,
    fetched_at: OffsetDateTime,
) -> ModelBenchmarkScore {
    ModelBenchmarkScore {
        model_id,
        metric_key: INTELLIGENCE_INDEX_METRIC_KEY.to_string(),
        label: "Artificial Analysis Intelligence Index".to_string(),
        value,
        unit: "index_points".to_string(),
        benchmark_version: benchmark_version.to_string(),
        source: ARTIFICIAL_ANALYSIS_SOURCE.to_string(),
        source_model_id: "aa-bound".to_string(),
        source_url: "https://artificialanalysis.ai/models/bound-model".to_string(),
        fetched_at,
    }
}

fn sync_state(benchmark_version: &str, fetched_at: OffsetDateTime) -> BenchmarkSyncState {
    BenchmarkSyncState {
        source: ARTIFICIAL_ANALYSIS_SOURCE.to_string(),
        benchmark_version: benchmark_version.to_string(),
        last_successful_refresh_at: fetched_at,
        updated_at: fetched_at,
    }
}

async fn serve(app: Router) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock server");
    let address = listener.local_addr().expect("mock server address");
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve mock API");
    });
    (format!("http://{address}/models"), server)
}
