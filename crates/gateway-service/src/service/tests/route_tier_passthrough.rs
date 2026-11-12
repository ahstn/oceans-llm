//! Stored routes that still select a service tier through raw passthrough fail startup.

use serde_json::Value;

use super::*;

/// A selectable route of `model_id` with the given raw passthrough.
fn passthrough_route(model_id: Uuid, extra_body: Value, extra_headers: Value) -> ModelRoute {
    let mut route = vertex_embedding_route(model_id);
    route.extra_body = serde_json::from_value(extra_body).expect("body object");
    route.extra_headers = serde_json::from_value(extra_headers).expect("headers object");
    route
}

fn service_with(
    model_id: Uuid,
    routes: Vec<ModelRoute>,
) -> GatewayService<UsageAccountingRepo, PassThroughPlanner> {
    GatewayService::new(
        Arc::new(UsageAccountingRepo {
            models: vec![model(model_id)],
            routes,
            ..Default::default()
        }),
        Arc::new(PassThroughPlanner),
    )
}

#[tokio::test]
async fn startup_rejects_stored_routes_that_select_a_tier_through_passthrough() {
    for (extra_body, extra_headers, expected) in [
        (
            json!({"service_tier": "priority"}),
            json!({}),
            "extra_body cannot set `service_tier`",
        ),
        (
            json!({"serviceTier": {"type": "flex"}}),
            json!({}),
            "extra_body cannot set `serviceTier`",
        ),
        (
            json!({}),
            json!({"X-Vertex-AI-LLM-Shared-Request-Type": "priority"}),
            "extra_headers cannot set `X-Vertex-AI-LLM-Shared-Request-Type`",
        ),
    ] {
        let model_id = Uuid::new_v4();
        let route = passthrough_route(model_id, extra_body, extra_headers);
        let route_id = route.id.to_string();

        let error = service_with(model_id, vec![route])
            .validate_route_tier_passthrough()
            .await
            .expect_err("startup must reject the stored route");

        let message = error.to_string();
        assert!(
            message.contains(&route_id) && message.contains(expected),
            "unexpected error: {message}"
        );
    }
}

#[tokio::test]
async fn startup_tier_passthrough_check_skips_non_selectable_and_untiered_routes() {
    let model_id = Uuid::new_v4();
    let mut disabled = passthrough_route(model_id, json!({"service_tier": "priority"}), json!({}));
    disabled.enabled = false;
    let mut zero_weight = passthrough_route(
        model_id,
        json!({}),
        json!({"x-amzn-bedrock-service-tier": "flex"}),
    );
    zero_weight.weight = 0.0;
    let untiered = passthrough_route(
        model_id,
        json!({"temperature": 0}),
        json!({"OpenAI-Project": "proj"}),
    );

    service_with(model_id, vec![disabled, zero_weight, untiered])
        .validate_route_tier_passthrough()
        .await
        .expect("disabled, zero-weight and untiered routes cannot block startup");
}
