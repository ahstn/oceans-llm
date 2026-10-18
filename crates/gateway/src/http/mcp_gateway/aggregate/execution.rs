use std::{time::Duration, time::Instant};

use axum::{
    body::Body,
    http::{Response, StatusCode},
};
use gateway_core::{
    AuthenticatedApiKey, GatewayError, McpCatalogToolRecord, McpToolInvocationStatus,
    McpToolPolicyResult, ProviderError,
};
use gateway_guardrails::EvaluationPayload;
use gateway_mcp::server::{
    JSON_RPC_INVALID_PARAMS, call_tool_error_result, json_rpc_error, json_rpc_success,
};
use gateway_mcp::{
    JsonRpcErrorObject, JsonRpcId, McpClientError, StreamableHttpClient, ToolsCallResponse,
};
use gateway_service::{CallMcpToolInput, McpCatalog, McpInvocationLogInput};
use serde_json::{Map, Value, json};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::http::{
    mcp_gateway::{
        credentials::credential_guidance,
        governance::{GovernedCall, GuardedArguments, Refusal, Refused},
        rate_limits::{JSON_RPC_INTERNAL_ERROR, rejection_body},
    },
    request_policy::RequestPolicyContext,
    state::AppState,
};

use super::{
    evaluate_mcp_result, guardrail_decision_metadata, json_rpc_response, serialization_error,
};

pub(super) async fn call_catalog_tool(
    state: &AppState,
    policy: &RequestPolicyContext,
    auth: &AuthenticatedApiKey,
    id: JsonRpcId,
    input: CallMcpToolInput,
) -> Response<Body> {
    let started_at = Instant::now();
    let catalog = McpCatalog::new(state.store.clone());
    let record = match catalog
        .authorized_tool_by_address(auth, &input.address)
        .await
    {
        Ok(record) => record,
        Err(GatewayError::InvalidRequest(message)) => {
            return json_rpc_response(
                StatusCode::BAD_REQUEST,
                json_rpc_error(Some(id), JSON_RPC_INVALID_PARAMS, message),
                None,
            );
        }
        Err(error) => {
            return aggregate_tool_error(
                id,
                error.to_string(),
                error.error_code(),
                json!({"address": input.address}),
            );
        }
    };
    if let Some(schema_hash) = input.schema_hash.as_deref()
        && schema_hash != record.tool.schema_hash
    {
        return aggregate_tool_error(
            id,
            "Tool schema changed",
            "tool_schema_changed",
            json!({
                "address": input.address,
                "expected_schema_hash": schema_hash,
                "actual_schema_hash": record.tool.schema_hash,
                "schema_version": record.tool.schema_version
            }),
        );
    }

    let mut invocation = CatalogInvocation {
        state,
        policy,
        auth,
        record: &record,
        mcp_tool_invocation_id: Uuid::new_v4(),
        request_id: match &id {
            JsonRpcId::Number(value) => value.to_string(),
            JsonRpcId::String(value) => value.clone(),
        },
        arguments: input.arguments,
        result_json: None,
        metadata: Map::new(),
        started_at,
    };
    let outcome = invocation.call().await;
    invocation.finish(id, &input.address, outcome).await
}

enum CatalogCallError {
    Gateway(GatewayError),
    /// A governance step refused the call; `Refusal::log` gives its
    /// invocation-log fields, as on the per-server route.
    Refused(Refusal),
    /// A result guardrail denied the call.
    ResultDenied,
    InvalidTransformation(serde_json::Error),
}

struct CatalogInvocation<'a> {
    state: &'a AppState,
    policy: &'a RequestPolicyContext,
    auth: &'a AuthenticatedApiKey,
    record: &'a McpCatalogToolRecord,
    mcp_tool_invocation_id: Uuid,
    request_id: String,
    arguments: Value,
    result_json: Option<Value>,
    metadata: Map<String, Value>,
    started_at: Instant,
}

impl CatalogInvocation<'_> {
    async fn call(&mut self) -> Result<ToolsCallResponse, CatalogCallError> {
        let original_arguments = self.arguments.take();
        let arguments = if original_arguments.is_null() {
            json!({})
        } else {
            original_arguments.clone()
        };
        let record = self.record;
        let governed = GovernedCall::new(
            self.state,
            self.policy,
            self.auth,
            &record.server,
            self.request_id.clone(),
        );
        self.mcp_tool_invocation_id = governed.invocation_id;
        let admitted = match governed
            .admit(Some((&record.tool.upstream_name, arguments)))
            .await
        {
            Ok(admitted) => admitted,
            Err(refused) => {
                let Refused { refusal, arguments } = *refused;
                return Err(self.refused(refusal, arguments, original_arguments));
            }
        };
        let guarded = admitted
            .arguments
            .expect("tools/call runs argument guardrails");
        self.metadata.insert(
            "guardrail_call".to_string(),
            Value::Object(guardrail_decision_metadata(&guarded.evaluation)),
        );
        self.arguments = guarded.value;
        let upstream = admitted.upstream;
        let client = StreamableHttpClient::with_http_client(
            &upstream.server.server_url,
            self.state
                .mcp_http_clients
                .for_server(&upstream.server)
                .map_err(CatalogCallError::Gateway)?
                .clone(),
            Duration::from_millis(upstream.server.timeout_ms.max(1) as u64),
        )
        .map_err(|error| CatalogCallError::Gateway(map_mcp_client_error(error)))?
        .with_query_credential(upstream.query_credential.clone());
        match client
            .call_tool(
                upstream.headers.as_ref(),
                &self.record.tool.upstream_name,
                self.arguments.clone(),
            )
            .await
        {
            Ok(result) => self.evaluate_result(result).await,
            Err(McpClientError::Http { status, body }) => {
                let body = match self.evaluate_error_payload(Value::String(body)).await? {
                    Value::String(body) => body,
                    result => result.to_string(),
                };
                Err(CatalogCallError::Gateway(
                    ProviderError::UpstreamHttp { status, body }.into(),
                ))
            }
            Err(McpClientError::JsonRpc(error)) => {
                let payload = self.evaluate_error_payload(json!(error)).await?;
                let error: JsonRpcErrorObject = serde_json::from_value(payload)
                    .map_err(CatalogCallError::InvalidTransformation)?;
                Err(CatalogCallError::Gateway(map_mcp_client_error(
                    McpClientError::JsonRpc(error),
                )))
            }
            Err(error) => Err(CatalogCallError::Gateway(map_mcp_client_error(error))),
        }
    }

    /// Records why the governance steps refused the call. A call refused for
    /// want of a credential logs the arguments as sent, since no guarded
    /// arguments were ever used.
    fn refused(
        &mut self,
        refusal: Refusal,
        guarded: Option<GuardedArguments>,
        original_arguments: Value,
    ) -> CatalogCallError {
        let sent_none = original_arguments.is_null();
        self.arguments = if sent_none {
            json!({})
        } else {
            original_arguments
        };
        if let Some(guarded) = &guarded {
            self.metadata.insert(
                "guardrail_call".to_string(),
                Value::Object(guardrail_decision_metadata(&guarded.evaluation)),
            );
        }
        match &refusal {
            Refusal::Credential(
                GatewayError::McpCredentialRequired { .. }
                | GatewayError::McpCredentialExpired { .. },
            ) => {
                if sent_none {
                    self.arguments = Value::Null;
                }
            }
            Refusal::Credential(_) => {
                if let Some(guarded) = guarded {
                    self.arguments = guarded.value;
                }
            }
            _ => {}
        }
        CatalogCallError::Refused(refusal)
    }

    async fn evaluate_error_payload(&mut self, payload: Value) -> Result<Value, CatalogCallError> {
        let evaluation = evaluate_mcp_result(
            self.state,
            Some(&self.request_id),
            Some(self.mcp_tool_invocation_id),
            &self.record.server.server_key,
            &self.record.tool.upstream_name,
            payload,
        )
        .await;
        self.metadata.insert(
            "guardrail_result".to_string(),
            Value::Object(guardrail_decision_metadata(&evaluation)),
        );
        if evaluation.denied() {
            return Err(CatalogCallError::ResultDenied);
        }
        match evaluation.output {
            EvaluationPayload::McpResult { result, .. } => Ok(result),
            _ => unreachable!("MCP result evaluation preserves payload kind"),
        }
    }

    async fn evaluate_result(
        &mut self,
        result: ToolsCallResponse,
    ) -> Result<ToolsCallResponse, CatalogCallError> {
        self.result_json = serde_json::to_value(&result).ok();
        let evaluation = evaluate_mcp_result(
            self.state,
            Some(&self.request_id),
            Some(self.mcp_tool_invocation_id),
            &self.record.server.server_key,
            &self.record.tool.upstream_name,
            self.result_json.clone().unwrap_or(Value::Null),
        )
        .await;
        self.metadata.insert(
            "guardrail_result".to_string(),
            Value::Object(guardrail_decision_metadata(&evaluation)),
        );
        if evaluation.denied() {
            return Err(CatalogCallError::ResultDenied);
        }
        if !evaluation
            .decisions
            .iter()
            .any(|decision| decision.transformed)
        {
            return Ok(result);
        }
        let transformed = match evaluation.output {
            EvaluationPayload::McpResult { result, .. } => result,
            _ => unreachable!("MCP result evaluation preserves payload kind"),
        };
        self.result_json = Some(transformed.clone());
        serde_json::from_value(transformed).map_err(CatalogCallError::InvalidTransformation)
    }

    /// The aggregate route's response for a gateway error: a tool result
    /// naming the next step for a missing or expired credential.
    fn gateway_error_response(
        &self,
        id: JsonRpcId,
        error: &GatewayError,
        mut error_context: Value,
    ) -> Response<Body> {
        let message = match credential_guidance(self.state, self.auth, &self.record.server, error) {
            Some(guidance) => {
                if let Some(url) = guidance.connect_url {
                    error_context["connect_url"] = json!(url);
                }
                guidance.message
            }
            None => error.to_string(),
        };
        aggregate_tool_error(id, message, error.error_code(), error_context)
    }

    async fn finish(
        mut self,
        id: JsonRpcId,
        address: &str,
        outcome: Result<ToolsCallResponse, CatalogCallError>,
    ) -> Response<Body> {
        let error_context =
            json!({"address": address, "server_key": self.record.server.server_key});
        let (response, status, policy_result, error_code) = match outcome {
            Ok(result) => {
                let status = if result.is_error.unwrap_or(false) {
                    McpToolInvocationStatus::UpstreamError
                } else {
                    McpToolInvocationStatus::Success
                };
                (
                    json_rpc_response(
                        StatusCode::OK,
                        json_rpc_success(id, result).unwrap_or_else(serialization_error),
                        None,
                    ),
                    status,
                    McpToolPolicyResult::Allowed,
                    None,
                )
            }
            Err(CatalogCallError::Gateway(error)) => (
                self.gateway_error_response(id, &error, error_context),
                invocation_status_for_error(&error),
                McpToolPolicyResult::Allowed,
                Some(error.error_code().to_string()),
            ),
            Err(CatalogCallError::Refused(refusal)) => {
                let log = refusal.log();
                // Only the response format is the aggregate route's own.
                let response = match &refusal {
                    Refusal::RateLimited(rejected) => json_rpc_response(
                        StatusCode::TOO_MANY_REQUESTS,
                        rejection_body(serde_json::to_value(&id).unwrap_or(Value::Null), rejected),
                        None,
                    ),
                    // A real HTTP error, like the per-server route's 503, so
                    // clients retry it instead of reading a tool result.
                    Refusal::RateLimitUnavailable(error) => json_rpc_response(
                        StatusCode::from_u16(error.http_status_code())
                            .unwrap_or(StatusCode::SERVICE_UNAVAILABLE),
                        json_rpc_error(Some(id), JSON_RPC_INTERNAL_ERROR, error.to_string()),
                        None,
                    ),
                    Refusal::GuardrailDenied(_) => aggregate_tool_error(
                        id,
                        "MCP operation denied by guardrail policy",
                        log.error_code.as_str(),
                        error_context,
                    ),
                    Refusal::Credential(error) => {
                        self.gateway_error_response(id, error, error_context)
                    }
                };
                // This route files the argument guardrail's decisions under
                // `guardrail_call`.
                if matches!(refusal, Refusal::GuardrailDenied(_)) {
                    self.metadata
                        .insert("guardrail_call".to_string(), Value::Object(log.metadata));
                } else {
                    self.metadata.extend(log.metadata);
                }
                (
                    response,
                    log.status,
                    log.policy_result,
                    Some(log.error_code),
                )
            }
            Err(CatalogCallError::ResultDenied) => {
                let code = "guardrail_policy_denied";
                (
                    aggregate_tool_error(
                        id,
                        "MCP result denied by guardrail policy",
                        code,
                        error_context,
                    ),
                    McpToolInvocationStatus::PolicyDenied,
                    McpToolPolicyResult::Denied,
                    Some(code.to_string()),
                )
            }
            Err(CatalogCallError::InvalidTransformation(error)) => {
                let code = "guardrail_invalid_transformation";
                (
                    aggregate_tool_error(
                        id,
                        format!("managed guardrail produced an invalid MCP result: {error}"),
                        code,
                        error_context,
                    ),
                    McpToolInvocationStatus::GatewayError,
                    McpToolPolicyResult::Allowed,
                    Some(code.to_string()),
                )
            }
        };
        let mut metadata = self.metadata;
        metadata.insert("mcp_route".to_string(), json!("aggregate"));
        metadata.insert("aggregate_tool".to_string(), json!("call_tool"));
        let _ = self
            .state
            .service
            .log_mcp_tool_invocation(
                self.auth,
                McpInvocationLogInput {
                    mcp_tool_invocation_id: Some(self.mcp_tool_invocation_id),
                    request_log_id: None,
                    request_id: self.request_id,
                    server_id: Some(self.record.server.mcp_server_id),
                    server_display_key: self.record.server.server_key.clone(),
                    server_display_name: self.record.server.display_name.clone(),
                    tool_id: Some(self.record.tool.mcp_tool_id),
                    tool_display_key: self.record.tool.upstream_name.clone(),
                    tool_display_name: self.record.tool.display_name.clone(),
                    mcp_method: "tools/call".to_string(),
                    status,
                    policy_result,
                    latency_ms: Some(self.started_at.elapsed().as_millis() as i64),
                    error_code,
                    arguments_json: Some(self.arguments),
                    result_json: self.result_json,
                    metadata,
                    occurred_at: OffsetDateTime::now_utc(),
                    tool_schema_version: Some(self.record.tool.schema_version),
                },
            )
            .await;
        response
    }
}

fn aggregate_tool_error(
    id: JsonRpcId,
    message: impl Into<String>,
    error_code: impl Into<String>,
    structured: Value,
) -> Response<Body> {
    json_rpc_response(
        StatusCode::OK,
        json_rpc_success(id, call_tool_error_result(message, error_code, structured))
            .unwrap_or_else(serialization_error),
        None,
    )
}

fn map_mcp_client_error(error: McpClientError) -> GatewayError {
    match error {
        McpClientError::Timeout => ProviderError::Timeout.into(),
        McpClientError::Http { status, body } => {
            ProviderError::UpstreamHttp { status, body }.into()
        }
        McpClientError::ResponseTooLarge { limit_bytes } => {
            GatewayError::PayloadTooLarge { limit_bytes }
        }
        // Decoder diagnostics can contain upstream values rejected by serde.
        McpClientError::InvalidResponse { .. } => {
            ProviderError::Transport("invalid MCP response".to_string()).into()
        }
        other => ProviderError::Transport(other.to_string()).into(),
    }
}

fn invocation_status_for_error(error: &GatewayError) -> McpToolInvocationStatus {
    match error {
        GatewayError::McpCredentialRequired { .. } | GatewayError::McpCredentialExpired { .. } => {
            McpToolInvocationStatus::Unauthorized
        }
        GatewayError::Provider(ProviderError::Timeout) => McpToolInvocationStatus::Timeout,
        GatewayError::Provider(ProviderError::UpstreamHttp { .. })
        | GatewayError::Provider(ProviderError::Transport(_)) => {
            McpToolInvocationStatus::UpstreamError
        }
        _ => McpToolInvocationStatus::GatewayError,
    }
}
