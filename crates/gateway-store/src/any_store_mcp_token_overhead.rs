use async_trait::async_trait;
use gateway_core::{
    McpTokenOverheadRepository, McpToolTokenEstimateRecord, RequestMcpTokenOverheadRecord,
    StoreError,
};
use time::OffsetDateTime;

use crate::store::AnyStore;

#[async_trait]
impl McpTokenOverheadRepository for AnyStore {
    async fn get_mcp_tool_token_estimate(
        &self,
        cache_key: &str,
        now: OffsetDateTime,
    ) -> Result<Option<McpToolTokenEstimateRecord>, StoreError> {
        match self {
            Self::Libsql(store) => store.get_mcp_tool_token_estimate(cache_key, now).await,
            Self::Postgres(store) => store.get_mcp_tool_token_estimate(cache_key, now).await,
        }
    }

    async fn upsert_mcp_tool_token_estimate(
        &self,
        estimate: &McpToolTokenEstimateRecord,
    ) -> Result<(), StoreError> {
        match self {
            Self::Libsql(store) => store.upsert_mcp_tool_token_estimate(estimate).await,
            Self::Postgres(store) => store.upsert_mcp_tool_token_estimate(estimate).await,
        }
    }

    async fn upsert_request_mcp_token_overhead(
        &self,
        overhead: &RequestMcpTokenOverheadRecord,
    ) -> Result<(), StoreError> {
        match self {
            Self::Libsql(store) => store.upsert_request_mcp_token_overhead(overhead).await,
            Self::Postgres(store) => store.upsert_request_mcp_token_overhead(overhead).await,
        }
    }

    async fn get_request_mcp_token_overhead(
        &self,
        request_id: &str,
    ) -> Result<Option<RequestMcpTokenOverheadRecord>, StoreError> {
        match self {
            Self::Libsql(store) => store.get_request_mcp_token_overhead(request_id).await,
            Self::Postgres(store) => store.get_request_mcp_token_overhead(request_id).await,
        }
    }
}
