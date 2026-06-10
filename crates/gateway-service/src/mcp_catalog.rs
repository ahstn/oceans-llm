use std::sync::Arc;

use gateway_core::{
    AuthenticatedApiKey, GatewayError, IdentityRepository, McpAccessRepository,
    McpCatalogToolRecord,
};
use percent_encoding::percent_decode_str;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;
use uuid::Uuid;

use crate::mcp_access::McpAccess;

pub const MCP_CATALOG_RANKER: &str = "lexical_v1";
pub const MCP_TOOL_NOT_GRANTED_MESSAGE: &str = "MCP tool address is not granted";
pub const MCP_TOOL_ADDRESS_SCHEME: &str = "mcp";
pub const DEFAULT_SEARCH_LIMIT: usize = 10;
pub const MAX_SEARCH_LIMIT: usize = 50;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SearchMcpToolsInput {
    #[serde(default)]
    pub query: String,
    #[serde(default = "default_search_limit")]
    pub limit: usize,
    #[serde(default)]
    pub offset: usize,
    #[serde(default)]
    pub server_key: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DescribeMcpToolInput {
    pub address: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CallMcpToolInput {
    pub address: String,
    #[serde(default)]
    pub arguments: Value,
    #[serde(default)]
    pub schema_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchMcpToolsOutput {
    pub items: Vec<McpCatalogSearchItem>,
    pub total: usize,
    pub next_offset: Option<usize>,
    pub ranker: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct McpCatalogSearchItem {
    pub address: String,
    pub score: i64,
    pub server: McpCatalogServerView,
    pub tool: McpCatalogToolSummary,
}

#[derive(Debug, Clone, Serialize)]
pub struct DescribeMcpToolOutput {
    pub address: String,
    pub server: McpCatalogServerView,
    pub tool: McpCatalogToolDescription,
}

#[derive(Debug, Clone, Serialize)]
pub struct McpCatalogServerView {
    pub mcp_server_id: Uuid,
    pub server_key: String,
    pub display_name: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct McpCatalogToolSummary {
    pub mcp_tool_id: Uuid,
    pub upstream_name: String,
    pub display_name: String,
    pub description: Option<String>,
    pub schema_hash: String,
    pub schema_version: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct McpCatalogToolDescription {
    pub mcp_tool_id: Uuid,
    pub upstream_name: String,
    pub display_name: String,
    pub description: Option<String>,
    pub input_schema: Value,
    pub schema_hash: String,
    pub schema_version: i64,
}

#[derive(Clone)]
pub struct McpCatalog<R> {
    repo: Arc<R>,
}

impl<R> McpCatalog<R>
where
    R: McpAccessRepository + IdentityRepository,
{
    #[must_use]
    pub fn new(repo: Arc<R>) -> Self {
        Self { repo }
    }

    pub async fn search_tools(
        &self,
        auth: &AuthenticatedApiKey,
        input: SearchMcpToolsInput,
    ) -> Result<SearchMcpToolsOutput, GatewayError> {
        let limit = input.limit.clamp(1, MAX_SEARCH_LIMIT);
        let server_key = normalize_optional_filter(input.server_key)?;
        let records = self.authorized_catalog(auth, server_key.as_deref()).await?;
        let query_tokens = tokenize(&input.query);

        let mut scored = records
            .allowed_tools
            .into_iter()
            .filter_map(|record| {
                let score = score_record(&record, &query_tokens);
                if query_tokens.is_empty() || score > 0 {
                    Some((record, score))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        scored.sort_by(|(a, a_score), (b, b_score)| {
            b_score
                .cmp(a_score)
                .then_with(|| a.server.server_key.cmp(&b.server.server_key))
                .then_with(|| a.tool.upstream_name.cmp(&b.tool.upstream_name))
                .then_with(|| a.tool.mcp_tool_id.cmp(&b.tool.mcp_tool_id))
        });

        let total = scored.len();
        let items = scored
            .into_iter()
            .skip(input.offset)
            .take(limit)
            .map(|(record, score)| search_item(record, score))
            .collect::<Result<Vec<_>, _>>()?;
        let next_offset = input
            .offset
            .checked_add(items.len())
            .filter(|next| *next < total);

        Ok(SearchMcpToolsOutput {
            items,
            total,
            next_offset,
            ranker: MCP_CATALOG_RANKER,
        })
    }

    pub async fn describe_tool(
        &self,
        auth: &AuthenticatedApiKey,
        input: DescribeMcpToolInput,
    ) -> Result<DescribeMcpToolOutput, GatewayError> {
        let parsed = parse_tool_address(&input.address)?;
        let record = self.authorized_tool(auth, &parsed).await?;
        Ok(DescribeMcpToolOutput {
            address: tool_address(&record.server.server_key, &record.tool.upstream_name)?,
            server: server_view(&record),
            tool: McpCatalogToolDescription {
                mcp_tool_id: record.tool.mcp_tool_id,
                upstream_name: record.tool.upstream_name,
                display_name: record.tool.display_name,
                description: record.tool.description,
                input_schema: record.tool.input_schema,
                schema_hash: record.tool.schema_hash,
                schema_version: record.tool.schema_version,
            },
        })
    }

    pub async fn authorized_tool_by_address(
        &self,
        auth: &AuthenticatedApiKey,
        address: &str,
    ) -> Result<McpCatalogToolRecord, GatewayError> {
        let parsed = parse_tool_address(address)?;
        self.authorized_tool(auth, &parsed).await
    }

    async fn authorized_catalog(
        &self,
        auth: &AuthenticatedApiKey,
        server_key: Option<&str>,
    ) -> Result<gateway_core::McpCatalogAccessResolution, GatewayError> {
        let access = McpAccess::new(self.repo.clone());
        let subjects = access.grant_subjects(auth).await?;
        Ok(self
            .repo
            .resolve_mcp_catalog_access_for_subjects(&subjects, server_key)
            .await?)
    }

    async fn authorized_tool(
        &self,
        auth: &AuthenticatedApiKey,
        parsed: &McpToolAddress,
    ) -> Result<McpCatalogToolRecord, GatewayError> {
        let records = self
            .authorized_catalog(auth, Some(parsed.server_key.as_str()))
            .await?;
        records
            .allowed_tools
            .into_iter()
            .find(|record| record.tool.upstream_name == parsed.upstream_name)
            .ok_or_else(|| GatewayError::InvalidRequest(MCP_TOOL_NOT_GRANTED_MESSAGE.to_string()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpToolAddress {
    pub server_key: String,
    pub upstream_name: String,
}

pub fn tool_address(server_key: &str, upstream_name: &str) -> Result<String, GatewayError> {
    let mut url = Url::parse(&format!("{MCP_TOOL_ADDRESS_SCHEME}://{server_key}"))
        .map_err(|error| GatewayError::InvalidRequest(error.to_string()))?;
    url.path_segments_mut()
        .map_err(|_| GatewayError::InvalidRequest("MCP tool address cannot be built".to_string()))?
        .push("tools")
        .push(upstream_name);
    Ok(url.to_string())
}

pub fn parse_tool_address(address: &str) -> Result<McpToolAddress, GatewayError> {
    let url = Url::parse(address)
        .map_err(|_| GatewayError::InvalidRequest("MCP tool address is invalid".to_string()))?;
    if url.scheme() != MCP_TOOL_ADDRESS_SCHEME {
        return Err(GatewayError::InvalidRequest(
            "MCP tool address must use the mcp scheme".to_string(),
        ));
    }
    let server_key = url.host_str().ok_or_else(|| {
        GatewayError::InvalidRequest("MCP tool address is missing a server key".to_string())
    })?;
    let path = url.path().strip_prefix("/tools/").ok_or_else(|| {
        GatewayError::InvalidRequest("MCP tool address must use /tools/{name}".to_string())
    })?;
    if server_key.is_empty() || path.is_empty() {
        return Err(GatewayError::InvalidRequest(
            "MCP tool address is incomplete".to_string(),
        ));
    }
    let upstream_name = percent_decode_str(path)
        .decode_utf8()
        .map_err(|error| GatewayError::InvalidRequest(error.to_string()))?
        .to_string();
    Ok(McpToolAddress {
        server_key: server_key.to_string(),
        upstream_name,
    })
}

fn default_search_limit() -> usize {
    DEFAULT_SEARCH_LIMIT
}

fn normalize_optional_filter(value: Option<String>) -> Result<Option<String>, GatewayError> {
    value
        .map(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                Err(GatewayError::InvalidRequest(
                    "server_key filter cannot be empty".to_string(),
                ))
            } else {
                Ok(trimmed.to_string())
            }
        })
        .transpose()
}

fn search_item(
    record: McpCatalogToolRecord,
    score: i64,
) -> Result<McpCatalogSearchItem, GatewayError> {
    Ok(McpCatalogSearchItem {
        address: tool_address(&record.server.server_key, &record.tool.upstream_name)?,
        score,
        server: server_view(&record),
        tool: McpCatalogToolSummary {
            mcp_tool_id: record.tool.mcp_tool_id,
            upstream_name: record.tool.upstream_name,
            display_name: record.tool.display_name,
            description: record.tool.description,
            schema_hash: record.tool.schema_hash,
            schema_version: record.tool.schema_version,
        },
    })
}

fn server_view(record: &McpCatalogToolRecord) -> McpCatalogServerView {
    McpCatalogServerView {
        mcp_server_id: record.server.mcp_server_id,
        server_key: record.server.server_key.clone(),
        display_name: record.server.display_name.clone(),
        description: record.server.description.clone(),
    }
}

fn score_record(record: &McpCatalogToolRecord, query_tokens: &[String]) -> i64 {
    if query_tokens.is_empty() {
        return 1;
    }
    let name_fields = [
        record.tool.upstream_name.as_str(),
        record.tool.display_name.as_str(),
    ];
    let description_fields = [
        record.tool.description.as_deref().unwrap_or_default(),
        record.server.description.as_deref().unwrap_or_default(),
    ];
    let server_fields = [
        record.server.server_key.as_str(),
        record.server.display_name.as_str(),
    ];

    let mut score = 0;
    for token in query_tokens {
        if name_fields
            .iter()
            .any(|field| field.eq_ignore_ascii_case(token))
        {
            score += 100;
        } else if name_fields
            .iter()
            .flat_map(|field| tokenize(field))
            .any(|field_token| field_token.starts_with(token))
        {
            score += 60;
        } else if name_fields
            .iter()
            .flat_map(|field| tokenize(field))
            .any(|field_token| field_token == *token)
        {
            score += 45;
        } else if description_fields
            .iter()
            .flat_map(|field| tokenize(field))
            .any(|field_token| field_token == *token)
        {
            score += 20;
        } else if server_fields
            .iter()
            .flat_map(|field| tokenize(field))
            .any(|field_token| field_token == *token || field_token.starts_with(token))
        {
            score += 10;
        }
    }
    score
}

fn tokenize(value: &str) -> Vec<String> {
    value
        .split(|character: char| {
            !character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.' | '/' | ':')
        })
        .filter_map(|token| {
            let normalized = token.trim().to_ascii_lowercase();
            (!normalized.is_empty()).then_some(normalized)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_address_round_trips_special_names() {
        let address = tool_address("github", "issues/create draft").expect("address");
        assert_eq!(address, "mcp://github/tools/issues%2Fcreate%20draft");
        let parsed = parse_tool_address(&address).expect("parsed address");
        assert_eq!(parsed.server_key, "github");
        assert_eq!(parsed.upstream_name, "issues/create draft");
    }
}
