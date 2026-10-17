//! The gateway's OAuth authorization server for MCP clients.
use super::*;
use crate::shared::{parse_uuid, unix_to_datetime};
use gateway_core::{
    GatewayOauthAuthorizationDecision, GatewayOauthAuthorizationRequestRecord,
    GatewayOauthAuthorizationStatus, GatewayOauthClientAuthMethod, GatewayOauthClientRecord,
    GatewayOauthGrantRecord, GatewayOauthGrantSummary, GatewayOauthPurgeCounts,
    GatewayOauthRepository, GatewayOauthSessionState, GatewayOauthTokenContext,
    GatewayOauthTokenKind, GatewayOauthTokenRecord,
};

const CLIENT_COLUMNS: &str = "client_id, registration_kind, client_name, client_uri, logo_uri, \
    redirect_uris_json, grant_types_json, token_endpoint_auth_method, client_secret_hash, \
    software_id, software_version, created_at, updated_at";
const REQUEST_COLUMNS: &str = "request_id, client_id, redirect_uri, state, code_challenge, \
    resource, scope, status, user_id, user_session_id, code_hash, code_expires_at, grant_id, \
    expires_at, created_at, decided_at, consumed_at, redirect_uri_supplied";

fn decode_json_list(raw: &str, field: &str) -> Result<Vec<String>, StoreError> {
    serde_json::from_str(raw)
        .map_err(|error| StoreError::Serialization(format!("invalid {field}: {error}")))
}

fn encode_json_list(values: &[String]) -> Result<String, StoreError> {
    serde_json::to_string(values).map_err(|error| StoreError::Serialization(error.to_string()))
}

fn optional_uuid(value: Option<String>) -> Result<Option<Uuid>, StoreError> {
    value.as_deref().map(parse_uuid).transpose()
}

fn optional_time(value: Option<i64>) -> Result<Option<OffsetDateTime>, StoreError> {
    value.map(unix_to_datetime).transpose()
}

fn decode_client(row: &libsql::Row) -> Result<GatewayOauthClientRecord, StoreError> {
    let redirect_uris: String = row.get(5).map_err(to_query_error)?;
    let grant_types: String = row.get(6).map_err(to_query_error)?;
    let auth_method: String = row.get(7).map_err(to_query_error)?;
    Ok(GatewayOauthClientRecord {
        client_id: row.get(0).map_err(to_query_error)?,
        registration_kind: row.get(1).map_err(to_query_error)?,
        client_name: row.get(2).map_err(to_query_error)?,
        client_uri: row.get(3).map_err(to_query_error)?,
        logo_uri: row.get(4).map_err(to_query_error)?,
        redirect_uris: decode_json_list(&redirect_uris, "redirect_uris_json")?,
        grant_types: decode_json_list(&grant_types, "grant_types_json")?,
        token_endpoint_auth_method: GatewayOauthClientAuthMethod::from_db(&auth_method)
            .ok_or_else(|| {
                StoreError::Serialization(format!("invalid OAuth client auth `{auth_method}`"))
            })?,
        client_secret_hash: row.get(8).map_err(to_query_error)?,
        software_id: row.get(9).map_err(to_query_error)?,
        software_version: row.get(10).map_err(to_query_error)?,
        created_at: unix_to_datetime(row.get(11).map_err(to_query_error)?)?,
        updated_at: unix_to_datetime(row.get(12).map_err(to_query_error)?)?,
    })
}

fn decode_request(row: &libsql::Row) -> Result<GatewayOauthAuthorizationRequestRecord, StoreError> {
    let request_id: String = row.get(0).map_err(to_query_error)?;
    let status: String = row.get(7).map_err(to_query_error)?;
    Ok(GatewayOauthAuthorizationRequestRecord {
        request_id: parse_uuid(&request_id)?,
        client_id: row.get(1).map_err(to_query_error)?,
        redirect_uri: row.get(2).map_err(to_query_error)?,
        state: row.get(3).map_err(to_query_error)?,
        code_challenge: row.get(4).map_err(to_query_error)?,
        resource: row.get(5).map_err(to_query_error)?,
        scope: row.get(6).map_err(to_query_error)?,
        status: GatewayOauthAuthorizationStatus::from_db(&status).ok_or_else(|| {
            StoreError::Serialization(format!("invalid OAuth authorization status `{status}`"))
        })?,
        user_id: optional_uuid(row.get(8).map_err(to_query_error)?)?,
        user_session_id: optional_uuid(row.get(9).map_err(to_query_error)?)?,
        code_hash: row.get(10).map_err(to_query_error)?,
        code_expires_at: optional_time(row.get(11).map_err(to_query_error)?)?,
        grant_id: optional_uuid(row.get(12).map_err(to_query_error)?)?,
        expires_at: unix_to_datetime(row.get(13).map_err(to_query_error)?)?,
        created_at: unix_to_datetime(row.get(14).map_err(to_query_error)?)?,
        decided_at: optional_time(row.get(15).map_err(to_query_error)?)?,
        consumed_at: optional_time(row.get(16).map_err(to_query_error)?)?,
        redirect_uri_supplied: row.get::<i64>(17).map_err(to_query_error)? == 1,
    })
}

/// Decodes the token, grant, session, and user columns of the context query.
fn decode_token_context(row: &libsql::Row) -> Result<GatewayOauthTokenContext, StoreError> {
    let grant_id: String = row.get(1).map_err(to_query_error)?;
    let kind: String = row.get(2).map_err(to_query_error)?;
    let user_id: String = row.get(7).map_err(to_query_error)?;
    let user_session_id: String = row.get(8).map_err(to_query_error)?;
    let user_status: String = row.get(17).map_err(to_query_error)?;
    let grant_id = parse_uuid(&grant_id)?;
    Ok(GatewayOauthTokenContext {
        token: GatewayOauthTokenRecord {
            token_hash: row.get(0).map_err(to_query_error)?,
            grant_id,
            kind: GatewayOauthTokenKind::from_db(&kind).ok_or_else(|| {
                StoreError::Serialization(format!("invalid OAuth token kind `{kind}`"))
            })?,
            expires_at: unix_to_datetime(row.get(3).map_err(to_query_error)?)?,
            created_at: unix_to_datetime(row.get(4).map_err(to_query_error)?)?,
            consumed_at: optional_time(row.get(5).map_err(to_query_error)?)?,
            scope: row.get(18).map_err(to_query_error)?,
        },
        grant: GatewayOauthGrantRecord {
            grant_id,
            client_id: row.get(6).map_err(to_query_error)?,
            user_id: parse_uuid(&user_id)?,
            user_session_id: parse_uuid(&user_session_id)?,
            resource: row.get(9).map_err(to_query_error)?,
            scope: row.get(10).map_err(to_query_error)?,
            created_at: unix_to_datetime(row.get(11).map_err(to_query_error)?)?,
            updated_at: unix_to_datetime(row.get(12).map_err(to_query_error)?)?,
            revoked_at: optional_time(row.get(13).map_err(to_query_error)?)?,
            revoked_reason: row.get(14).map_err(to_query_error)?,
        },
        session_expires_at: unix_to_datetime(row.get(15).map_err(to_query_error)?)?,
        session_revoked_at: optional_time(row.get(16).map_err(to_query_error)?)?,
        user_status: UserStatus::from_db(&user_status).ok_or_else(|| {
            StoreError::Serialization(format!("invalid user status `{user_status}`"))
        })?,
    })
}

fn decode_grant(row: &libsql::Row) -> Result<GatewayOauthGrantRecord, StoreError> {
    let grant_id: String = row.get(0).map_err(to_query_error)?;
    let user_id: String = row.get(2).map_err(to_query_error)?;
    let user_session_id: String = row.get(3).map_err(to_query_error)?;
    Ok(GatewayOauthGrantRecord {
        grant_id: parse_uuid(&grant_id)?,
        client_id: row.get(1).map_err(to_query_error)?,
        user_id: parse_uuid(&user_id)?,
        user_session_id: parse_uuid(&user_session_id)?,
        resource: row.get(4).map_err(to_query_error)?,
        scope: row.get(5).map_err(to_query_error)?,
        created_at: unix_to_datetime(row.get(6).map_err(to_query_error)?)?,
        updated_at: unix_to_datetime(row.get(7).map_err(to_query_error)?)?,
        revoked_at: optional_time(row.get(8).map_err(to_query_error)?)?,
        revoked_reason: row.get(9).map_err(to_query_error)?,
    })
}

fn decode_session_state(row: &libsql::Row) -> Result<GatewayOauthSessionState, StoreError> {
    let session_id: String = row.get(0).map_err(to_query_error)?;
    let user_id: String = row.get(1).map_err(to_query_error)?;
    let user_status: String = row.get(4).map_err(to_query_error)?;
    Ok(GatewayOauthSessionState {
        session_id: parse_uuid(&session_id)?,
        user_id: parse_uuid(&user_id)?,
        expires_at: unix_to_datetime(row.get(2).map_err(to_query_error)?)?,
        revoked_at: optional_time(row.get(3).map_err(to_query_error)?)?,
        user_status: UserStatus::from_db(&user_status).ok_or_else(|| {
            StoreError::Serialization(format!("invalid user status `{user_status}`"))
        })?,
    })
}

fn decode_grant_summary(row: &libsql::Row) -> Result<GatewayOauthGrantSummary, StoreError> {
    let grant_id: String = row.get(0).map_err(to_query_error)?;
    Ok(GatewayOauthGrantSummary {
        grant_id: parse_uuid(&grant_id)?,
        client_id: row.get(1).map_err(to_query_error)?,
        client_name: row.get(2).map_err(to_query_error)?,
        client_uri: row.get(3).map_err(to_query_error)?,
        resource: row.get(4).map_err(to_query_error)?,
        scope: row.get(5).map_err(to_query_error)?,
        created_at: unix_to_datetime(row.get(6).map_err(to_query_error)?)?,
        last_token_issued_at: unix_to_datetime(row.get(7).map_err(to_query_error)?)?,
        expires_at: unix_to_datetime(row.get(8).map_err(to_query_error)?)?,
    })
}

async fn query_one<T>(
    connection: &libsql::Connection,
    sql: &str,
    params: impl libsql::params::IntoParams,
    decode: fn(&libsql::Row) -> Result<T, StoreError>,
) -> Result<Option<T>, StoreError> {
    let mut rows = connection
        .query(sql, params)
        .await
        .map_err(to_query_error)?;
    rows.next()
        .await
        .map_err(to_query_error)?
        .map(|row| decode(&row))
        .transpose()
}

#[async_trait]
impl GatewayOauthRepository for LibsqlStore {
    async fn create_gateway_oauth_client(
        &self,
        client: &GatewayOauthClientRecord,
    ) -> Result<(), StoreError> {
        let sql = format!(
            "INSERT INTO gateway_oauth_clients ({CLIENT_COLUMNS})
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)"
        );
        self.connection
            .execute(
                &sql,
                libsql::params![
                    client.client_id.as_str(),
                    client.registration_kind.as_str(),
                    client.client_name.clone(),
                    client.client_uri.clone(),
                    client.logo_uri.clone(),
                    encode_json_list(&client.redirect_uris)?,
                    encode_json_list(&client.grant_types)?,
                    client.token_endpoint_auth_method.as_str(),
                    client.client_secret_hash.clone(),
                    client.software_id.clone(),
                    client.software_version.clone(),
                    client.created_at.unix_timestamp(),
                    client.updated_at.unix_timestamp(),
                ],
            )
            .await
            .map_err(to_write_error)?;
        Ok(())
    }

    async fn get_gateway_oauth_client(
        &self,
        client_id: &str,
    ) -> Result<Option<GatewayOauthClientRecord>, StoreError> {
        let sql =
            format!("SELECT {CLIENT_COLUMNS} FROM gateway_oauth_clients WHERE client_id = ?1");
        query_one(&self.connection, &sql, [client_id], decode_client).await
    }

    async fn upsert_gateway_oauth_metadata_document_client(
        &self,
        client: &GatewayOauthClientRecord,
    ) -> Result<GatewayOauthClientRecord, StoreError> {
        let sql = format!(
            "INSERT INTO gateway_oauth_clients ({CLIENT_COLUMNS})
             VALUES (?1, 'metadata_document', ?2, ?3, ?4, ?5, ?6, 'none', NULL, ?7, ?8, ?9, ?9)
             ON CONFLICT (client_id) DO UPDATE SET
               client_name = excluded.client_name,
               client_uri = excluded.client_uri,
               logo_uri = excluded.logo_uri,
               redirect_uris_json = excluded.redirect_uris_json,
               grant_types_json = excluded.grant_types_json,
               software_id = excluded.software_id,
               software_version = excluded.software_version,
               updated_at = excluded.updated_at
             WHERE gateway_oauth_clients.registration_kind = 'metadata_document'
             RETURNING {CLIENT_COLUMNS}"
        );
        query_one(
            &self.connection,
            &sql,
            libsql::params![
                client.client_id.as_str(),
                client.client_name.clone(),
                client.client_uri.clone(),
                client.logo_uri.clone(),
                encode_json_list(&client.redirect_uris)?,
                encode_json_list(&client.grant_types)?,
                client.software_id.clone(),
                client.software_version.clone(),
                client.updated_at.unix_timestamp(),
            ],
            decode_client,
        )
        .await?
        .ok_or_else(|| {
            StoreError::Conflict("client_id belongs to a dynamically registered client".into())
        })
    }

    async fn create_gateway_oauth_authorization_request(
        &self,
        request: &GatewayOauthAuthorizationRequestRecord,
    ) -> Result<(), StoreError> {
        let sql = format!(
            "INSERT INTO gateway_oauth_authorization_requests ({REQUEST_COLUMNS})
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)"
        );
        self.connection
            .execute(
                &sql,
                libsql::params![
                    request.request_id.to_string(),
                    request.client_id.as_str(),
                    request.redirect_uri.as_str(),
                    request.state.clone(),
                    request.code_challenge.as_str(),
                    request.resource.as_str(),
                    request.scope.as_str(),
                    request.status.as_str(),
                    request.user_id.map(|value| value.to_string()),
                    request.user_session_id.map(|value| value.to_string()),
                    request.code_hash.clone(),
                    request.code_expires_at.map(OffsetDateTime::unix_timestamp),
                    request.grant_id.map(|value| value.to_string()),
                    request.expires_at.unix_timestamp(),
                    request.created_at.unix_timestamp(),
                    request.decided_at.map(OffsetDateTime::unix_timestamp),
                    request.consumed_at.map(OffsetDateTime::unix_timestamp),
                    i64::from(request.redirect_uri_supplied),
                ],
            )
            .await
            .map_err(to_write_error)?;
        Ok(())
    }

    async fn get_gateway_oauth_authorization_request(
        &self,
        request_id: Uuid,
    ) -> Result<Option<GatewayOauthAuthorizationRequestRecord>, StoreError> {
        let sql = format!(
            "SELECT {REQUEST_COLUMNS} FROM gateway_oauth_authorization_requests WHERE request_id = ?1"
        );
        query_one(
            &self.connection,
            &sql,
            [request_id.to_string()],
            decode_request,
        )
        .await
    }

    async fn get_gateway_oauth_authorization_request_by_code_hash(
        &self,
        code_hash: &str,
    ) -> Result<Option<GatewayOauthAuthorizationRequestRecord>, StoreError> {
        let sql = format!(
            "SELECT {REQUEST_COLUMNS} FROM gateway_oauth_authorization_requests WHERE code_hash = ?1"
        );
        query_one(&self.connection, &sql, [code_hash], decode_request).await
    }

    async fn decide_gateway_oauth_authorization_request(
        &self,
        request_id: Uuid,
        decision: &GatewayOauthAuthorizationDecision,
        decided_at: OffsetDateTime,
    ) -> Result<bool, StoreError> {
        let (status, user_id, session_id, code_hash, code_expires_at) = match decision {
            GatewayOauthAuthorizationDecision::Approved {
                user_id,
                user_session_id,
                code_hash,
                code_expires_at,
            } => (
                GatewayOauthAuthorizationStatus::Approved,
                *user_id,
                Some(user_session_id.to_string()),
                Some(code_hash.clone()),
                Some(code_expires_at.unix_timestamp()),
            ),
            GatewayOauthAuthorizationDecision::Denied { user_id } => (
                GatewayOauthAuthorizationStatus::Denied,
                *user_id,
                None,
                None,
                None,
            ),
        };
        let changed = self
            .connection
            .execute(
                "UPDATE gateway_oauth_authorization_requests
                 SET status = ?1, user_id = ?2, user_session_id = ?3, code_hash = ?4,
                     code_expires_at = ?5, decided_at = ?6
                 WHERE request_id = ?7 AND status = 'pending' AND expires_at > ?6",
                libsql::params![
                    status.as_str(),
                    user_id.to_string(),
                    session_id,
                    code_hash,
                    code_expires_at,
                    decided_at.unix_timestamp(),
                    request_id.to_string(),
                ],
            )
            .await
            .map_err(to_write_error)?;
        Ok(changed > 0)
    }

    async fn consume_gateway_oauth_authorization_code(
        &self,
        request_id: Uuid,
        grant_id: Uuid,
        consumed_at: OffsetDateTime,
    ) -> Result<bool, StoreError> {
        let changed = self
            .connection
            .execute(
                "UPDATE gateway_oauth_authorization_requests
                 SET status = 'consumed', grant_id = ?1, consumed_at = ?2
                 WHERE request_id = ?3 AND status = 'approved'",
                libsql::params![
                    grant_id.to_string(),
                    consumed_at.unix_timestamp(),
                    request_id.to_string(),
                ],
            )
            .await
            .map_err(to_write_error)?;
        Ok(changed > 0)
    }

    async fn create_gateway_oauth_grant(
        &self,
        grant: &GatewayOauthGrantRecord,
    ) -> Result<bool, StoreError> {
        // SQLite serialises writes, so with sign-out revoking the session
        // before its grants, this check cannot miss a revocation.
        let inserted = self
            .connection
            .execute(
                "INSERT INTO gateway_oauth_grants (
                    grant_id, client_id, user_id, user_session_id, resource, scope,
                    created_at, updated_at, revoked_at, revoked_reason, session_expires_at
                 )
                 SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, s.expires_at
                 FROM user_sessions s
                 WHERE s.session_id = ?4 AND s.revoked_at IS NULL",
                libsql::params![
                    grant.grant_id.to_string(),
                    grant.client_id.as_str(),
                    grant.user_id.to_string(),
                    grant.user_session_id.to_string(),
                    grant.resource.as_str(),
                    grant.scope.as_str(),
                    grant.created_at.unix_timestamp(),
                    grant.updated_at.unix_timestamp(),
                    grant.revoked_at.map(OffsetDateTime::unix_timestamp),
                    grant.revoked_reason.clone(),
                ],
            )
            .await
            .map_err(to_write_error)?;
        Ok(inserted > 0)
    }

    async fn create_gateway_oauth_tokens(
        &self,
        tokens: &[GatewayOauthTokenRecord],
    ) -> Result<(), StoreError> {
        let tx = self.begin_transaction().await?;
        for token in tokens {
            tx.execute(
                "INSERT INTO gateway_oauth_tokens (
                    token_hash, grant_id, token_kind, scope, expires_at, created_at, consumed_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                libsql::params![
                    token.token_hash.as_str(),
                    token.grant_id.to_string(),
                    token.kind.as_str(),
                    token.scope.as_str(),
                    token.expires_at.unix_timestamp(),
                    token.created_at.unix_timestamp(),
                    token.consumed_at.map(OffsetDateTime::unix_timestamp),
                ],
            )
            .await
            .map_err(to_write_error)?;
        }
        tx.commit().await.map_err(to_write_error)?;
        Ok(())
    }

    async fn get_gateway_oauth_token_context(
        &self,
        token_hash: &str,
    ) -> Result<Option<GatewayOauthTokenContext>, StoreError> {
        query_one(
            &self.connection,
            "SELECT t.token_hash, t.grant_id, t.token_kind, t.expires_at, t.created_at,
                    t.consumed_at, g.client_id, g.user_id, g.user_session_id, g.resource,
                    g.scope, g.created_at, g.updated_at, g.revoked_at, g.revoked_reason,
                    s.expires_at, s.revoked_at, u.status, t.scope
             FROM gateway_oauth_tokens t
             JOIN gateway_oauth_grants g ON g.grant_id = t.grant_id
             JOIN user_sessions s ON s.session_id = g.user_session_id
             JOIN users u ON u.user_id = g.user_id
             WHERE t.token_hash = ?1",
            [token_hash],
            decode_token_context,
        )
        .await
    }

    async fn live_gateway_oauth_access_tokens(
        &self,
        token_hashes: &[String],
        now: OffsetDateTime,
    ) -> Result<Vec<String>, StoreError> {
        let mut live = Vec::new();
        // Bound SQL parameters however many streams are open.
        for hashes in token_hashes.chunks(500) {
            let placeholders = vec!["?"; hashes.len()].join(", ");
            let query = format!(
                "SELECT t.token_hash
                 FROM gateway_oauth_tokens t
                 JOIN gateway_oauth_grants g ON g.grant_id = t.grant_id
                 JOIN user_sessions s ON s.session_id = g.user_session_id
                 JOIN users u ON u.user_id = g.user_id
                 WHERE t.token_hash IN ({placeholders})
                   AND t.token_kind = 'access' AND t.expires_at > {now}
                   AND g.revoked_at IS NULL
                   AND s.revoked_at IS NULL AND s.expires_at > {now}
                   AND u.status = 'active'",
                now = now.unix_timestamp(),
            );
            let params = hashes
                .iter()
                .map(|hash| libsql::Value::Text(hash.clone()))
                .collect::<Vec<_>>();
            let mut rows = self
                .connection
                .query(&query, params)
                .await
                .map_err(to_query_error)?;
            while let Some(row) = rows.next().await.map_err(to_query_error)? {
                live.push(row.get::<String>(0).map_err(to_query_error)?);
            }
        }
        Ok(live)
    }

    async fn get_gateway_oauth_session_state(
        &self,
        user_session_id: Uuid,
    ) -> Result<Option<GatewayOauthSessionState>, StoreError> {
        query_one(
            &self.connection,
            "SELECT s.session_id, s.user_id, s.expires_at, s.revoked_at, u.status
             FROM user_sessions s
             JOIN users u ON u.user_id = s.user_id
             WHERE s.session_id = ?1",
            [user_session_id.to_string()],
            decode_session_state,
        )
        .await
    }

    async fn consume_gateway_oauth_refresh_token(
        &self,
        token_hash: &str,
        consumed_at: OffsetDateTime,
    ) -> Result<bool, StoreError> {
        let changed = self
            .connection
            .execute(
                "UPDATE gateway_oauth_tokens SET consumed_at = ?1
                 WHERE token_hash = ?2 AND token_kind = 'refresh' AND consumed_at IS NULL",
                libsql::params![consumed_at.unix_timestamp(), token_hash],
            )
            .await
            .map_err(to_write_error)?;
        Ok(changed > 0)
    }

    async fn revoke_gateway_oauth_grant(
        &self,
        grant_id: Uuid,
        reason: &str,
        revoked_at: OffsetDateTime,
    ) -> Result<bool, StoreError> {
        let changed = self
            .connection
            .execute(
                "UPDATE gateway_oauth_grants
                 SET revoked_at = ?1, revoked_reason = ?2, updated_at = ?1
                 WHERE grant_id = ?3 AND revoked_at IS NULL",
                libsql::params![revoked_at.unix_timestamp(), reason, grant_id.to_string()],
            )
            .await
            .map_err(to_write_error)?;
        Ok(changed > 0)
    }

    async fn revoke_gateway_oauth_grants_for_user(
        &self,
        user_id: Uuid,
        reason: &str,
        revoked_at: OffsetDateTime,
    ) -> Result<u64, StoreError> {
        self.connection
            .execute(
                "UPDATE gateway_oauth_grants
                 SET revoked_at = ?1, revoked_reason = ?2, updated_at = ?1
                 WHERE user_id = ?3 AND revoked_at IS NULL",
                libsql::params![revoked_at.unix_timestamp(), reason, user_id.to_string()],
            )
            .await
            .map_err(to_write_error)
    }

    async fn list_live_gateway_oauth_grants_for_user(
        &self,
        user_id: Uuid,
        aggregate_resource: Option<&str>,
        now: OffsetDateTime,
        limit: u32,
    ) -> Result<Vec<GatewayOauthGrantSummary>, StoreError> {
        let mut rows = self
            .connection
            .query(
                "SELECT g.grant_id, g.client_id, c.client_name, c.client_uri, g.resource, g.scope,
                    g.created_at,
                    (SELECT MAX(t.created_at) FROM gateway_oauth_tokens t
                     WHERE t.grant_id = g.grant_id) AS last_issued_at,
                    (SELECT MAX(t.expires_at) FROM gateway_oauth_tokens t
                     WHERE t.grant_id = g.grant_id AND t.consumed_at IS NULL) AS usable_until
             FROM (
                 SELECT lg.grant_id
                 FROM gateway_oauth_grants lg
                 JOIN user_sessions s ON s.session_id = lg.user_session_id
                 JOIN users u ON u.user_id = lg.user_id
                 WHERE lg.user_id = ?1 AND lg.revoked_at IS NULL
                   AND (?4 IS NULL OR lg.resource = ?4
                        OR substr(lg.resource, 1, length(?4) + 1) = ?4 || '/')
                   AND s.revoked_at IS NULL AND s.expires_at > ?2
                   AND u.status = 'active'
                   AND EXISTS (
                       SELECT 1 FROM gateway_oauth_tokens lt
                       WHERE lt.grant_id = lg.grant_id
                         AND lt.consumed_at IS NULL AND lt.expires_at > ?2
                   )
                 ORDER BY lg.created_at DESC, lg.grant_id
                 LIMIT ?3
             ) page
             JOIN gateway_oauth_grants g ON g.grant_id = page.grant_id
             JOIN gateway_oauth_clients c ON c.client_id = g.client_id
             ORDER BY g.created_at DESC, g.grant_id",
                libsql::params![
                    user_id.to_string(),
                    now.unix_timestamp(),
                    i64::from(limit),
                    aggregate_resource
                ],
            )
            .await
            .map_err(to_query_error)?;
        let mut grants = Vec::new();
        while let Some(row) = rows.next().await.map_err(to_query_error)? {
            grants.push(decode_grant_summary(&row)?);
        }
        Ok(grants)
    }

    async fn revoke_gateway_oauth_grant_for_user(
        &self,
        grant_id: Uuid,
        user_id: Uuid,
        reason: &str,
        revoked_at: OffsetDateTime,
    ) -> Result<Option<GatewayOauthGrantRecord>, StoreError> {
        query_one(
            &self.connection,
            "UPDATE gateway_oauth_grants
             SET revoked_at = ?1, revoked_reason = ?2, updated_at = ?1
             WHERE grant_id = ?3 AND user_id = ?4 AND revoked_at IS NULL
             RETURNING grant_id, client_id, user_id, user_session_id, resource, scope, created_at, updated_at, revoked_at, revoked_reason",
            libsql::params![
                revoked_at.unix_timestamp(),
                reason,
                grant_id.to_string(),
                user_id.to_string()
            ],
            decode_grant,
        )
        .await
    }

    async fn purge_gateway_oauth_records(
        &self,
        unused_clients_before: OffsetDateTime,
        expired_before: OffsetDateTime,
        retire_before: OffsetDateTime,
    ) -> Result<GatewayOauthPurgeCounts, StoreError> {
        let expired = expired_before.unix_timestamp();
        let retired_grants = self
            .purge_in_batches(RETIRE_ENDED_GRANTS, retire_before.unix_timestamp())
            .await?;
        let authorization_requests = self
            .purge_in_batches(PURGE_AUTHORIZATION_REQUESTS, expired)
            .await?;
        let tokens = self.purge_in_batches(PURGE_TOKENS, expired).await?;
        let clients = self
            .purge_in_batches(PURGE_UNUSED_CLIENTS, unused_clients_before.unix_timestamp())
            .await?;
        Ok(GatewayOauthPurgeCounts {
            retired_grants,
            clients,
            authorization_requests,
            tokens,
        })
    }
}

impl LibsqlStore {
    /// Repeats a cleanup statement until a batch comes back short, or for at
    /// most `PURGE_MAX_BATCHES` rounds; the next run continues a backlog.
    async fn purge_in_batches(&self, sql: &str, cutoff: i64) -> Result<u64, StoreError> {
        let mut total = 0;
        for _ in 0..PURGE_MAX_BATCHES {
            let affected = self
                .connection
                .execute(sql, libsql::params![cutoff, PURGE_BATCH_SIZE])
                .await
                .map_err(to_write_error)?;
            total += affected;
            if affected < PURGE_BATCH_SIZE.unsigned_abs() {
                break;
            }
        }
        Ok(total)
    }
}

/// Cleanup statements for `purge_gateway_oauth_records`. Each takes the
/// cutoff as `?1` and touches at most `?2` rows.
const RETIRE_ENDED_GRANTS: &str = "UPDATE gateway_oauth_grants
     SET revoked_at = session_expires_at,
         updated_at = session_expires_at,
         revoked_reason = 'session_ended'
     WHERE revoked_at IS NULL
       AND grant_id IN (
         SELECT grant_id FROM gateway_oauth_grants
         WHERE revoked_at IS NULL AND session_expires_at < ?1
         LIMIT ?2)";
const PURGE_AUTHORIZATION_REQUESTS: &str = "DELETE FROM gateway_oauth_authorization_requests
     WHERE request_id IN (
         SELECT r.request_id FROM gateway_oauth_authorization_requests r
         WHERE r.expires_at < ?1 AND (r.code_expires_at IS NULL OR r.code_expires_at < ?1)
           AND NOT EXISTS (SELECT 1 FROM gateway_oauth_grants g
                           JOIN user_sessions s ON s.session_id = g.user_session_id
                           WHERE g.grant_id = r.grant_id
                             AND g.revoked_at IS NULL
                             AND s.revoked_at IS NULL AND s.expires_at >= ?1)
         LIMIT ?2)";
const PURGE_TOKENS: &str = "DELETE FROM gateway_oauth_tokens
     WHERE token_hash IN (
         SELECT t.token_hash FROM gateway_oauth_tokens t
         WHERE t.expires_at < ?1
           -- Only a consumed refresh token is a replay marker; other expired
           -- tokens can go even while their grant lives.
           AND (t.token_kind <> 'refresh' OR t.consumed_at IS NULL
                OR NOT EXISTS (SELECT 1 FROM gateway_oauth_grants g
                           JOIN user_sessions s ON s.session_id = g.user_session_id
                           WHERE g.grant_id = t.grant_id
                             AND g.revoked_at IS NULL
                             AND s.revoked_at IS NULL AND s.expires_at >= ?1))
         LIMIT ?2)";
const PURGE_UNUSED_CLIENTS: &str = "DELETE FROM gateway_oauth_clients
     WHERE client_id IN (
         SELECT c.client_id FROM gateway_oauth_clients c
         -- A metadata document row is refreshed on every fetch, so its age is
         -- its last fetch: a row a process may still cache is never purged.
         WHERE ((c.registration_kind = 'dynamic' AND c.created_at < ?1)
                OR (c.registration_kind = 'metadata_document' AND c.updated_at < ?1))
           AND NOT EXISTS (SELECT 1 FROM gateway_oauth_grants g WHERE g.client_id = c.client_id)
           AND NOT EXISTS (SELECT 1 FROM gateway_oauth_authorization_requests r
                           WHERE r.client_id = c.client_id)
         LIMIT ?2)";

/// Rows each cleanup statement touches per round trip.
#[cfg(not(test))]
const PURGE_BATCH_SIZE: i64 = 1000;
/// Small in tests, so the shared store tests run several batches.
#[cfg(test)]
const PURGE_BATCH_SIZE: i64 = 2;
/// Rounds each cleanup statement gets per run, bounding one run's work to
/// 50,000 rows a step. A larger backlog drains over the following hours.
#[cfg(not(test))]
const PURGE_MAX_BATCHES: usize = 50;
/// One round in tests, so the store tests see a backlog carried over.
#[cfg(test)]
const PURGE_MAX_BATCHES: usize = 1;
