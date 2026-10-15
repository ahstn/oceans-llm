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

fn decode_client(row: &PgRow) -> Result<GatewayOauthClientRecord, StoreError> {
    let redirect_uris: String = row.try_get(5).map_err(to_query_error)?;
    let grant_types: String = row.try_get(6).map_err(to_query_error)?;
    let auth_method: String = row.try_get(7).map_err(to_query_error)?;
    Ok(GatewayOauthClientRecord {
        client_id: row.try_get(0).map_err(to_query_error)?,
        registration_kind: row.try_get(1).map_err(to_query_error)?,
        client_name: row.try_get(2).map_err(to_query_error)?,
        client_uri: row.try_get(3).map_err(to_query_error)?,
        logo_uri: row.try_get(4).map_err(to_query_error)?,
        redirect_uris: decode_json_list(&redirect_uris, "redirect_uris_json")?,
        grant_types: decode_json_list(&grant_types, "grant_types_json")?,
        token_endpoint_auth_method: GatewayOauthClientAuthMethod::from_db(&auth_method)
            .ok_or_else(|| {
                StoreError::Serialization(format!("invalid OAuth client auth `{auth_method}`"))
            })?,
        client_secret_hash: row.try_get(8).map_err(to_query_error)?,
        software_id: row.try_get(9).map_err(to_query_error)?,
        software_version: row.try_get(10).map_err(to_query_error)?,
        created_at: unix_to_datetime(row.try_get(11).map_err(to_query_error)?)?,
        updated_at: unix_to_datetime(row.try_get(12).map_err(to_query_error)?)?,
    })
}

fn decode_request(row: &PgRow) -> Result<GatewayOauthAuthorizationRequestRecord, StoreError> {
    let request_id: String = row.try_get(0).map_err(to_query_error)?;
    let status: String = row.try_get(7).map_err(to_query_error)?;
    Ok(GatewayOauthAuthorizationRequestRecord {
        request_id: parse_uuid(&request_id)?,
        client_id: row.try_get(1).map_err(to_query_error)?,
        redirect_uri: row.try_get(2).map_err(to_query_error)?,
        state: row.try_get(3).map_err(to_query_error)?,
        code_challenge: row.try_get(4).map_err(to_query_error)?,
        resource: row.try_get(5).map_err(to_query_error)?,
        scope: row.try_get(6).map_err(to_query_error)?,
        status: GatewayOauthAuthorizationStatus::from_db(&status).ok_or_else(|| {
            StoreError::Serialization(format!("invalid OAuth authorization status `{status}`"))
        })?,
        user_id: optional_uuid(row.try_get(8).map_err(to_query_error)?)?,
        user_session_id: optional_uuid(row.try_get(9).map_err(to_query_error)?)?,
        code_hash: row.try_get(10).map_err(to_query_error)?,
        code_expires_at: optional_time(row.try_get(11).map_err(to_query_error)?)?,
        grant_id: optional_uuid(row.try_get(12).map_err(to_query_error)?)?,
        expires_at: unix_to_datetime(row.try_get(13).map_err(to_query_error)?)?,
        created_at: unix_to_datetime(row.try_get(14).map_err(to_query_error)?)?,
        decided_at: optional_time(row.try_get(15).map_err(to_query_error)?)?,
        consumed_at: optional_time(row.try_get(16).map_err(to_query_error)?)?,
        redirect_uri_supplied: row.try_get::<i64, _>(17).map_err(to_query_error)? == 1,
    })
}

/// Decodes the token, grant, session, and user columns of the context query.
fn decode_token_context(row: &PgRow) -> Result<GatewayOauthTokenContext, StoreError> {
    let grant_id: String = row.try_get(1).map_err(to_query_error)?;
    let kind: String = row.try_get(2).map_err(to_query_error)?;
    let user_id: String = row.try_get(7).map_err(to_query_error)?;
    let user_session_id: String = row.try_get(8).map_err(to_query_error)?;
    let user_status: String = row.try_get(17).map_err(to_query_error)?;
    let grant_id = parse_uuid(&grant_id)?;
    Ok(GatewayOauthTokenContext {
        token: GatewayOauthTokenRecord {
            token_hash: row.try_get(0).map_err(to_query_error)?,
            grant_id,
            kind: GatewayOauthTokenKind::from_db(&kind).ok_or_else(|| {
                StoreError::Serialization(format!("invalid OAuth token kind `{kind}`"))
            })?,
            expires_at: unix_to_datetime(row.try_get(3).map_err(to_query_error)?)?,
            created_at: unix_to_datetime(row.try_get(4).map_err(to_query_error)?)?,
            consumed_at: optional_time(row.try_get(5).map_err(to_query_error)?)?,
            scope: row.try_get(18).map_err(to_query_error)?,
        },
        grant: GatewayOauthGrantRecord {
            grant_id,
            client_id: row.try_get(6).map_err(to_query_error)?,
            user_id: parse_uuid(&user_id)?,
            user_session_id: parse_uuid(&user_session_id)?,
            resource: row.try_get(9).map_err(to_query_error)?,
            scope: row.try_get(10).map_err(to_query_error)?,
            created_at: unix_to_datetime(row.try_get(11).map_err(to_query_error)?)?,
            updated_at: unix_to_datetime(row.try_get(12).map_err(to_query_error)?)?,
            revoked_at: optional_time(row.try_get(13).map_err(to_query_error)?)?,
            revoked_reason: row.try_get(14).map_err(to_query_error)?,
        },
        session_expires_at: unix_to_datetime(row.try_get(15).map_err(to_query_error)?)?,
        session_revoked_at: optional_time(row.try_get(16).map_err(to_query_error)?)?,
        user_status: UserStatus::from_db(&user_status).ok_or_else(|| {
            StoreError::Serialization(format!("invalid user status `{user_status}`"))
        })?,
    })
}

fn decode_session_state(row: &PgRow) -> Result<GatewayOauthSessionState, StoreError> {
    let session_id: String = row.try_get(0).map_err(to_query_error)?;
    let user_id: String = row.try_get(1).map_err(to_query_error)?;
    let user_status: String = row.try_get(4).map_err(to_query_error)?;
    Ok(GatewayOauthSessionState {
        session_id: parse_uuid(&session_id)?,
        user_id: parse_uuid(&user_id)?,
        expires_at: unix_to_datetime(row.try_get(2).map_err(to_query_error)?)?,
        revoked_at: optional_time(row.try_get(3).map_err(to_query_error)?)?,
        user_status: UserStatus::from_db(&user_status).ok_or_else(|| {
            StoreError::Serialization(format!("invalid user status `{user_status}`"))
        })?,
    })
}

fn decode_grant_summary(row: &PgRow) -> Result<GatewayOauthGrantSummary, StoreError> {
    let grant_id: String = row.try_get(0).map_err(to_query_error)?;
    Ok(GatewayOauthGrantSummary {
        grant_id: parse_uuid(&grant_id)?,
        client_id: row.try_get(1).map_err(to_query_error)?,
        client_name: row.try_get(2).map_err(to_query_error)?,
        client_uri: row.try_get(3).map_err(to_query_error)?,
        resource: row.try_get(4).map_err(to_query_error)?,
        scope: row.try_get(5).map_err(to_query_error)?,
        created_at: unix_to_datetime(row.try_get(6).map_err(to_query_error)?)?,
        last_token_issued_at: unix_to_datetime(row.try_get(7).map_err(to_query_error)?)?,
        expires_at: unix_to_datetime(row.try_get(8).map_err(to_query_error)?)?,
    })
}

#[async_trait]
impl GatewayOauthRepository for PostgresStore {
    async fn create_gateway_oauth_client(
        &self,
        client: &GatewayOauthClientRecord,
    ) -> Result<(), StoreError> {
        let sql = format!(
            "INSERT INTO gateway_oauth_clients ({CLIENT_COLUMNS})
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)"
        );
        sqlx::query(&sql)
            .bind(&client.client_id)
            .bind(&client.registration_kind)
            .bind(&client.client_name)
            .bind(&client.client_uri)
            .bind(&client.logo_uri)
            .bind(encode_json_list(&client.redirect_uris)?)
            .bind(encode_json_list(&client.grant_types)?)
            .bind(client.token_endpoint_auth_method.as_str())
            .bind(&client.client_secret_hash)
            .bind(&client.software_id)
            .bind(&client.software_version)
            .bind(client.created_at.unix_timestamp())
            .bind(client.updated_at.unix_timestamp())
            .execute(&self.pool)
            .await
            .map_err(to_write_error)?;
        Ok(())
    }

    async fn get_gateway_oauth_client(
        &self,
        client_id: &str,
    ) -> Result<Option<GatewayOauthClientRecord>, StoreError> {
        let sql =
            format!("SELECT {CLIENT_COLUMNS} FROM gateway_oauth_clients WHERE client_id = $1");
        let row = sqlx::query(&sql)
            .bind(client_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(to_query_error)?;
        row.as_ref().map(decode_client).transpose()
    }

    async fn create_gateway_oauth_authorization_request(
        &self,
        request: &GatewayOauthAuthorizationRequestRecord,
    ) -> Result<(), StoreError> {
        let sql = format!(
            "INSERT INTO gateway_oauth_authorization_requests ({REQUEST_COLUMNS})
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18)"
        );
        sqlx::query(&sql)
            .bind(request.request_id.to_string())
            .bind(&request.client_id)
            .bind(&request.redirect_uri)
            .bind(&request.state)
            .bind(&request.code_challenge)
            .bind(&request.resource)
            .bind(&request.scope)
            .bind(request.status.as_str())
            .bind(request.user_id.map(|value| value.to_string()))
            .bind(request.user_session_id.map(|value| value.to_string()))
            .bind(&request.code_hash)
            .bind(request.code_expires_at.map(OffsetDateTime::unix_timestamp))
            .bind(request.grant_id.map(|value| value.to_string()))
            .bind(request.expires_at.unix_timestamp())
            .bind(request.created_at.unix_timestamp())
            .bind(request.decided_at.map(OffsetDateTime::unix_timestamp))
            .bind(request.consumed_at.map(OffsetDateTime::unix_timestamp))
            .bind(i64::from(request.redirect_uri_supplied))
            .execute(&self.pool)
            .await
            .map_err(to_write_error)?;
        Ok(())
    }

    async fn get_gateway_oauth_authorization_request(
        &self,
        request_id: Uuid,
    ) -> Result<Option<GatewayOauthAuthorizationRequestRecord>, StoreError> {
        let sql = format!(
            "SELECT {REQUEST_COLUMNS} FROM gateway_oauth_authorization_requests WHERE request_id = $1"
        );
        let row = sqlx::query(&sql)
            .bind(request_id.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(to_query_error)?;
        row.as_ref().map(decode_request).transpose()
    }

    async fn get_gateway_oauth_authorization_request_by_code_hash(
        &self,
        code_hash: &str,
    ) -> Result<Option<GatewayOauthAuthorizationRequestRecord>, StoreError> {
        let sql = format!(
            "SELECT {REQUEST_COLUMNS} FROM gateway_oauth_authorization_requests WHERE code_hash = $1"
        );
        let row = sqlx::query(&sql)
            .bind(code_hash)
            .fetch_optional(&self.pool)
            .await
            .map_err(to_query_error)?;
        row.as_ref().map(decode_request).transpose()
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
        let result = sqlx::query(
            "UPDATE gateway_oauth_authorization_requests
             SET status = $1, user_id = $2, user_session_id = $3, code_hash = $4,
                 code_expires_at = $5, decided_at = $6
             WHERE request_id = $7 AND status = 'pending' AND expires_at > $6",
        )
        .bind(status.as_str())
        .bind(user_id.to_string())
        .bind(session_id)
        .bind(code_hash)
        .bind(code_expires_at)
        .bind(decided_at.unix_timestamp())
        .bind(request_id.to_string())
        .execute(&self.pool)
        .await
        .map_err(to_write_error)?;
        Ok(result.rows_affected() > 0)
    }

    async fn consume_gateway_oauth_authorization_code(
        &self,
        request_id: Uuid,
        grant_id: Uuid,
        consumed_at: OffsetDateTime,
    ) -> Result<bool, StoreError> {
        let result = sqlx::query(
            "UPDATE gateway_oauth_authorization_requests
             SET status = 'consumed', grant_id = $1, consumed_at = $2
             WHERE request_id = $3 AND status = 'approved'",
        )
        .bind(grant_id.to_string())
        .bind(consumed_at.unix_timestamp())
        .bind(request_id.to_string())
        .execute(&self.pool)
        .await
        .map_err(to_write_error)?;
        Ok(result.rows_affected() > 0)
    }

    async fn create_gateway_oauth_grant(
        &self,
        grant: &GatewayOauthGrantRecord,
    ) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO gateway_oauth_grants (
                grant_id, client_id, user_id, user_session_id, resource, scope,
                created_at, updated_at, revoked_at, revoked_reason
             ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        )
        .bind(grant.grant_id.to_string())
        .bind(&grant.client_id)
        .bind(grant.user_id.to_string())
        .bind(grant.user_session_id.to_string())
        .bind(&grant.resource)
        .bind(&grant.scope)
        .bind(grant.created_at.unix_timestamp())
        .bind(grant.updated_at.unix_timestamp())
        .bind(grant.revoked_at.map(OffsetDateTime::unix_timestamp))
        .bind(&grant.revoked_reason)
        .execute(&self.pool)
        .await
        .map_err(to_write_error)?;
        Ok(())
    }

    async fn create_gateway_oauth_token(
        &self,
        token: &GatewayOauthTokenRecord,
    ) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO gateway_oauth_tokens (
                token_hash, grant_id, token_kind, scope, expires_at, created_at, consumed_at
             ) VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(&token.token_hash)
        .bind(token.grant_id.to_string())
        .bind(token.kind.as_str())
        .bind(&token.scope)
        .bind(token.expires_at.unix_timestamp())
        .bind(token.created_at.unix_timestamp())
        .bind(token.consumed_at.map(OffsetDateTime::unix_timestamp))
        .execute(&self.pool)
        .await
        .map_err(to_write_error)?;
        Ok(())
    }

    async fn get_gateway_oauth_token_context(
        &self,
        token_hash: &str,
    ) -> Result<Option<GatewayOauthTokenContext>, StoreError> {
        let row = sqlx::query(
            "SELECT t.token_hash, t.grant_id, t.token_kind, t.expires_at, t.created_at,
                    t.consumed_at, g.client_id, g.user_id, g.user_session_id, g.resource,
                    g.scope, g.created_at, g.updated_at, g.revoked_at, g.revoked_reason,
                    s.expires_at, s.revoked_at, u.status, t.scope
             FROM gateway_oauth_tokens t
             JOIN gateway_oauth_grants g ON g.grant_id = t.grant_id
             JOIN user_sessions s ON s.session_id = g.user_session_id
             JOIN users u ON u.user_id = g.user_id
             WHERE t.token_hash = $1",
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(to_query_error)?;
        row.as_ref().map(decode_token_context).transpose()
    }

    async fn get_gateway_oauth_session_state(
        &self,
        user_session_id: Uuid,
    ) -> Result<Option<GatewayOauthSessionState>, StoreError> {
        let row = sqlx::query(
            "SELECT s.session_id, s.user_id, s.expires_at, s.revoked_at, u.status
             FROM user_sessions s
             JOIN users u ON u.user_id = s.user_id
             WHERE s.session_id = $1",
        )
        .bind(user_session_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(to_query_error)?;
        row.as_ref().map(decode_session_state).transpose()
    }

    async fn consume_gateway_oauth_refresh_token(
        &self,
        token_hash: &str,
        consumed_at: OffsetDateTime,
    ) -> Result<bool, StoreError> {
        let result = sqlx::query(
            "UPDATE gateway_oauth_tokens SET consumed_at = $1
             WHERE token_hash = $2 AND token_kind = 'refresh' AND consumed_at IS NULL",
        )
        .bind(consumed_at.unix_timestamp())
        .bind(token_hash)
        .execute(&self.pool)
        .await
        .map_err(to_write_error)?;
        Ok(result.rows_affected() > 0)
    }

    async fn revoke_gateway_oauth_grant(
        &self,
        grant_id: Uuid,
        reason: &str,
        revoked_at: OffsetDateTime,
    ) -> Result<bool, StoreError> {
        let result = sqlx::query(
            "UPDATE gateway_oauth_grants
             SET revoked_at = $1, revoked_reason = $2, updated_at = $1
             WHERE grant_id = $3 AND revoked_at IS NULL",
        )
        .bind(revoked_at.unix_timestamp())
        .bind(reason)
        .bind(grant_id.to_string())
        .execute(&self.pool)
        .await
        .map_err(to_write_error)?;
        Ok(result.rows_affected() > 0)
    }

    async fn revoke_gateway_oauth_grants_for_user(
        &self,
        user_id: Uuid,
        reason: &str,
        revoked_at: OffsetDateTime,
    ) -> Result<u64, StoreError> {
        let result = sqlx::query(
            "UPDATE gateway_oauth_grants
             SET revoked_at = $1, revoked_reason = $2, updated_at = $1
             WHERE user_id = $3 AND revoked_at IS NULL",
        )
        .bind(revoked_at.unix_timestamp())
        .bind(reason)
        .bind(user_id.to_string())
        .execute(&self.pool)
        .await
        .map_err(to_write_error)?;
        Ok(result.rows_affected())
    }
    async fn list_live_gateway_oauth_grants_for_user(
        &self,
        user_id: Uuid,
        aggregate_resource: Option<&str>,
        now: OffsetDateTime,
        limit: u32,
    ) -> Result<Vec<GatewayOauthGrantSummary>, StoreError> {
        let rows = sqlx::query(
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
                 WHERE lg.user_id = $1 AND lg.revoked_at IS NULL
                   AND ($4 IS NULL OR lg.resource = $4
                        OR substr(lg.resource, 1, length($4) + 1) = $4 || '/')
                   AND s.revoked_at IS NULL AND s.expires_at > $2
                   AND u.status = 'active'
                   AND EXISTS (
                       SELECT 1 FROM gateway_oauth_tokens lt
                       WHERE lt.grant_id = lg.grant_id
                         AND lt.consumed_at IS NULL AND lt.expires_at > $2
                   )
                 ORDER BY lg.created_at DESC, lg.grant_id
                 LIMIT $3
             ) page
             JOIN gateway_oauth_grants g ON g.grant_id = page.grant_id
             JOIN gateway_oauth_clients c ON c.client_id = g.client_id
             ORDER BY g.created_at DESC, g.grant_id",
        )
        .bind(user_id.to_string())
        .bind(now.unix_timestamp())
        .bind(i64::from(limit))
        .bind(aggregate_resource)
        .fetch_all(&self.pool)
        .await
        .map_err(to_query_error)?;
        rows.iter().map(decode_grant_summary).collect()
    }

    async fn revoke_gateway_oauth_grant_for_user(
        &self,
        grant_id: Uuid,
        user_id: Uuid,
        reason: &str,
        revoked_at: OffsetDateTime,
    ) -> Result<bool, StoreError> {
        let result = sqlx::query(
            "UPDATE gateway_oauth_grants
             SET revoked_at = $1, revoked_reason = $2, updated_at = $1
             WHERE grant_id = $3 AND user_id = $4 AND revoked_at IS NULL",
        )
        .bind(revoked_at.unix_timestamp())
        .bind(reason)
        .bind(grant_id.to_string())
        .bind(user_id.to_string())
        .execute(&self.pool)
        .await
        .map_err(to_write_error)?;
        Ok(result.rows_affected() > 0)
    }

    async fn purge_gateway_oauth_records(
        &self,
        unused_clients_before: OffsetDateTime,
        expired_before: OffsetDateTime,
    ) -> Result<GatewayOauthPurgeCounts, StoreError> {
        let authorization_requests = sqlx::query(
            "DELETE FROM gateway_oauth_authorization_requests
             WHERE expires_at < $1 AND (code_expires_at IS NULL OR code_expires_at < $1)
               AND NOT EXISTS (SELECT 1 FROM gateway_oauth_grants g
                               JOIN user_sessions s ON s.session_id = g.user_session_id
                               WHERE g.grant_id = gateway_oauth_authorization_requests.grant_id
                                 AND g.revoked_at IS NULL
                                 AND s.revoked_at IS NULL AND s.expires_at >= $1)",
        )
        .bind(expired_before.unix_timestamp())
        .execute(&self.pool)
        .await
        .map_err(to_write_error)?
        .rows_affected();
        let tokens = sqlx::query(
            "DELETE FROM gateway_oauth_tokens
             WHERE expires_at < $1
               AND NOT EXISTS (SELECT 1 FROM gateway_oauth_grants g
                               JOIN user_sessions s ON s.session_id = g.user_session_id
                               WHERE g.grant_id = gateway_oauth_tokens.grant_id
                                 AND g.revoked_at IS NULL
                                 AND s.revoked_at IS NULL AND s.expires_at >= $1)",
        )
        .bind(expired_before.unix_timestamp())
        .execute(&self.pool)
        .await
        .map_err(to_write_error)?
        .rows_affected();
        let clients = sqlx::query(
            "DELETE FROM gateway_oauth_clients
             WHERE registration_kind = 'dynamic' AND created_at < $1
               AND NOT EXISTS (SELECT 1 FROM gateway_oauth_grants g
                               WHERE g.client_id = gateway_oauth_clients.client_id)
               AND NOT EXISTS (SELECT 1 FROM gateway_oauth_authorization_requests r
                               WHERE r.client_id = gateway_oauth_clients.client_id)",
        )
        .bind(unused_clients_before.unix_timestamp())
        .execute(&self.pool)
        .await
        .map_err(to_write_error)?
        .rows_affected();
        Ok(GatewayOauthPurgeCounts {
            clients,
            authorization_requests,
            tokens,
        })
    }
}
