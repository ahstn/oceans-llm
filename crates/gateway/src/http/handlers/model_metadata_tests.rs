use axum::{extract::State, http::HeaderMap};
use serde_json::json;

use super::{tests::seed_stream_cancellation_test, v1_model_metadata, v1_models};
use crate::http::test_support::app_state;

#[tokio::test]
async fn model_metadata_requires_authentication_and_matches_visible_models() {
    let (_directory, state) = app_state().await;
    seed_stream_cancellation_test(&state.store).await;
    assert!(
        v1_model_metadata(State(state.clone()), HeaderMap::new())
            .await
            .is_err()
    );
    let mut headers = HeaderMap::new();
    headers.insert(
        "authorization",
        "Bearer gwk_streamtest.cancel-secret".parse().unwrap(),
    );
    let models = v1_models(State(state.clone()), headers.clone())
        .await
        .map_err(|error| error.0)
        .unwrap()
        .0;
    let metadata = v1_model_metadata(State(state), headers)
        .await
        .map_err(|error| error.0)
        .unwrap()
        .0;
    assert_eq!(metadata.schema_version, 1);
    let provenance = serde_json::to_value(&metadata.supplement).unwrap();
    let provenance = provenance.as_object().unwrap();
    let provenance_fields = [
        "source",
        "provider_id",
        "generated_at",
        "models_dev_sha256",
        "litellm_sha256",
    ];
    assert_eq!(provenance.len(), provenance_fields.len());
    assert!(
        provenance_fields
            .iter()
            .all(|field| provenance.contains_key(*field))
    );
    assert_eq!(
        metadata
            .data
            .iter()
            .map(|model| model.id.as_str())
            .collect::<Vec<_>>(),
        models
            .data
            .iter()
            .map(|model| model.id.as_str())
            .collect::<Vec<_>>()
    );
    assert_eq!(metadata.data[0].enabled_route_count, 1);
    // A similar upstream name or URL cannot supply a missing explicit catalog identity.
    assert!(metadata.data[0].routes[0].pricing.is_none());
    assert_eq!(
        serde_json::to_value(models).unwrap(),
        json!({"object":"list","data":[{
            "id":"fast","object":"model","created":0,"owned_by":"gateway"
        }]})
    );
}
