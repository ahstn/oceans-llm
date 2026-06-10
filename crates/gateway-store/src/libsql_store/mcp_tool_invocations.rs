use super::*;
use crate::shared::{parse_uuid, serialize_json, unix_to_datetime};

fn normalize_query(query: &McpToolInvocationQuery) -> (i64, i64) {
    let page = query.page.max(1);
    let page_size = query.page_size.clamp(1, MAX_MCP_TOOL_INVOCATION_PAGE_SIZE);
    let offset = u64::from(page.saturating_sub(1)) * u64::from(page_size);
    (i64::from(page), offset as i64)
}

fn decode_mcp_tool_invocation_row(
    row: &libsql::Row,
) -> Result<McpToolInvocationRecord, StoreError> {
    let mcp_tool_invocation_id: String = row.get(0).map_err(to_query_error)?;
    let parent_invocation_id: Option<String> = row.get(24).map_err(to_query_error)?;
    let request_log_id: Option<String> = row.get(1).map_err(to_query_error)?;
    let api_key_id: Option<String> = row.get(3).map_err(to_query_error)?;
    let user_id: Option<String> = row.get(4).map_err(to_query_error)?;
    let team_id: Option<String> = row.get(5).map_err(to_query_error)?;
    let owner_kind: String = row.get(6).map_err(to_query_error)?;
    let server_id: Option<String> = row.get(7).map_err(to_query_error)?;
    let tool_id: Option<String> = row.get(10).map_err(to_query_error)?;
    let status: String = row.get(13).map_err(to_query_error)?;
    let policy_result: String = row.get(14).map_err(to_query_error)?;
    let has_payload: i64 = row.get(17).map_err(to_query_error)?;
    let arguments_payload_truncated: i64 = row.get(18).map_err(to_query_error)?;
    let result_payload_truncated: i64 = row.get(19).map_err(to_query_error)?;
    let arguments_payload_redacted: i64 = row.get(20).map_err(to_query_error)?;
    let result_payload_redacted: i64 = row.get(21).map_err(to_query_error)?;
    let metadata_json: String = row.get(22).map_err(to_query_error)?;
    let occurred_at: i64 = row.get(23).map_err(to_query_error)?;

    Ok(McpToolInvocationRecord {
        mcp_tool_invocation_id: parse_uuid(&mcp_tool_invocation_id)?,
        parent_invocation_id: parent_invocation_id
            .as_deref()
            .map(parse_uuid)
            .transpose()?,
        request_log_id: request_log_id.as_deref().map(parse_uuid).transpose()?,
        request_id: row.get(2).map_err(to_query_error)?,
        api_key_id: api_key_id.as_deref().map(parse_uuid).transpose()?,
        user_id: user_id.as_deref().map(parse_uuid).transpose()?,
        team_id: team_id.as_deref().map(parse_uuid).transpose()?,
        owner_kind: ApiKeyOwnerKind::from_db(&owner_kind).ok_or_else(|| {
            StoreError::Serialization(format!("invalid owner kind `{owner_kind}`"))
        })?,
        server_id: server_id.as_deref().map(parse_uuid).transpose()?,
        server_display_key: row.get(8).map_err(to_query_error)?,
        server_display_name: row.get(9).map_err(to_query_error)?,
        tool_id: tool_id.as_deref().map(parse_uuid).transpose()?,
        tool_display_key: row.get(11).map_err(to_query_error)?,
        tool_display_name: row.get(12).map_err(to_query_error)?,
        status: McpToolInvocationStatus::from_db(&status).ok_or_else(|| {
            StoreError::Serialization(format!("invalid MCP tool invocation status `{status}`"))
        })?,
        policy_result: McpToolPolicyResult::from_db(&policy_result).ok_or_else(|| {
            StoreError::Serialization(format!("invalid MCP tool policy result `{policy_result}`"))
        })?,
        latency_ms: row.get(15).map_err(to_query_error)?,
        error_code: row.get(16).map_err(to_query_error)?,
        has_payload: has_payload == 1,
        arguments_payload_truncated: arguments_payload_truncated == 1,
        result_payload_truncated: result_payload_truncated == 1,
        arguments_payload_redacted: arguments_payload_redacted == 1,
        result_payload_redacted: result_payload_redacted == 1,
        metadata: serde_json::from_str(&metadata_json)
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
        occurred_at: unix_to_datetime(occurred_at)?,
    })
}

#[async_trait]
impl McpToolInvocationRepository for LibsqlStore {
    async fn insert_mcp_tool_invocation(
        &self,
        invocation: &McpToolInvocationRecord,
        payload: Option<&McpToolInvocationPayloadRecord>,
    ) -> Result<(), StoreError> {
        let metadata_json = serialize_json(&invocation.metadata)?;
        let tx = self
            .connection
            .transaction()
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?;

        tx.execute(
            r#"
            INSERT INTO mcp_tool_invocations (
                mcp_tool_invocation_id, request_log_id, request_id, api_key_id, user_id, team_id,
                owner_kind, server_id, server_display_key, server_display_name, tool_id, tool_display_key,
                tool_display_name, status, policy_result, latency_ms, error_code, has_payload,
                arguments_payload_truncated, result_payload_truncated, arguments_payload_redacted,
                result_payload_redacted, metadata_json, occurred_at, parent_invocation_id
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25)
            "#,
            libsql::params![
                invocation.mcp_tool_invocation_id.to_string(),
                invocation.request_log_id.map(|value| value.to_string()),
                invocation.request_id.as_str(),
                invocation.api_key_id.map(|value| value.to_string()),
                invocation.user_id.map(|value| value.to_string()),
                invocation.team_id.map(|value| value.to_string()),
                invocation.owner_kind.as_str(),
                invocation.server_id.map(|value| value.to_string()),
                invocation.server_display_key.as_str(),
                invocation.server_display_name.as_str(),
                invocation.tool_id.map(|value| value.to_string()),
                invocation.tool_display_key.as_str(),
                invocation.tool_display_name.as_str(),
                invocation.status.as_str(),
                invocation.policy_result.as_str(),
                invocation.latency_ms,
                invocation.error_code.as_deref(),
                if invocation.has_payload { 1_i64 } else { 0_i64 },
                if invocation.arguments_payload_truncated { 1_i64 } else { 0_i64 },
                if invocation.result_payload_truncated { 1_i64 } else { 0_i64 },
                if invocation.arguments_payload_redacted { 1_i64 } else { 0_i64 },
                if invocation.result_payload_redacted { 1_i64 } else { 0_i64 },
                metadata_json,
                invocation.occurred_at.unix_timestamp(),
                invocation
                    .parent_invocation_id
                    .map(|value| value.to_string()),
            ],
        )
        .await
        .map_err(|error| StoreError::Query(error.to_string()))?;

        if let Some(payload) = payload {
            let arguments_json = serialize_json(&payload.arguments_json)?;
            let result_json = serialize_json(&payload.result_json)?;
            tx.execute(
                r#"
                INSERT INTO mcp_tool_invocation_payloads (
                    mcp_tool_invocation_id, arguments_json, result_json
                ) VALUES (?1, ?2, ?3)
                "#,
                libsql::params![
                    payload.mcp_tool_invocation_id.to_string(),
                    arguments_json,
                    result_json
                ],
            )
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?;
        }

        tx.commit()
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?;
        Ok(())
    }

    async fn list_mcp_tool_invocations(
        &self,
        query: &McpToolInvocationQuery,
    ) -> Result<McpToolInvocationPage, StoreError> {
        let (page, offset) = normalize_query(query);
        let page_size = i64::from(query.page_size.clamp(1, MAX_MCP_TOOL_INVOCATION_PAGE_SIZE));
        let api_key_id = query.api_key_id.map(|value| value.to_string());
        let user_id = query.user_id.map(|value| value.to_string());
        let team_id = query.team_id.map(|value| value.to_string());
        let status = query.status.map(McpToolInvocationStatus::as_str);
        let policy_result = query.policy_result.map(McpToolPolicyResult::as_str);
        let occurred_at_start = query.occurred_at_start.map(|value| value.unix_timestamp());
        let occurred_at_end = query.occurred_at_end.map(|value| value.unix_timestamp());
        let parent_invocation_id = query.parent_invocation_id.map(|value| value.to_string());

        let mut count_rows = self
            .connection
            .query(
                r#"
                SELECT COUNT(*)
                FROM mcp_tool_invocations
                WHERE (?1 IS NULL OR request_id = ?1)
                  AND (?2 IS NULL OR server_display_key = ?2)
                  AND (?3 IS NULL OR server_display_name = ?3)
                  AND (?4 IS NULL OR tool_display_key = ?4)
                  AND (?5 IS NULL OR tool_display_name = ?5)
                  AND (?6 IS NULL OR api_key_id = ?6)
                  AND (?7 IS NULL OR user_id = ?7)
                  AND (?8 IS NULL OR team_id = ?8)
                  AND (?9 IS NULL OR status = ?9)
                  AND (?10 IS NULL OR policy_result = ?10)
                  AND (?11 IS NULL OR occurred_at >= ?11)
                  AND (?12 IS NULL OR occurred_at <= ?12)
                  AND (?13 IS NULL OR parent_invocation_id = ?13)
                "#,
                libsql::params![
                    query.request_id.as_deref(),
                    query.server_display_key.as_deref(),
                    query.server_display_name.as_deref(),
                    query.tool_display_key.as_deref(),
                    query.tool_display_name.as_deref(),
                    api_key_id.clone(),
                    user_id.clone(),
                    team_id.clone(),
                    status,
                    policy_result,
                    occurred_at_start,
                    occurred_at_end,
                    parent_invocation_id.clone(),
                ],
            )
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?;
        let count_row = count_rows
            .next()
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?
            .ok_or_else(|| StoreError::Query("MCP invocation count row missing".to_string()))?;
        let total: i64 = count_row.get(0).map_err(to_query_error)?;

        let mut rows = self
            .connection
            .query(
                r#"
                SELECT mcp_tool_invocation_id, request_log_id, request_id, api_key_id, user_id,
                       team_id, owner_kind, server_id, server_display_key, server_display_name, tool_id,
                       tool_display_key, tool_display_name, status, policy_result, latency_ms,
                       error_code, has_payload, arguments_payload_truncated,
                       result_payload_truncated, arguments_payload_redacted,
                       result_payload_redacted, metadata_json, occurred_at, parent_invocation_id
                FROM mcp_tool_invocations
                WHERE (?1 IS NULL OR request_id = ?1)
                  AND (?2 IS NULL OR server_display_key = ?2)
                  AND (?3 IS NULL OR server_display_name = ?3)
                  AND (?4 IS NULL OR tool_display_key = ?4)
                  AND (?5 IS NULL OR tool_display_name = ?5)
                  AND (?6 IS NULL OR api_key_id = ?6)
                  AND (?7 IS NULL OR user_id = ?7)
                  AND (?8 IS NULL OR team_id = ?8)
                  AND (?9 IS NULL OR status = ?9)
                  AND (?10 IS NULL OR policy_result = ?10)
                  AND (?11 IS NULL OR occurred_at >= ?11)
                  AND (?12 IS NULL OR occurred_at <= ?12)
                  AND (?13 IS NULL OR parent_invocation_id = ?13)
                ORDER BY occurred_at DESC, mcp_tool_invocation_id DESC
                LIMIT ?14 OFFSET ?15
                "#,
                libsql::params![
                    query.request_id.as_deref(),
                    query.server_display_key.as_deref(),
                    query.server_display_name.as_deref(),
                    query.tool_display_key.as_deref(),
                    query.tool_display_name.as_deref(),
                    api_key_id,
                    user_id,
                    team_id,
                    status,
                    policy_result,
                    occurred_at_start,
                    occurred_at_end,
                    parent_invocation_id,
                    page_size,
                    offset,
                ],
            )
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?;

        let mut items = Vec::new();
        while let Some(row) = rows
            .next()
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?
        {
            items.push(decode_mcp_tool_invocation_row(&row)?);
        }

        Ok(McpToolInvocationPage {
            items,
            page: u32::try_from(page).unwrap_or(u32::MAX),
            page_size: u32::try_from(page_size).unwrap_or(u32::MAX),
            total: u64::try_from(total.max(0)).unwrap_or(u64::MAX),
        })
    }

    async fn get_mcp_tool_invocation_detail(
        &self,
        mcp_tool_invocation_id: Uuid,
    ) -> Result<McpToolInvocationDetail, StoreError> {
        let mut rows = self
            .connection
            .query(
                r#"
                SELECT mti.mcp_tool_invocation_id, mti.request_log_id, mti.request_id,
                       mti.api_key_id, mti.user_id, mti.team_id, mti.owner_kind, mti.server_id,
                       mti.server_display_key, mti.server_display_name, mti.tool_id,
                       mti.tool_display_key, mti.tool_display_name, mti.status,
                       mti.policy_result, mti.latency_ms, mti.error_code, mti.has_payload,
                       mti.arguments_payload_truncated, mti.result_payload_truncated,
                       mti.arguments_payload_redacted, mti.result_payload_redacted,
                       mti.metadata_json, mti.occurred_at, mti.parent_invocation_id,
                       mtip.arguments_json, mtip.result_json
                FROM mcp_tool_invocations mti
                LEFT JOIN mcp_tool_invocation_payloads mtip
                  ON mtip.mcp_tool_invocation_id = mti.mcp_tool_invocation_id
                WHERE mti.mcp_tool_invocation_id = ?1
                "#,
                [mcp_tool_invocation_id.to_string()],
            )
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?;

        let Some(row) = rows
            .next()
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?
        else {
            return Err(StoreError::NotFound(format!(
                "MCP tool invocation `{mcp_tool_invocation_id}` not found"
            )));
        };

        let invocation = decode_mcp_tool_invocation_row(&row)?;
        let arguments_json: Option<String> = row.get(25).map_err(to_query_error)?;
        let result_json: Option<String> = row.get(26).map_err(to_query_error)?;
        let payload = match (arguments_json, result_json) {
            (Some(arguments_json), Some(result_json)) => Some(McpToolInvocationPayloadRecord {
                mcp_tool_invocation_id,
                arguments_json: serde_json::from_str(&arguments_json)
                    .map_err(|error| StoreError::Serialization(error.to_string()))?,
                result_json: serde_json::from_str(&result_json)
                    .map_err(|error| StoreError::Serialization(error.to_string()))?,
            }),
            _ => None,
        };

        Ok(McpToolInvocationDetail {
            invocation,
            payload,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_query_handles_large_page_without_overflow() {
        let query = McpToolInvocationQuery {
            page: u32::MAX,
            page_size: u32::MAX,
            ..McpToolInvocationQuery::default()
        };

        let (page, offset) = normalize_query(&query);

        assert_eq!(page, i64::from(u32::MAX));
        assert_eq!(
            offset,
            i64::from(u32::MAX - 1) * i64::from(MAX_MCP_TOOL_INVOCATION_PAGE_SIZE)
        );
    }
}
