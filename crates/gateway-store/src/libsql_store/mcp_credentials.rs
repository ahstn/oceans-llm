use super::*;
use crate::shared::{json_object_from_str, parse_uuid, serialize_json, unix_to_datetime};

const CREDENTIAL_COLUMNS: &str = "credential_binding_id, mcp_server_id, owner_scope_kind, owner_scope_key, owner_user_id, owner_team_id, owner_service_account_id, material_kind, header_name, storage_kind, secret_ciphertext, secret_nonce, secret_key_id, secret_ref, expires_at, metadata_json, created_at, updated_at, last_used_at, revoked_at";

fn decode_credential(row: &libsql::Row) -> Result<McpUpstreamCredentialBindingRecord, StoreError> {
    let credential_binding_id: String = row.get(0).map_err(to_query_error)?;
    let mcp_server_id: String = row.get(1).map_err(to_query_error)?;
    let owner_scope_kind: String = row.get(2).map_err(to_query_error)?;
    let owner_user_id: Option<String> = row.get(4).map_err(to_query_error)?;
    let owner_team_id: Option<String> = row.get(5).map_err(to_query_error)?;
    let owner_service_account_id: Option<String> = row.get(6).map_err(to_query_error)?;
    let material_kind: String = row.get(7).map_err(to_query_error)?;
    let storage_kind: String = row.get(9).map_err(to_query_error)?;
    let expires_at: Option<i64> = row.get(14).map_err(to_query_error)?;
    let metadata_json: String = row.get(15).map_err(to_query_error)?;
    let created_at: i64 = row.get(16).map_err(to_query_error)?;
    let updated_at: i64 = row.get(17).map_err(to_query_error)?;
    let last_used_at: Option<i64> = row.get(18).map_err(to_query_error)?;
    let revoked_at: Option<i64> = row.get(19).map_err(to_query_error)?;
    Ok(McpUpstreamCredentialBindingRecord {
        credential_binding_id: parse_uuid(&credential_binding_id)?,
        mcp_server_id: parse_uuid(&mcp_server_id)?,
        owner_scope_kind: McpUpstreamCredentialOwnerScopeKind::from_db(&owner_scope_kind)
            .ok_or_else(|| {
                StoreError::Serialization(format!(
                    "invalid MCP credential owner scope `{owner_scope_kind}`"
                ))
            })?,
        owner_scope_key: row.get(3).map_err(to_query_error)?,
        owner_user_id: owner_user_id.as_deref().map(parse_uuid).transpose()?,
        owner_team_id: owner_team_id.as_deref().map(parse_uuid).transpose()?,
        owner_service_account_id: owner_service_account_id
            .as_deref()
            .map(parse_uuid)
            .transpose()?,
        material_kind: McpUpstreamCredentialMaterialKind::from_db(&material_kind).ok_or_else(
            || {
                StoreError::Serialization(format!(
                    "invalid MCP credential material `{material_kind}`"
                ))
            },
        )?,
        header_name: row.get(8).map_err(to_query_error)?,
        storage_kind: McpUpstreamSecretStorageKind::from_db(&storage_kind).ok_or_else(|| {
            StoreError::Serialization(format!("invalid MCP credential storage `{storage_kind}`"))
        })?,
        secret_ciphertext: row.get(10).map_err(to_query_error)?,
        secret_nonce: row.get(11).map_err(to_query_error)?,
        secret_key_id: row.get(12).map_err(to_query_error)?,
        secret_ref: row.get(13).map_err(to_query_error)?,
        expires_at: expires_at.map(unix_to_datetime).transpose()?,
        metadata: json_object_from_str(&metadata_json)?,
        created_at: unix_to_datetime(created_at)?,
        updated_at: unix_to_datetime(updated_at)?,
        last_used_at: last_used_at.map(unix_to_datetime).transpose()?,
        revoked_at: revoked_at.map(unix_to_datetime).transpose()?,
    })
}

async fn load_credential(
    connection: &libsql::Connection,
    credential_binding_id: Uuid,
) -> Result<McpUpstreamCredentialBindingRecord, StoreError> {
    let sql = format!(
        "SELECT {CREDENTIAL_COLUMNS} FROM mcp_upstream_credential_bindings WHERE credential_binding_id = ?1"
    );
    let mut rows = connection
        .query(&sql, [credential_binding_id.to_string()])
        .await
        .map_err(to_query_error)?;
    rows.next()
        .await
        .map_err(to_query_error)?
        .map(|row| decode_credential(&row))
        .transpose()?
        .ok_or_else(|| {
            StoreError::Unexpected(format!(
                "MCP credential binding `{credential_binding_id}` was not found"
            ))
        })
}

#[async_trait]
impl McpUpstreamCredentialRepository for LibsqlStore {
    async fn upsert_mcp_upstream_credential_binding(
        &self,
        input: &UpsertMcpUpstreamCredentialBindingRecord,
    ) -> Result<McpUpstreamCredentialBindingRecord, StoreError> {
        let id = input.credential_binding_id.unwrap_or_else(Uuid::new_v4);
        let metadata_json = serialize_json(&input.metadata)?;
        if input.credential_binding_id.is_some() {
            let changed = self
                .connection
                .execute(
                    r#"
                        UPDATE mcp_upstream_credential_bindings
                        SET mcp_server_id = ?2, owner_scope_kind = ?3, owner_scope_key = ?4,
                            owner_user_id = ?5, owner_team_id = ?6, owner_service_account_id = ?7,
                            material_kind = ?8, header_name = ?9, storage_kind = ?10,
                            secret_ciphertext = ?11, secret_nonce = ?12, secret_key_id = ?13,
                            secret_ref = ?14, expires_at = ?15, metadata_json = ?16,
                            updated_at = ?17, revoked_at = NULL
                        WHERE credential_binding_id = ?1
                        "#,
                    libsql::params![
                        id.to_string(),
                        input.mcp_server_id.to_string(),
                        input.owner_scope_kind.as_str(),
                        input.owner_scope_key.as_str(),
                        input.owner_user_id.map(|value| value.to_string()),
                        input.owner_team_id.map(|value| value.to_string()),
                        input
                            .owner_service_account_id
                            .map(|value| value.to_string()),
                        input.material_kind.as_str(),
                        input.header_name.as_deref(),
                        input.storage_kind.as_str(),
                        input.secret_ciphertext.as_deref(),
                        input.secret_nonce.as_deref(),
                        input.secret_key_id.as_deref(),
                        input.secret_ref.as_deref(),
                        input.expires_at.map(|value| value.unix_timestamp()),
                        metadata_json.as_str(),
                        input.updated_at.unix_timestamp(),
                    ],
                )
                .await
                .map_err(to_write_error)?;
            if changed > 0 {
                return load_credential(&self.connection, id).await;
            }
        }
        self.connection
            .execute(
                r#"
                INSERT INTO mcp_upstream_credential_bindings (
                    credential_binding_id, mcp_server_id, owner_scope_kind, owner_scope_key,
                    owner_user_id, owner_team_id, owner_service_account_id, material_kind,
                    header_name, storage_kind, secret_ciphertext, secret_nonce, secret_key_id,
                    secret_ref, expires_at, metadata_json, created_at, updated_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?17)
                "#,
                libsql::params![
                    id.to_string(),
                    input.mcp_server_id.to_string(),
                    input.owner_scope_kind.as_str(),
                    input.owner_scope_key.as_str(),
                    input.owner_user_id.map(|value| value.to_string()),
                    input.owner_team_id.map(|value| value.to_string()),
                    input.owner_service_account_id.map(|value| value.to_string()),
                    input.material_kind.as_str(),
                    input.header_name.as_deref(),
                    input.storage_kind.as_str(),
                    input.secret_ciphertext.as_deref(),
                    input.secret_nonce.as_deref(),
                    input.secret_key_id.as_deref(),
                    input.secret_ref.as_deref(),
                    input.expires_at.map(|value| value.unix_timestamp()),
                    metadata_json.as_str(),
                    input.updated_at.unix_timestamp(),
                ],
            )
            .await
            .map_err(to_write_error)?;
        load_credential(&self.connection, id).await
    }

    async fn get_active_mcp_upstream_credential_binding(
        &self,
        mcp_server_id: Uuid,
        owner_scope_key: &str,
    ) -> Result<Option<McpUpstreamCredentialBindingRecord>, StoreError> {
        let sql = format!(
            "SELECT {CREDENTIAL_COLUMNS} FROM mcp_upstream_credential_bindings WHERE mcp_server_id = ?1 AND owner_scope_key = ?2 AND revoked_at IS NULL"
        );
        let mut rows = self
            .connection
            .query(
                &sql,
                libsql::params![mcp_server_id.to_string(), owner_scope_key],
            )
            .await
            .map_err(to_query_error)?;
        rows.next()
            .await
            .map_err(to_query_error)?
            .map(|row| decode_credential(&row))
            .transpose()
    }

    async fn list_mcp_upstream_credential_bindings(
        &self,
        mcp_server_id: Option<Uuid>,
        owner_scope_kind: Option<McpUpstreamCredentialOwnerScopeKind>,
        owner_scope_id: Option<Uuid>,
        include_revoked: bool,
    ) -> Result<Vec<McpUpstreamCredentialBindingRecord>, StoreError> {
        let sql = format!(
            r#"
            SELECT {CREDENTIAL_COLUMNS}
            FROM mcp_upstream_credential_bindings
            WHERE (?1 IS NULL OR mcp_server_id = ?1)
              AND (?2 IS NULL OR owner_scope_kind = ?2)
              AND (?3 IS NULL OR owner_user_id = ?3 OR owner_team_id = ?3 OR owner_service_account_id = ?3)
              AND (?4 = 1 OR revoked_at IS NULL)
            ORDER BY mcp_server_id, owner_scope_key, created_at DESC
            "#
        );
        let mut rows = self
            .connection
            .query(
                &sql,
                libsql::params![
                    mcp_server_id.map(|value| value.to_string()),
                    owner_scope_kind.map(|value| value.as_str().to_string()),
                    owner_scope_id.map(|value| value.to_string()),
                    if include_revoked { 1_i64 } else { 0_i64 },
                ],
            )
            .await
            .map_err(to_query_error)?;
        let mut bindings = Vec::new();
        while let Some(row) = rows.next().await.map_err(to_query_error)? {
            bindings.push(decode_credential(&row)?);
        }
        Ok(bindings)
    }

    async fn revoke_mcp_upstream_credential_binding(
        &self,
        credential_binding_id: Uuid,
        revoked_at: OffsetDateTime,
    ) -> Result<bool, StoreError> {
        let changed = self
            .connection
            .execute(
                r#"
                UPDATE mcp_upstream_credential_bindings
                SET revoked_at = ?1, updated_at = ?1
                WHERE credential_binding_id = ?2 AND revoked_at IS NULL
                "#,
                libsql::params![
                    revoked_at.unix_timestamp(),
                    credential_binding_id.to_string()
                ],
            )
            .await
            .map_err(to_write_error)?;
        Ok(changed > 0)
    }

    async fn touch_mcp_upstream_credential_binding_last_used(
        &self,
        credential_binding_id: Uuid,
        last_used_at: OffsetDateTime,
    ) -> Result<bool, StoreError> {
        let changed = self
            .connection
            .execute(
                r#"
                UPDATE mcp_upstream_credential_bindings
                SET last_used_at = ?1, updated_at = ?1
                WHERE credential_binding_id = ?2 AND revoked_at IS NULL
                "#,
                libsql::params![
                    last_used_at.unix_timestamp(),
                    credential_binding_id.to_string()
                ],
            )
            .await
            .map_err(to_write_error)?;
        Ok(changed > 0)
    }
}
