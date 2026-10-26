//! `POST /v1/embeddings`. Runs the shared non-streaming path in
//! [`super::json_operation`] and records `gateway.embeddings.requests`.
use gateway_core::{EmbeddingsRequest, RequestTags, openai_embeddings_request_to_core};

use super::json_operation::{JsonOperation, execute_json_operation};
use super::*;
use crate::observability::{GatewayMetrics, OperationRequestMetric};

pub async fn v1_embeddings(
    State(state): State<AppState>,
    request_id: Option<Extension<RequestId>>,
    headers: HeaderMap,
    policy: RequestPolicyContext,
    InferenceAuth(auth): InferenceAuth,
    Json(request): Json<EmbeddingsRequest>,
) -> Result<Response, AppError> {
    execute_json_operation(state, request_id, headers, policy, auth, request).await
}

impl JsonOperation for EmbeddingsRequest {
    const OPERATION: &'static str = "embeddings";
    const ROUTE_PATH: &'static str = "/v1/embeddings";
    const ENDPOINT: RoutingEndpoint = RoutingEndpoint::Embeddings;

    fn model(&self) -> &str {
        &self.model
    }

    fn route_requirements(&self) -> CoreRequestRequirements {
        // What the core request's `requirements` returns, without copying the
        // input into a core request.
        CoreRequestRequirements {
            embeddings: true,
            ..CoreRequestRequirements::default()
        }
    }

    fn extra(&self) -> &BTreeMap<String, Value> {
        &self.extra
    }

    fn begin_request_log(
        &self,
        service: &AppGatewayService,
        request_id: &str,
        resolved: &ResolvedGatewayRequest,
        request_headers: &BTreeMap<String, String>,
        request_tags: RequestTags,
    ) -> RequestLogContext {
        service.begin_embeddings_request_log(
            request_id,
            &resolved.selection.requested_model.model_key,
            &resolved.selection.execution_model.model_key,
            self,
            request_headers,
            request_tags,
        )
    }

    async fn call_provider(
        &self,
        provider: &dyn ProviderClient,
        context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        // Translated after the guardrails, so redacted input reaches the provider.
        provider
            .embeddings(&openai_embeddings_request_to_core(self), context)
            .await
    }

    fn caller_response(upstream: Value, requested_model_key: &str) -> Value {
        normalize_response_model(upstream, requested_model_key)
    }

    fn record_request_metric(metrics: &GatewayMetrics, metric: &OperationRequestMetric<'_>) {
        metrics.record_embeddings_request(metric);
    }
}
