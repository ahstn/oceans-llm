use std::sync::Arc;

use gateway_core::{
    ExternalMcpToolRecord, GatewayError, McpTokenEstimateConfidence, McpTokenEstimateSource,
    McpTokenOverheadRepository, McpToolTokenEstimateRecord, RequestMcpTokenOverheadRecord,
    mcp_tool_policy::effective_annotations,
};
use serde_json::{Map, json};
use sha2::{Digest, Sha256};
use time::{Duration, OffsetDateTime};

const SERIALIZER_VERSION: &str = "mcp-tool-json-v2";
const DEFAULT_PROTOCOL_VERSION: &str = "2025-11-25";
const ESTIMATE_TTL_DAYS: i64 = 30;

#[derive(Debug, Clone)]
pub struct McpTokenOverheadInput {
    pub request_id: String,
    pub request_log_id: Option<uuid::Uuid>,
    pub model_key: Option<String>,
    pub provider_family: String,
    pub model_or_encoding: String,
    /// Shared, so a caller that caches the tool set passes it without
    /// copying every tool's schema.
    pub tools: Arc<[ExternalMcpToolRecord]>,
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

        let cache_keys = input
            .tools
            .iter()
            .map(|tool| {
                cache_key(
                    &input.provider_family,
                    &input.model_or_encoding,
                    tool,
                    &protocol_version,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        // One query for every tool's cached estimate; only misses are written.
        let cached = self
            .repo
            .get_mcp_tool_token_estimates(&cache_keys, input.occurred_at)
            .await?
            .into_iter()
            .map(|estimate| (estimate.cache_key, estimate.estimated_tokens))
            .collect::<std::collections::HashMap<_, _>>();

        for (tool, cache_key) in input.tools.iter().zip(cache_keys) {
            if let Some(estimated_tokens) = cached.get(&cache_key) {
                definition_tokens += estimated_tokens;
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
    // Overrides change what clients receive without changing the schema hash.
    let overrides =
        serde_json::to_vec(&tool.policy.annotation_overrides.to_object()).map_err(|error| {
            GatewayError::Internal(format!("failed hashing MCP overrides: {error}"))
        })?;
    hasher.update(&overrides);
    hasher.update(protocol_version.as_bytes());
    hasher.update(SERIALIZER_VERSION.as_bytes());
    Ok(format!("{:x}", hasher.finalize()))
}

fn description_hash(description: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(description.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Estimates the definition a client receives from `tools/list`, including
/// the optional title, effective annotations, and output schema.
fn estimate_tool_definition_tokens(tool: &ExternalMcpToolRecord) -> Result<i64, GatewayError> {
    let mut definition = json!({
        "name": tool.upstream_name,
        "description": tool.description,
        "inputSchema": tool.input_schema,
    });
    let metadata = &tool.metadata;
    if let Some(title) = &metadata.title {
        definition["title"] = json!(title);
    }
    if let Some(annotations) = effective_annotations(tool) {
        definition["annotations"] = json!(annotations);
    }
    if let Some(output_schema) = &metadata.output_schema {
        definition["outputSchema"] = output_schema.clone();
    }
    let serialized = serde_json::to_vec(&definition).map_err(|error| {
        GatewayError::Internal(format!("failed estimating MCP tool tokens: {error}"))
    })?;
    Ok(((serialized.len() as i64) + 3) / 4)
}

#[cfg(test)]
mod tests {
    use gateway_core::McpToolMetadata;
    use serde_json::json;
    use time::OffsetDateTime;
    use uuid::Uuid;

    use super::*;

    fn tool(metadata: McpToolMetadata) -> ExternalMcpToolRecord {
        let now = OffsetDateTime::UNIX_EPOCH;
        ExternalMcpToolRecord {
            mcp_tool_id: Uuid::nil(),
            mcp_server_id: Uuid::nil(),
            upstream_name: "search".to_string(),
            display_name: "search".to_string(),
            description: Some("Search the index".to_string()),
            input_schema: json!({"type": "object"}),
            metadata,
            policy: Default::default(),
            schema_hash: "sha256:test".to_string(),
            schema_version: 1,
            is_active: true,
            first_discovered_at: now,
            last_discovered_at: now,
            deactivated_at: None,
            review: Default::default(),
        }
    }

    #[test]
    fn estimate_counts_title_annotations_and_output_schema() {
        let plain = estimate_tool_definition_tokens(&tool(McpToolMetadata::default()))
            .expect("plain estimate");
        let annotations = json!({"readOnlyHint": true})
            .as_object()
            .cloned()
            .expect("annotations");
        let output_schema = json!({
            "type": "object",
            "properties": {"results": {"type": "array", "items": {"type": "string"}}},
        });
        let output_schema_bytes = serde_json::to_vec(&output_schema).expect("bytes").len() as i64;
        let full = estimate_tool_definition_tokens(&tool(McpToolMetadata {
            title: Some("Search".to_string()),
            annotations: Some(annotations),
            output_schema: Some(output_schema),
        }))
        .expect("full estimate");

        assert!(full - plain >= output_schema_bytes / 4);
    }

    /// Counts estimate lookups and keeps what is written.
    #[derive(Default)]
    struct CountingRepo {
        lookups: std::sync::atomic::AtomicUsize,
        estimates: std::sync::Mutex<std::collections::HashMap<String, McpToolTokenEstimateRecord>>,
    }

    #[async_trait::async_trait]
    impl McpTokenOverheadRepository for CountingRepo {
        async fn get_mcp_tool_token_estimates(
            &self,
            cache_keys: &[String],
            _now: OffsetDateTime,
        ) -> Result<Vec<McpToolTokenEstimateRecord>, gateway_core::StoreError> {
            self.lookups
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let estimates = self.estimates.lock().unwrap();
            Ok(cache_keys
                .iter()
                .filter_map(|key| estimates.get(key).cloned())
                .collect())
        }

        async fn upsert_mcp_tool_token_estimate(
            &self,
            estimate: &McpToolTokenEstimateRecord,
        ) -> Result<(), gateway_core::StoreError> {
            self.estimates
                .lock()
                .unwrap()
                .insert(estimate.cache_key.clone(), estimate.clone());
            Ok(())
        }

        async fn upsert_request_mcp_token_overhead(
            &self,
            _overhead: &RequestMcpTokenOverheadRecord,
        ) -> Result<(), gateway_core::StoreError> {
            Ok(())
        }

        async fn get_request_mcp_token_overhead(
            &self,
            _request_id: &str,
        ) -> Result<Option<RequestMcpTokenOverheadRecord>, gateway_core::StoreError> {
            Ok(None)
        }
    }

    #[tokio::test]
    async fn every_tool_estimate_is_read_in_one_query() {
        let repo = Arc::new(CountingRepo::default());
        let overhead = McpTokenOverhead::new(repo.clone());
        let tools = (0..5)
            .map(|index| {
                let mut tool = tool(McpToolMetadata::default());
                tool.mcp_tool_id = Uuid::new_v4();
                tool.upstream_name = format!("tool_{index}");
                tool
            })
            .collect::<Vec<_>>();
        let input = McpTokenOverheadInput {
            request_id: "request-1".to_string(),
            request_log_id: None,
            model_key: None,
            provider_family: "openai".to_string(),
            model_or_encoding: "o200k".to_string(),
            tools: tools.into(),
            context_window_tokens: None,
            protocol_version: None,
            occurred_at: OffsetDateTime::now_utc(),
        };
        let first = overhead
            .record_request_overhead(input.clone())
            .await
            .expect("first");
        assert_eq!(first.cache_miss_count, 5);
        let second = overhead
            .record_request_overhead(input)
            .await
            .expect("second");
        assert_eq!(second.cache_hit_count, 5);
        assert_eq!(
            repo.lookups.load(std::sync::atomic::Ordering::SeqCst),
            2,
            "one lookup per request, not one per tool"
        );
    }

    #[test]
    fn overrides_change_the_estimate_and_its_cache_key() {
        let plain = tool(McpToolMetadata::default());
        let mut overridden = plain.clone();
        overridden.policy.annotation_overrides.read_only = Some(true);
        assert!(
            estimate_tool_definition_tokens(&overridden).expect("estimate")
                > estimate_tool_definition_tokens(&plain).expect("estimate")
        );
        assert_ne!(
            cache_key("openai", "o200k", &plain, "2025-06-18").expect("key"),
            cache_key("openai", "o200k", &overridden, "2025-06-18").expect("key")
        );
    }
}
