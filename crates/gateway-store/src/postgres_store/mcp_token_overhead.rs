use super::*;
use crate::shared::{json_object_from_str, parse_uuid, serialize_json, unix_to_datetime};

fn decode_estimate(row: &PgRow) -> Result<McpToolTokenEstimateRecord, StoreError> {
    let estimator_source: String = row.try_get(12).map_err(to_query_error)?;
    let confidence: String = row.try_get(13).map_err(to_query_error)?;
    Ok(McpToolTokenEstimateRecord {
        cache_key: row.try_get(0).map_err(to_query_error)?,
        provider_family: row.try_get(1).map_err(to_query_error)?,
        model_or_encoding: row.try_get(2).map_err(to_query_error)?,
        mcp_server_id: parse_uuid(&row.try_get::<String, _>(3).map_err(to_query_error)?)?,
        mcp_tool_id: parse_uuid(&row.try_get::<String, _>(4).map_err(to_query_error)?)?,
        tool_name: row.try_get(5).map_err(to_query_error)?,
        schema_hash: row.try_get(6).map_err(to_query_error)?,
        description_hash: row.try_get(7).map_err(to_query_error)?,
        protocol_version: row.try_get(8).map_err(to_query_error)?,
        serializer_version: row.try_get(9).map_err(to_query_error)?,
        estimated_tokens: row.try_get(10).map_err(to_query_error)?,
        estimator_source: McpTokenEstimateSource::from_db(&estimator_source).ok_or_else(|| {
            StoreError::Serialization(format!("invalid MCP estimate source `{estimator_source}`"))
        })?,
        confidence: McpTokenEstimateConfidence::from_db(&confidence).ok_or_else(|| {
            StoreError::Serialization(format!("invalid MCP estimate confidence `{confidence}`"))
        })?,
        created_at: unix_to_datetime(row.try_get(14).map_err(to_query_error)?)?,
        updated_at: unix_to_datetime(row.try_get(15).map_err(to_query_error)?)?,
        expires_at: unix_to_datetime(row.try_get(16).map_err(to_query_error)?)?,
    })
}

fn decode_overhead(row: &PgRow) -> Result<RequestMcpTokenOverheadRecord, StoreError> {
    let request_log_id: Option<String> = row.try_get(1).map_err(to_query_error)?;
    let estimator_source: String = row.try_get(8).map_err(to_query_error)?;
    let confidence: String = row.try_get(9).map_err(to_query_error)?;
    let metadata_json: String = row.try_get(14).map_err(to_query_error)?;
    Ok(RequestMcpTokenOverheadRecord {
        request_id: row.try_get(0).map_err(to_query_error)?,
        request_log_id: request_log_id.as_deref().map(parse_uuid).transpose()?,
        model_key: row.try_get(2).map_err(to_query_error)?,
        provider_family: row.try_get(3).map_err(to_query_error)?,
        model_or_encoding: row.try_get(4).map_err(to_query_error)?,
        exposed_tool_count: row.try_get(5).map_err(to_query_error)?,
        estimated_definition_tokens: row.try_get(6).map_err(to_query_error)?,
        estimated_result_tokens: row.try_get(7).map_err(to_query_error)?,
        estimator_source: McpTokenEstimateSource::from_db(&estimator_source).ok_or_else(|| {
            StoreError::Serialization(format!("invalid MCP estimate source `{estimator_source}`"))
        })?,
        confidence: McpTokenEstimateConfidence::from_db(&confidence).ok_or_else(|| {
            StoreError::Serialization(format!("invalid MCP estimate confidence `{confidence}`"))
        })?,
        cache_hit_count: row.try_get(10).map_err(to_query_error)?,
        cache_miss_count: row.try_get(11).map_err(to_query_error)?,
        context_window_tokens: row.try_get(12).map_err(to_query_error)?,
        context_window_percent_bps: row.try_get(13).map_err(to_query_error)?,
        metadata: json_object_from_str(&metadata_json)?,
        created_at: unix_to_datetime(row.try_get(15).map_err(to_query_error)?)?,
        updated_at: unix_to_datetime(row.try_get(16).map_err(to_query_error)?)?,
    })
}

const ESTIMATE_COLUMNS: &str = "cache_key, provider_family, model_or_encoding, mcp_server_id, mcp_tool_id, tool_name, schema_hash, description_hash, protocol_version, serializer_version, estimated_tokens, estimator_source, confidence, created_at, updated_at, expires_at";
const OVERHEAD_COLUMNS: &str = "request_id, request_log_id, model_key, provider_family, model_or_encoding, exposed_tool_count, estimated_definition_tokens, estimated_result_tokens, estimator_source, confidence, cache_hit_count, cache_miss_count, context_window_tokens, context_window_percent_bps, metadata_json::text, created_at, updated_at";

#[async_trait]
impl McpTokenOverheadRepository for PostgresStore {
    async fn get_mcp_tool_token_estimate(
        &self,
        cache_key: &str,
        now: OffsetDateTime,
    ) -> Result<Option<McpToolTokenEstimateRecord>, StoreError> {
        let sql = format!(
            "SELECT {ESTIMATE_COLUMNS} FROM mcp_tool_token_estimates WHERE cache_key = $1 AND expires_at > $2"
        );
        let row = sqlx::query(&sql)
            .bind(cache_key)
            .bind(now.unix_timestamp())
            .fetch_optional(&self.pool)
            .await
            .map_err(to_query_error)?;
        row.as_ref().map(decode_estimate).transpose()
    }

    async fn upsert_mcp_tool_token_estimate(
        &self,
        estimate: &McpToolTokenEstimateRecord,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO mcp_tool_token_estimates (
                cache_key, provider_family, model_or_encoding, mcp_server_id, mcp_tool_id,
                tool_name, schema_hash, description_hash, protocol_version, serializer_version,
                estimated_tokens, estimator_source, confidence, created_at, updated_at, expires_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)
            ON CONFLICT(cache_key) DO UPDATE SET
                estimated_tokens = excluded.estimated_tokens,
                estimator_source = excluded.estimator_source,
                confidence = excluded.confidence,
                updated_at = excluded.updated_at,
                expires_at = excluded.expires_at
            "#,
        )
        .bind(&estimate.cache_key)
        .bind(&estimate.provider_family)
        .bind(&estimate.model_or_encoding)
        .bind(estimate.mcp_server_id.to_string())
        .bind(estimate.mcp_tool_id.to_string())
        .bind(&estimate.tool_name)
        .bind(&estimate.schema_hash)
        .bind(&estimate.description_hash)
        .bind(&estimate.protocol_version)
        .bind(&estimate.serializer_version)
        .bind(estimate.estimated_tokens)
        .bind(estimate.estimator_source.as_str())
        .bind(estimate.confidence.as_str())
        .bind(estimate.created_at.unix_timestamp())
        .bind(estimate.updated_at.unix_timestamp())
        .bind(estimate.expires_at.unix_timestamp())
        .execute(&self.pool)
        .await
        .map_err(to_write_error)?;
        Ok(())
    }

    async fn upsert_request_mcp_token_overhead(
        &self,
        overhead: &RequestMcpTokenOverheadRecord,
    ) -> Result<(), StoreError> {
        let metadata_json = serialize_json(&overhead.metadata)?;
        sqlx::query(
            r#"
            INSERT INTO request_mcp_token_overheads (
                request_id, request_log_id, model_key, provider_family, model_or_encoding,
                exposed_tool_count, estimated_definition_tokens, estimated_result_tokens,
                estimator_source, confidence, cache_hit_count, cache_miss_count,
                context_window_tokens, context_window_percent_bps, metadata_json, created_at, updated_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15::jsonb, $16, $17)
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
        )
        .bind(&overhead.request_id)
        .bind(overhead.request_log_id.map(|value| value.to_string()))
        .bind(&overhead.model_key)
        .bind(&overhead.provider_family)
        .bind(&overhead.model_or_encoding)
        .bind(overhead.exposed_tool_count)
        .bind(overhead.estimated_definition_tokens)
        .bind(overhead.estimated_result_tokens)
        .bind(overhead.estimator_source.as_str())
        .bind(overhead.confidence.as_str())
        .bind(overhead.cache_hit_count)
        .bind(overhead.cache_miss_count)
        .bind(overhead.context_window_tokens)
        .bind(overhead.context_window_percent_bps)
        .bind(metadata_json)
        .bind(overhead.created_at.unix_timestamp())
        .bind(overhead.updated_at.unix_timestamp())
        .execute(&self.pool)
        .await
        .map_err(to_write_error)?;
        Ok(())
    }

    async fn get_request_mcp_token_overhead(
        &self,
        request_id: &str,
    ) -> Result<Option<RequestMcpTokenOverheadRecord>, StoreError> {
        let sql = format!(
            "SELECT {OVERHEAD_COLUMNS} FROM request_mcp_token_overheads WHERE request_id = $1"
        );
        let row = sqlx::query(&sql)
            .bind(request_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(to_query_error)?;
        row.as_ref().map(decode_overhead).transpose()
    }
}
