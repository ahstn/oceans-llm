use super::mcp_registry::check_ownership_guard;
use super::*;
use crate::shared::{json_object_from_str, parse_uuid, serialize_json, unix_to_datetime};

const CREDENTIAL_COLUMNS: &str = "credential_binding_id, mcp_server_id, owner_scope_kind, owner_scope_key, owner_user_id, owner_team_id, owner_service_account_id, material_kind, header_name, storage_kind, secret_ciphertext, secret_nonce, secret_key_id, secret_ref, expires_at, metadata_json::text, created_at, updated_at, last_used_at, revoked_at";

fn decode_credential(row: &PgRow) -> Result<McpUpstreamCredentialBindingRecord, StoreError> {
    let owner_scope_kind: String = row.try_get(2).map_err(to_query_error)?;
    let owner_user_id: Option<String> = row.try_get(4).map_err(to_query_error)?;
    let owner_team_id: Option<String> = row.try_get(5).map_err(to_query_error)?;
    let owner_service_account_id: Option<String> = row.try_get(6).map_err(to_query_error)?;
    let material_kind: String = row.try_get(7).map_err(to_query_error)?;
    let storage_kind: String = row.try_get(9).map_err(to_query_error)?;
    let expires_at: Option<i64> = row.try_get(14).map_err(to_query_error)?;
    let metadata_json: String = row.try_get(15).map_err(to_query_error)?;
    let last_used_at: Option<i64> = row.try_get(18).map_err(to_query_error)?;
    let revoked_at: Option<i64> = row.try_get(19).map_err(to_query_error)?;
    Ok(McpUpstreamCredentialBindingRecord {
        credential_binding_id: parse_uuid(&row.try_get::<String, _>(0).map_err(to_query_error)?)?,
        mcp_server_id: parse_uuid(&row.try_get::<String, _>(1).map_err(to_query_error)?)?,
        owner_scope_kind: McpUpstreamCredentialOwnerScopeKind::from_db(&owner_scope_kind)
            .ok_or_else(|| {
                StoreError::Serialization(format!(
                    "invalid MCP credential owner scope `{owner_scope_kind}`"
                ))
            })?,
        owner_scope_key: row.try_get(3).map_err(to_query_error)?,
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
        header_name: row.try_get(8).map_err(to_query_error)?,
        storage_kind: McpUpstreamSecretStorageKind::from_db(&storage_kind).ok_or_else(|| {
            StoreError::Serialization(format!("invalid MCP credential storage `{storage_kind}`"))
        })?,
        secret_ciphertext: row.try_get(10).map_err(to_query_error)?,
        secret_nonce: row.try_get(11).map_err(to_query_error)?,
        secret_key_id: row.try_get(12).map_err(to_query_error)?,
        secret_ref: row.try_get(13).map_err(to_query_error)?,
        expires_at: expires_at.map(unix_to_datetime).transpose()?,
        metadata: json_object_from_str(&metadata_json)?,
        created_at: unix_to_datetime(row.try_get(16).map_err(to_query_error)?)?,
        updated_at: unix_to_datetime(row.try_get(17).map_err(to_query_error)?)?,
        last_used_at: last_used_at.map(unix_to_datetime).transpose()?,
        revoked_at: revoked_at.map(unix_to_datetime).transpose()?,
    })
}

const OAUTH_CLIENT_COLUMNS: &str = "mcp_server_id, issuer, redirect_uri, client_id, token_endpoint_auth_method, secret_ciphertext, secret_nonce, secret_key_id, client_secret_expires_at, created_at, updated_at";

fn decode_oauth_client(row: &PgRow) -> Result<McpOauthClientRecord, StoreError> {
    let client_secret_expires_at: Option<i64> = row.try_get(8).map_err(to_query_error)?;
    Ok(McpOauthClientRecord {
        mcp_server_id: parse_uuid(&row.try_get::<String, _>(0).map_err(to_query_error)?)?,
        issuer: row.try_get(1).map_err(to_query_error)?,
        redirect_uri: row.try_get(2).map_err(to_query_error)?,
        client_id: row.try_get(3).map_err(to_query_error)?,
        token_endpoint_auth_method: row.try_get(4).map_err(to_query_error)?,
        secret_ciphertext: row.try_get(5).map_err(to_query_error)?,
        secret_nonce: row.try_get(6).map_err(to_query_error)?,
        secret_key_id: row.try_get(7).map_err(to_query_error)?,
        client_secret_expires_at: client_secret_expires_at.map(unix_to_datetime).transpose()?,
        created_at: unix_to_datetime(row.try_get(9).map_err(to_query_error)?)?,
        updated_at: unix_to_datetime(row.try_get(10).map_err(to_query_error)?)?,
    })
}

async fn find_credential(
    pool: &PgPool,
    credential_binding_id: Uuid,
) -> Result<Option<McpUpstreamCredentialBindingRecord>, StoreError> {
    let sql = format!(
        "SELECT {CREDENTIAL_COLUMNS} FROM mcp_upstream_credential_bindings WHERE credential_binding_id = $1"
    );
    let row = sqlx::query(&sql)
        .bind(credential_binding_id.to_string())
        .fetch_optional(pool)
        .await
        .map_err(to_query_error)?;
    row.as_ref().map(decode_credential).transpose()
}

async fn load_credential(
    pool: &PgPool,
    credential_binding_id: Uuid,
) -> Result<McpUpstreamCredentialBindingRecord, StoreError> {
    find_credential(pool, credential_binding_id)
        .await?
        .ok_or_else(|| {
            StoreError::Unexpected(format!(
                "MCP credential binding `{credential_binding_id}` was not found"
            ))
        })
}

#[async_trait]
impl McpUpstreamCredentialRepository for PostgresStore {
    async fn upsert_mcp_upstream_credential_binding(
        &self,
        input: &UpsertMcpUpstreamCredentialBindingRecord,
    ) -> Result<
        (
            McpUpstreamCredentialBindingRecord,
            Option<McpUpstreamCredentialBindingRecord>,
        ),
        StoreError,
    > {
        let id = input.credential_binding_id.unwrap_or_else(Uuid::new_v4);
        let metadata_json = serialize_json(&input.metadata)?;
        // `FOR UPDATE` locks the row, so the replaced row read here is exactly the
        // one the update below overwrites.
        let mut tx = self.pool.begin().await.map_err(to_query_error)?;
        if let Some(guard) = &input.expected_server {
            if guard.mcp_server_id != input.mcp_server_id {
                return Err(guard.conflict());
            }
            check_ownership_guard(&mut tx, guard, None).await?;
        }
        let previous = if input.credential_binding_id.is_some() {
            let row = sqlx::query(&format!(
                "SELECT {CREDENTIAL_COLUMNS} FROM mcp_upstream_credential_bindings WHERE credential_binding_id = $1 FOR UPDATE"
            ))
            .bind(id.to_string())
            .fetch_optional(&mut *tx)
            .await
            .map_err(to_query_error)?;
            row.as_ref().map(decode_credential).transpose()?
        } else {
            None
        };
        if let Some(previous) = previous {
            // A binding stays on its server: moving one would take a credential
            // off a server the caller was never checked against, under no lock.
            if previous.mcp_server_id != input.mcp_server_id {
                return Err(StoreError::Conflict(format!(
                    "credential binding `{id}` belongs to another MCP server; revoke it and create a new one"
                )));
            }
            let sql = format!(
                r#"
                UPDATE mcp_upstream_credential_bindings
                SET mcp_server_id = $2, owner_scope_kind = $3, owner_scope_key = $4,
                    owner_user_id = $5, owner_team_id = $6, owner_service_account_id = $7,
                    material_kind = $8, header_name = $9, storage_kind = $10,
                    secret_ciphertext = $11, secret_nonce = $12, secret_key_id = $13,
                    secret_ref = $14, expires_at = $15, metadata_json = $16::jsonb,
                    updated_at = $17, revoked_at = NULL
                WHERE credential_binding_id = $1
                RETURNING {CREDENTIAL_COLUMNS}
            "#
            );
            let row = sqlx::query(&sql)
                .bind(id.to_string())
                .bind(input.mcp_server_id.to_string())
                .bind(input.owner_scope_kind.as_str())
                .bind(&input.owner_scope_key)
                .bind(input.owner_user_id.map(|value| value.to_string()))
                .bind(input.owner_team_id.map(|value| value.to_string()))
                .bind(
                    input
                        .owner_service_account_id
                        .map(|value| value.to_string()),
                )
                .bind(input.material_kind.as_str())
                .bind(input.header_name.as_deref())
                .bind(input.storage_kind.as_str())
                .bind(input.secret_ciphertext.as_deref())
                .bind(input.secret_nonce.as_deref())
                .bind(input.secret_key_id.as_deref())
                .bind(input.secret_ref.as_deref())
                .bind(input.expires_at.map(|value| value.unix_timestamp()))
                .bind(&metadata_json)
                .bind(input.updated_at.unix_timestamp())
                .fetch_one(&mut *tx)
                .await
                .map_err(to_write_error)?;
            let replaced = decode_credential(&row)?;
            tx.commit().await.map_err(to_query_error)?;
            return Ok((replaced, Some(previous)));
        }
        let sql = format!(
            r#"
            INSERT INTO mcp_upstream_credential_bindings (
                credential_binding_id, mcp_server_id, owner_scope_kind, owner_scope_key,
                owner_user_id, owner_team_id, owner_service_account_id, material_kind,
                header_name, storage_kind, secret_ciphertext, secret_nonce, secret_key_id,
                secret_ref, expires_at, metadata_json, created_at, updated_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16::jsonb, $17, $17)
            RETURNING {CREDENTIAL_COLUMNS}
        "#
        );
        let row = sqlx::query(&sql)
            .bind(id.to_string())
            .bind(input.mcp_server_id.to_string())
            .bind(input.owner_scope_kind.as_str())
            .bind(&input.owner_scope_key)
            .bind(input.owner_user_id.map(|value| value.to_string()))
            .bind(input.owner_team_id.map(|value| value.to_string()))
            .bind(
                input
                    .owner_service_account_id
                    .map(|value| value.to_string()),
            )
            .bind(input.material_kind.as_str())
            .bind(input.header_name.as_deref())
            .bind(input.storage_kind.as_str())
            .bind(input.secret_ciphertext.as_deref())
            .bind(input.secret_nonce.as_deref())
            .bind(input.secret_key_id.as_deref())
            .bind(input.secret_ref.as_deref())
            .bind(input.expires_at.map(|value| value.unix_timestamp()))
            .bind(&metadata_json)
            .bind(input.updated_at.unix_timestamp())
            .fetch_one(&mut *tx)
            .await
            .map_err(to_write_error)?;
        let inserted = decode_credential(&row)?;
        tx.commit().await.map_err(to_query_error)?;
        Ok((inserted, None))
    }

    async fn get_active_mcp_upstream_credential_binding(
        &self,
        mcp_server_id: Uuid,
        owner_scope_key: &str,
    ) -> Result<Option<McpUpstreamCredentialBindingRecord>, StoreError> {
        let sql = format!(
            "SELECT {CREDENTIAL_COLUMNS} FROM mcp_upstream_credential_bindings WHERE mcp_server_id = $1 AND owner_scope_key = $2 AND revoked_at IS NULL"
        );
        let row = sqlx::query(&sql)
            .bind(mcp_server_id.to_string())
            .bind(owner_scope_key)
            .fetch_optional(&self.pool)
            .await
            .map_err(to_query_error)?;
        row.as_ref().map(decode_credential).transpose()
    }

    async fn compare_and_swap_mcp_oauth_credential_refresh(
        &self,
        input: &RefreshMcpOauthCredentialBindingRecord,
    ) -> Result<Option<McpUpstreamCredentialBindingRecord>, StoreError> {
        let metadata_json = serialize_json(&input.metadata)?;
        let result = sqlx::query(
            r#"
            UPDATE mcp_upstream_credential_bindings
            SET secret_ciphertext = $1, secret_nonce = $2, secret_key_id = $3,
                expires_at = $4, metadata_json = $5::jsonb, updated_at = $6
            WHERE credential_binding_id = $7
              AND revoked_at IS NULL
              AND secret_ciphertext = $8
            "#,
        )
        .bind(&input.secret_ciphertext)
        .bind(&input.secret_nonce)
        .bind(&input.secret_key_id)
        .bind(input.expires_at.unix_timestamp())
        .bind(&metadata_json)
        .bind(input.updated_at.unix_timestamp())
        .bind(input.credential_binding_id.to_string())
        .bind(&input.expected_secret_ciphertext)
        .execute(&self.pool)
        .await
        .map_err(to_write_error)?;
        if result.rows_affected() == 0 {
            return Ok(None);
        }
        load_credential(&self.pool, input.credential_binding_id)
            .await
            .map(Some)
    }

    async fn revoke_mcp_oauth_credential_if_unchanged(
        &self,
        credential_binding_id: Uuid,
        expected_secret_ciphertext: &str,
        revoked_at: OffsetDateTime,
    ) -> Result<bool, StoreError> {
        let result = sqlx::query(
            r#"
            UPDATE mcp_upstream_credential_bindings
            SET revoked_at = $1, updated_at = $1
            WHERE credential_binding_id = $2
              AND revoked_at IS NULL
              AND secret_ciphertext = $3
            "#,
        )
        .bind(revoked_at.unix_timestamp())
        .bind(credential_binding_id.to_string())
        .bind(expected_secret_ciphertext)
        .execute(&self.pool)
        .await
        .map_err(to_write_error)?;
        Ok(result.rows_affected() > 0)
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
            WHERE ($1 IS NULL OR mcp_server_id = $1)
              AND ($2 IS NULL OR owner_scope_kind = $2)
              AND ($3 IS NULL OR owner_user_id = $3 OR owner_team_id = $3 OR owner_service_account_id = $3)
              AND ($4 = TRUE OR revoked_at IS NULL)
            ORDER BY mcp_server_id, owner_scope_key, created_at DESC
            "#
        );
        let rows = sqlx::query(&sql)
            .bind(mcp_server_id.map(|value| value.to_string()))
            .bind(owner_scope_kind.map(|value| value.as_str().to_string()))
            .bind(owner_scope_id.map(|value| value.to_string()))
            .bind(include_revoked)
            .fetch_all(&self.pool)
            .await
            .map_err(to_query_error)?;
        rows.iter().map(decode_credential).collect()
    }

    async fn revoke_mcp_upstream_credential_binding(
        &self,
        credential_binding_id: Uuid,
        revoked_at: OffsetDateTime,
    ) -> Result<Option<McpUpstreamCredentialBindingRecord>, StoreError> {
        let sql = format!(
            r#"
            UPDATE mcp_upstream_credential_bindings
            SET revoked_at = $1, updated_at = $1
            WHERE credential_binding_id = $2 AND revoked_at IS NULL
            RETURNING {CREDENTIAL_COLUMNS}
            "#
        );
        let row = sqlx::query(&sql)
            .bind(revoked_at.unix_timestamp())
            .bind(credential_binding_id.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(to_write_error)?;
        row.as_ref().map(decode_credential).transpose()
    }

    async fn touch_mcp_upstream_credential_binding_last_used(
        &self,
        credential_binding_id: Uuid,
        last_used_at: OffsetDateTime,
    ) -> Result<bool, StoreError> {
        let result = sqlx::query(
            r#"
            UPDATE mcp_upstream_credential_bindings
            SET last_used_at = $1, updated_at = $1
            WHERE credential_binding_id = $2 AND revoked_at IS NULL
            "#,
        )
        .bind(last_used_at.unix_timestamp())
        .bind(credential_binding_id.to_string())
        .execute(&self.pool)
        .await
        .map_err(to_write_error)?;
        Ok(result.rows_affected() > 0)
    }

    async fn try_acquire_mcp_oauth_refresh_lease(
        &self,
        credential_binding_id: Uuid,
        lease_token: Uuid,
        now: OffsetDateTime,
        expires_at: OffsetDateTime,
    ) -> Result<bool, StoreError> {
        let result = sqlx::query(
            r#"
            INSERT INTO mcp_oauth_refresh_leases (
                credential_binding_id, lease_token, expires_at
            ) VALUES ($1, $2, $3)
            ON CONFLICT(credential_binding_id) DO UPDATE SET
                lease_token = excluded.lease_token,
                expires_at = excluded.expires_at
            WHERE mcp_oauth_refresh_leases.expires_at <= $4
            "#,
        )
        .bind(credential_binding_id.to_string())
        .bind(lease_token.to_string())
        .bind(expires_at.unix_timestamp())
        .bind(now.unix_timestamp())
        .execute(&self.pool)
        .await
        .map_err(to_write_error)?;
        Ok(result.rows_affected() > 0)
    }

    async fn release_mcp_oauth_refresh_lease(
        &self,
        credential_binding_id: Uuid,
        lease_token: Uuid,
    ) -> Result<bool, StoreError> {
        let result = sqlx::query(
            "DELETE FROM mcp_oauth_refresh_leases WHERE credential_binding_id = $1 AND lease_token = $2",
        )
        .bind(credential_binding_id.to_string())
        .bind(lease_token.to_string())
        .execute(&self.pool)
        .await
        .map_err(to_write_error)?;
        Ok(result.rows_affected() > 0)
    }

    async fn get_mcp_oauth_client(
        &self,
        mcp_server_id: Uuid,
    ) -> Result<Option<McpOauthClientRecord>, StoreError> {
        let sql = format!(
            "SELECT {OAUTH_CLIENT_COLUMNS} FROM mcp_oauth_clients WHERE mcp_server_id = $1"
        );
        sqlx::query(&sql)
            .bind(mcp_server_id.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(to_query_error)?
            .as_ref()
            .map(decode_oauth_client)
            .transpose()
    }

    async fn insert_mcp_oauth_client_if_absent(
        &self,
        record: &McpOauthClientRecord,
    ) -> Result<McpOauthClientRecord, StoreError> {
        sqlx::query(
            r#"
            INSERT INTO mcp_oauth_clients (
                mcp_server_id, issuer, redirect_uri, client_id, token_endpoint_auth_method,
                secret_ciphertext, secret_nonce, secret_key_id, client_secret_expires_at,
                created_at, updated_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            ON CONFLICT(mcp_server_id) DO NOTHING
            "#,
        )
        .bind(record.mcp_server_id.to_string())
        .bind(&record.issuer)
        .bind(&record.redirect_uri)
        .bind(&record.client_id)
        .bind(&record.token_endpoint_auth_method)
        .bind(&record.secret_ciphertext)
        .bind(&record.secret_nonce)
        .bind(&record.secret_key_id)
        .bind(
            record
                .client_secret_expires_at
                .map(OffsetDateTime::unix_timestamp),
        )
        .bind(record.created_at.unix_timestamp())
        .bind(record.updated_at.unix_timestamp())
        .execute(&self.pool)
        .await
        .map_err(to_write_error)?;
        self.get_mcp_oauth_client(record.mcp_server_id)
            .await?
            .ok_or_else(|| {
                StoreError::Unexpected(format!(
                    "MCP OAuth client for server `{}` was not stored",
                    record.mcp_server_id
                ))
            })
    }

    async fn delete_mcp_oauth_client(
        &self,
        mcp_server_id: Uuid,
        client_id: &str,
    ) -> Result<bool, StoreError> {
        let result = sqlx::query(
            "DELETE FROM mcp_oauth_clients WHERE mcp_server_id = $1 AND client_id = $2",
        )
        .bind(mcp_server_id.to_string())
        .bind(client_id)
        .execute(&self.pool)
        .await
        .map_err(to_write_error)?;
        Ok(result.rows_affected() > 0)
    }
}
