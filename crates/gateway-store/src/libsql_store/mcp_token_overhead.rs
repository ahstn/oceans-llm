use super::*;
use crate::shared::{json_object_from_str, parse_uuid, serialize_json, unix_to_datetime};

fn decode_estimate(row: &libsql::Row) -> Result<McpToolTokenEstimateRecord, StoreError> {
    let mcp_server_id: String = row.get(3).map_err(to_query_error)?;
    let mcp_tool_id: String = row.get(4).map_err(to_query_error)?;
    let estimator_source: String = row.get(11).map_err(to_query_error)?;
    let confidence: String = row.get(12).map_err(to_query_error)?;
    Ok(McpToolTokenEstimateRecord {
        cache_key: row.get(0).map_err(to_query_error)?,
        provider_family: row.get(1).map_err(to_query_error)?,
        model_or_encoding: row.get(2).map_err(to_query_error)?,
        mcp_server_id: parse_uuid(&mcp_server_id)?,
        mcp_tool_id: parse_uuid(&mcp_tool_id)?,
        tool_name: row.get(5).map_err(to_query_error)?,
        schema_hash: row.get(6).map_err(to_query_error)?,
        description_hash: row.get(7).map_err(to_query_error)?,
        protocol_version: row.get(8).map_err(to_query_error)?,
        serializer_version: row.get(9).map_err(to_query_error)?,
        estimated_tokens: row.get(10).map_err(to_query_error)?,
        estimator_source: McpTokenEstimateSource::from_db(&estimator_source).ok_or_else(|| {
            StoreError::Serialization(format!("invalid MCP estimate source `{estimator_source}`"))
        })?,
        confidence: McpTokenEstimateConfidence::from_db(&confidence).ok_or_else(|| {
            StoreError::Serialization(format!("invalid MCP estimate confidence `{confidence}`"))
        })?,
        created_at: unix_to_datetime(row.get(13).map_err(to_query_error)?)?,
        updated_at: unix_to_datetime(row.get(14).map_err(to_query_error)?)?,
        expires_at: unix_to_datetime(row.get(15).map_err(to_query_error)?)?,
    })
}

fn decode_overhead(row: &libsql::Row) -> Result<RequestMcpTokenOverheadRecord, StoreError> {
    let request_log_id: Option<String> = row.get(1).map_err(to_query_error)?;
    let estimator_source: String = row.get(8).map_err(to_query_error)?;
    let confidence: String = row.get(9).map_err(to_query_error)?;
    let metadata_json: String = row.get(14).map_err(to_query_error)?;
    Ok(RequestMcpTokenOverheadRecord {
        request_id: row.get(0).map_err(to_query_error)?,
        request_log_id: request_log_id.as_deref().map(parse_uuid).transpose()?,
        model_key: row.get(2).map_err(to_query_error)?,
        provider_family: row.get(3).map_err(to_query_error)?,
        model_or_encoding: row.get(4).map_err(to_query_error)?,
        exposed_tool_count: row.get(5).map_err(to_query_error)?,
        estimated_definition_tokens: row.get(6).map_err(to_query_error)?,
        estimated_result_tokens: row.get(7).map_err(to_query_error)?,
        estimator_source: McpTokenEstimateSource::from_db(&estimator_source).ok_or_else(|| {
            StoreError::Serialization(format!("invalid MCP estimate source `{estimator_source}`"))
        })?,
        confidence: McpTokenEstimateConfidence::from_db(&confidence).ok_or_else(|| {
            StoreError::Serialization(format!("invalid MCP estimate confidence `{confidence}`"))
        })?,
        cache_hit_count: row.get(10).map_err(to_query_error)?,
        cache_miss_count: row.get(11).map_err(to_query_error)?,
        context_window_tokens: row.get(12).map_err(to_query_error)?,
        context_window_percent_bps: row.get(13).map_err(to_query_error)?,
        metadata: json_object_from_str(&metadata_json)?,
        created_at: unix_to_datetime(row.get(15).map_err(to_query_error)?)?,
        updated_at: unix_to_datetime(row.get(16).map_err(to_query_error)?)?,
    })
}

const ESTIMATE_COLUMNS: &str = "cache_key, provider_family, model_or_encoding, mcp_server_id, mcp_tool_id, tool_name, schema_hash, description_hash, protocol_version, serializer_version, estimated_tokens, estimator_source, confidence, created_at, updated_at, expires_at";
const OVERHEAD_COLUMNS: &str = "request_id, request_log_id, model_key, provider_family, model_or_encoding, exposed_tool_count, estimated_definition_tokens, estimated_result_tokens, estimator_source, confidence, cache_hit_count, cache_miss_count, context_window_tokens, context_window_percent_bps, metadata_json, created_at, updated_at";

#[async_trait]
impl McpTokenOverheadRepository for LibsqlStore {
    async fn get_mcp_tool_token_estimate(
        &self,
        cache_key: &str,
        now: OffsetDateTime,
    ) -> Result<Option<McpToolTokenEstimateRecord>, StoreError> {
        let sql = format!(
            "SELECT {ESTIMATE_COLUMNS} FROM mcp_tool_token_estimates WHERE cache_key = ?1 AND expires_at > ?2"
        );
        let mut rows = self
            .connection
            .query(&sql, libsql::params![cache_key, now.unix_timestamp()])
            .await
            .map_err(to_query_error)?;
        rows.next()
            .await
            .map_err(to_query_error)?
            .map(|row| decode_estimate(&row))
            .transpose()
    }

    async fn upsert_mcp_tool_token_estimate(
        &self,
        estimate: &McpToolTokenEstimateRecord,
    ) -> Result<(), StoreError> {
        self.connection
            .execute(
                r#"
                INSERT INTO mcp_tool_token_estimates (
                    cache_key, provider_family, model_or_encoding, mcp_server_id, mcp_tool_id,
                    tool_name, schema_hash, description_hash, protocol_version, serializer_version,
                    estimated_tokens, estimator_source, confidence, created_at, updated_at, expires_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
                ON CONFLICT(cache_key) DO UPDATE SET
                    estimated_tokens = excluded.estimated_tokens,
                    estimator_source = excluded.estimator_source,
                    confidence = excluded.confidence,
                    updated_at = excluded.updated_at,
                    expires_at = excluded.expires_at
                "#,
                libsql::params![
                    estimate.cache_key.as_str(),
                    estimate.provider_family.as_str(),
                    estimate.model_or_encoding.as_str(),
                    estimate.mcp_server_id.to_string(),
                    estimate.mcp_tool_id.to_string(),
                    estimate.tool_name.as_str(),
                    estimate.schema_hash.as_str(),
                    estimate.description_hash.as_str(),
                    estimate.protocol_version.as_str(),
                    estimate.serializer_version.as_str(),
                    estimate.estimated_tokens,
                    estimate.estimator_source.as_str(),
                    estimate.confidence.as_str(),
                    estimate.created_at.unix_timestamp(),
                    estimate.updated_at.unix_timestamp(),
                    estimate.expires_at.unix_timestamp(),
                ],
            )
            .await
            .map_err(to_write_error)?;
        Ok(())
    }

    async fn upsert_request_mcp_token_overhead(
        &self,
        overhead: &RequestMcpTokenOverheadRecord,
    ) -> Result<(), StoreError> {
        let metadata_json = serialize_json(&overhead.metadata)?;
        self.connection
            .execute(
                r#"
                INSERT INTO request_mcp_token_overheads (
                    request_id, request_log_id, model_key, provider_family, model_or_encoding,
                    exposed_tool_count, estimated_definition_tokens, estimated_result_tokens,
                    estimator_source, confidence, cache_hit_count, cache_miss_count,
                    context_window_tokens, context_window_percent_bps, metadata_json, created_at, updated_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
                ON CONFLICT(request_id) DO UPDATE SET
                    request_log_id = excluded.request_log_id,
                    model_key = excluded.model_key,
                    provider_family = excluded.provider_family,
                    model_or_encoding = excluded.model_or_encoding,
                    exposed_tool_count = excluded.exposed_tool_count,
                    estimated_definition_tokens = excluded.estimated_definition_tokens,
                    estimated_result_tokens = excluded.estimated_result_tokens,
                    estimator_source = excluded.estimator_source,
                    confidence = excluded.confidence,
                    cache_hit_count = excluded.cache_hit_count,
                    cache_miss_count = excluded.cache_miss_count,
                    context_window_tokens = excluded.context_window_tokens,
                    context_window_percent_bps = excluded.context_window_percent_bps,
                    metadata_json = excluded.metadata_json,
                    updated_at = excluded.updated_at
                "#,
                libsql::params![
                    overhead.request_id.as_str(),
                    overhead.request_log_id.map(|value| value.to_string()),
                    overhead.model_key.as_deref(),
                    overhead.provider_family.as_str(),
                    overhead.model_or_encoding.as_str(),
                    overhead.exposed_tool_count,
                    overhead.estimated_definition_tokens,
                    overhead.estimated_result_tokens,
                    overhead.estimator_source.as_str(),
                    overhead.confidence.as_str(),
                    overhead.cache_hit_count,
                    overhead.cache_miss_count,
                    overhead.context_window_tokens,
                    overhead.context_window_percent_bps,
                    metadata_json,
                    overhead.created_at.unix_timestamp(),
                    overhead.updated_at.unix_timestamp(),
                ],
            )
            .await
            .map_err(to_write_error)?;
        Ok(())
    }

    async fn get_request_mcp_token_overhead(
        &self,
        request_id: &str,
    ) -> Result<Option<RequestMcpTokenOverheadRecord>, StoreError> {
        let sql = format!(
            "SELECT {OVERHEAD_COLUMNS} FROM request_mcp_token_overheads WHERE request_id = ?1"
        );
        let mut rows = self
            .connection
            .query(&sql, [request_id])
            .await
            .map_err(to_query_error)?;
        rows.next()
            .await
            .map_err(to_query_error)?
            .map(|row| decode_overhead(&row))
            .transpose()
    }
}
