//! The gateway's OAuth authorization server for MCP clients.
use super::*;
use crate::shared::{parse_uuid, unix_to_datetime};
use gateway_core::{
    GatewayOauthAuthorizationDecision, GatewayOauthAuthorizationRequestRecord,
    GatewayOauthAuthorizationStatus, GatewayOauthClientAuthMethod, GatewayOauthClientRecord,
    GatewayOauthGrantRecord, GatewayOauthRepository, GatewayOauthSessionState,
    GatewayOauthTokenContext, GatewayOauthTokenKind, GatewayOauthTokenRecord,
};

const CLIENT_COLUMNS: &str = "client_id, registration_kind, client_name, client_uri, logo_uri, \
    redirect_uris_json, grant_types_json, token_endpoint_auth_method, client_secret_hash, \
    software_id, software_version, created_at, updated_at";
const REQUEST_COLUMNS: &str = "request_id, client_id, redirect_uri, state, code_challenge, \
    resource, scope, status, user_id, user_session_id, code_hash, code_expires_at, grant_id, \
    expires_at, created_at, decided_at, consumed_at";

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

    async fn create_gateway_oauth_authorization_request(
        &self,
        request: &GatewayOauthAuthorizationRequestRecord,
    ) -> Result<(), StoreError> {
        let sql = format!(
            "INSERT INTO gateway_oauth_authorization_requests ({REQUEST_COLUMNS})
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)"
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
    ) -> Result<(), StoreError> {
        self.connection
            .execute(
                "INSERT INTO gateway_oauth_grants (
                    grant_id, client_id, user_id, user_session_id, resource, scope,
                    created_at, updated_at, revoked_at, revoked_reason
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
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
        Ok(())
    }

    async fn create_gateway_oauth_token(
        &self,
        token: &GatewayOauthTokenRecord,
    ) -> Result<(), StoreError> {
        self.connection
            .execute(
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
}
