use std::sync::Arc;

use gateway_core::{
    ExternalMcpToolRecord, GatewayError, McpTokenEstimateConfidence, McpTokenEstimateSource,
    McpTokenOverheadRepository, McpToolTokenEstimateRecord, RequestMcpTokenOverheadRecord,
};
use serde_json::{Map, json};
use sha2::{Digest, Sha256};
use time::{Duration, OffsetDateTime};

const SERIALIZER_VERSION: &str = "mcp-tool-json-v1";
const DEFAULT_PROTOCOL_VERSION: &str = "2025-11-25";
const ESTIMATE_TTL_DAYS: i64 = 30;

#[derive(Debug, Clone)]
pub struct McpTokenOverheadInput {
    pub request_id: String,
    pub request_log_id: Option<uuid::Uuid>,
    pub model_key: Option<String>,
    pub provider_family: String,
    pub model_or_encoding: String,
    pub tools: Vec<ExternalMcpToolRecord>,
    pub context_window_tokens: Option<i64>,
    pub protocol_version: Option<String>,
    pub occurred_at: OffsetDateTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct McpTokenOverheadSummary {
    pub estimated_definition_tokens: i64,
    pub cache_hit_count: i64,
    pub cache_miss_count: i64,
}

#[derive(Clone)]
pub struct McpTokenOverhead<R> {
    repo: Arc<R>,
}

impl<R> McpTokenOverhead<R>
where
    R: McpTokenOverheadRepository,
{
    #[must_use]
    pub fn new(repo: Arc<R>) -> Self {
        Self { repo }
    }

    pub async fn record_request_overhead(
        &self,
        input: McpTokenOverheadInput,
    ) -> Result<McpTokenOverheadSummary, GatewayError> {
        let protocol_version = input
            .protocol_version
            .unwrap_or_else(|| DEFAULT_PROTOCOL_VERSION.to_string());
        let mut definition_tokens = 0_i64;
        let mut cache_hit_count = 0_i64;
        let mut cache_miss_count = 0_i64;

        for tool in &input.tools {
            let cache_key = cache_key(
                &input.provider_family,
                &input.model_or_encoding,
                tool,
                &protocol_version,
            )?;
            if let Some(estimate) = self
                .repo
                .get_mcp_tool_token_estimate(&cache_key, input.occurred_at)
                .await?
            {
                definition_tokens += estimate.estimated_tokens;
                cache_hit_count += 1;
                continue;
            }

            let estimated_tokens = estimate_tool_definition_tokens(tool)?;
            let estimate = McpToolTokenEstimateRecord {
                cache_key,
                provider_family: input.provider_family.clone(),
                model_or_encoding: input.model_or_encoding.clone(),
                mcp_server_id: tool.mcp_server_id,
                mcp_tool_id: tool.mcp_tool_id,
                tool_name: tool.upstream_name.clone(),
                schema_hash: tool.schema_hash.clone(),
                description_hash: description_hash(tool.description.as_deref().unwrap_or("")),
                protocol_version: protocol_version.clone(),
                serializer_version: SERIALIZER_VERSION.to_string(),
                estimated_tokens,
                estimator_source: McpTokenEstimateSource::ConservativeFallback,
                confidence: McpTokenEstimateConfidence::Low,
                created_at: input.occurred_at,
                updated_at: input.occurred_at,
                expires_at: input.occurred_at + Duration::days(ESTIMATE_TTL_DAYS),
            };
            self.repo.upsert_mcp_tool_token_estimate(&estimate).await?;
            definition_tokens += estimated_tokens;
            cache_miss_count += 1;
        }

        let context_window_percent_bps = input.context_window_tokens.and_then(|window| {
            if window > 0 {
                Some(definition_tokens.saturating_mul(10_000) / window)
            } else {
                None
            }
        });
        let mut metadata = Map::new();
        metadata.insert(
            "billing_note".to_string(),
            json!(
                "MCP token overhead estimates are context-window telemetry, not spend accounting."
            ),
        );

        self.repo
            .upsert_request_mcp_token_overhead(&RequestMcpTokenOverheadRecord {
                request_id: input.request_id,
                request_log_id: input.request_log_id,
                model_key: input.model_key,
                provider_family: input.provider_family,
                model_or_encoding: input.model_or_encoding,
                exposed_tool_count: input.tools.len() as i64,
                estimated_definition_tokens: definition_tokens,
                estimated_result_tokens: None,
                estimator_source: McpTokenEstimateSource::ConservativeFallback,
                confidence: McpTokenEstimateConfidence::Low,
                cache_hit_count,
                cache_miss_count,
                context_window_tokens: input.context_window_tokens,
                context_window_percent_bps,
                metadata,
                created_at: input.occurred_at,
                updated_at: input.occurred_at,
            })
            .await?;

        Ok(McpTokenOverheadSummary {
            estimated_definition_tokens: definition_tokens,
            cache_hit_count,
            cache_miss_count,
        })
    }
}

fn cache_key(
    provider_family: &str,
    model_or_encoding: &str,
    tool: &ExternalMcpToolRecord,
    protocol_version: &str,
) -> Result<String, GatewayError> {
    let mut hasher = Sha256::new();
    hasher.update(provider_family.as_bytes());
    hasher.update(b"\0");
    hasher.update(model_or_encoding.as_bytes());
    hasher.update(b"\0");
    hasher.update(tool.mcp_server_id.as_bytes());
    hasher.update(tool.mcp_tool_id.as_bytes());
    hasher.update(tool.upstream_name.as_bytes());
    hasher.update(tool.schema_hash.as_bytes());
    hasher.update(description_hash(tool.description.as_deref().unwrap_or("")).as_bytes());
    hasher.update(protocol_version.as_bytes());
    hasher.update(SERIALIZER_VERSION.as_bytes());
    Ok(format!("{:x}", hasher.finalize()))
}

fn description_hash(description: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(description.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn estimate_tool_definition_tokens(tool: &ExternalMcpToolRecord) -> Result<i64, GatewayError> {
    let serialized = serde_json::to_vec(&json!({
        "name": tool.upstream_name,
        "description": tool.description,
        "inputSchema": tool.input_schema,
    }))
    .map_err(|error| {
        GatewayError::Internal(format!("failed estimating MCP tool tokens: {error}"))
    })?;
    Ok(((serialized.len() as i64) + 3) / 4)
}
