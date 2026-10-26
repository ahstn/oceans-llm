//! `POST /v1/images`, with `/v1/images/generations`, `/images` and
//! `/images/generations` as aliases. Runs the shared non-streaming path in
//! [`super::json_operation`] and records `gateway.images.requests`.
use gateway_core::{ImagesRequest, RequestTags};

use super::json_operation::{JsonOperation, execute_json_operation};
use super::*;
use crate::observability::{GatewayMetrics, OperationRequestMetric};

pub async fn v1_images(
    State(state): State<AppState>,
    request_id: Option<Extension<RequestId>>,
    headers: HeaderMap,
    policy: RequestPolicyContext,
    InferenceAuth(auth): InferenceAuth,
    Json(request): Json<ImagesRequest>,
) -> Result<Response, AppError> {
    // Rejected before model resolution, like a body that fails to parse: no
    // request metric sample, request log or rate-limit slot.
    if request.stream {
        return Err(AppError(GatewayError::InvalidRequest(
            "image generation does not support streaming; omit `stream` or set it to false"
                .to_string(),
        )));
    }
    execute_json_operation(state, request_id, headers, policy, auth, request).await
}

impl JsonOperation for ImagesRequest {
    const OPERATION: &'static str = "images";
    const ROUTE_PATH: &'static str = "/v1/images";
    const ENDPOINT: RoutingEndpoint = RoutingEndpoint::Images;

    fn model(&self) -> &str {
        &self.model
    }

    fn route_requirements(&self) -> CoreRequestRequirements {
        self.requirements()
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
        service.begin_images_request_log(
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
        provider.images(self, context).await
    }

    /// The Images API response has no `model` field, so none is added. If an
    /// upstream reports one anyway, it names the gateway model instead, so the
    /// upstream model name never reaches the caller.
    fn caller_response(mut upstream: Value, requested_model_key: &str) -> Value {
        if let Some(model) = upstream.get_mut("model") {
            *model = Value::String(requested_model_key.to_string());
        }
        upstream
    }

    fn record_request_metric(metrics: &GatewayMetrics, metric: &OperationRequestMetric<'_>) {
        metrics.record_images_request(metric);
    }
}
