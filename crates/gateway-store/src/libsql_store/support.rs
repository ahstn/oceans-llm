use super::*;
use crate::shared::{json_object_from_str, parse_uuid, unix_to_datetime};

pub(super) async fn list_allowed_model_keys(
    connection: &libsql::Connection,
    sql: &str,
    owner_id: String,
) -> Result<Vec<String>, StoreError> {
    let mut rows = connection
        .query(sql, [owner_id])
        .await
        .map_err(|error| StoreError::Query(error.to_string()))?;

    let mut model_keys = Vec::new();
    while let Some(row) = rows
        .next()
        .await
        .map_err(|error| StoreError::Query(error.to_string()))?
    {
        let model_key: String = row.get(0).map_err(to_query_error)?;
        model_keys.push(model_key);
    }

    Ok(model_keys)
}

pub(super) fn decode_api_key(row: &libsql::Row) -> Result<ApiKeyRecord, StoreError> {
    let id: String = row.get(0).map_err(to_query_error)?;
    let owner_kind: String = row.get(5).map_err(to_query_error)?;
    let owner_user_id: Option<String> = row.get(6).map_err(to_query_error)?;
    let owner_team_id: Option<String> = row.get(7).map_err(to_query_error)?;
    let created_at: i64 = row.get(8).map_err(to_query_error)?;
    let last_used_at: Option<i64> = row.get(9).map_err(to_query_error)?;
    let revoked_at: Option<i64> = row.get(10).map_err(to_query_error)?;

    Ok(ApiKeyRecord {
        id: parse_uuid(&id)?,
        public_id: row.get(1).map_err(to_query_error)?,
        secret_hash: row.get(2).map_err(to_query_error)?,
        name: row.get(3).map_err(to_query_error)?,
        status: row.get(4).map_err(to_query_error)?,
        owner_kind: ApiKeyOwnerKind::from_db(&owner_kind).ok_or_else(|| {
            StoreError::Serialization(format!("unknown owner kind `{owner_kind}`"))
        })?,
        owner_user_id: owner_user_id.as_deref().map(parse_uuid).transpose()?,
        owner_team_id: owner_team_id.as_deref().map(parse_uuid).transpose()?,
        created_at: unix_to_datetime(created_at)?,
        last_used_at: last_used_at.map(unix_to_datetime).transpose()?,
        revoked_at: revoked_at.map(unix_to_datetime).transpose()?,
    })
}

pub(super) fn decode_gateway_model(row: &libsql::Row) -> Result<GatewayModel, StoreError> {
    let id: String = row.get(0).map_err(to_query_error)?;
    let tags_json: String = row.get(3).map_err(to_query_error)?;

    Ok(GatewayModel {
        id: parse_uuid(&id)?,
        model_key: row.get(1).map_err(to_query_error)?,
        description: row.get(2).map_err(to_query_error)?,
        tags: serde_json::from_str(&tags_json)
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
        rank: row.get(4).map_err(to_query_error)?,
    })
}

pub(super) fn decode_model_route(row: &libsql::Row) -> Result<ModelRoute, StoreError> {
    let id: String = row.get(0).map_err(to_query_error)?;
    let model_id: String = row.get(1).map_err(to_query_error)?;
    let enabled: i64 = row.get(6).map_err(to_query_error)?;
    let extra_headers_json: String = row.get(7).map_err(to_query_error)?;
    let extra_body_json: String = row.get(8).map_err(to_query_error)?;

    Ok(ModelRoute {
        id: parse_uuid(&id)?,
        model_id: parse_uuid(&model_id)?,
        provider_key: row.get(2).map_err(to_query_error)?,
        upstream_model: row.get(3).map_err(to_query_error)?,
        priority: row.get(4).map_err(to_query_error)?,
        weight: row.get(5).map_err(to_query_error)?,
        enabled: enabled == 1,
        extra_headers: json_object_from_str(&extra_headers_json)?,
        extra_body: json_object_from_str(&extra_body_json)?,
    })
}

pub(super) fn decode_provider_connection(
    row: &libsql::Row,
) -> Result<ProviderConnection, StoreError> {
    let config_json: String = row.get(2).map_err(to_query_error)?;
    let secrets_json: Option<String> = row.get(3).map_err(to_query_error)?;

    Ok(ProviderConnection {
        provider_key: row.get(0).map_err(to_query_error)?,
        provider_type: row.get(1).map_err(to_query_error)?,
        config: serde_json::from_str(&config_json)
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
        secrets: secrets_json
            .map(|value| {
                serde_json::from_str(&value)
                    .map_err(|error| StoreError::Serialization(error.to_string()))
            })
            .transpose()?,
    })
}

pub(super) fn decode_user_record(row: &libsql::Row) -> Result<UserRecord, StoreError> {
    let user_id: String = row.get(0).map_err(to_query_error)?;
    let global_role: String = row.get(4).map_err(to_query_error)?;
    let auth_mode: String = row.get(5).map_err(to_query_error)?;
    let must_change_password: i64 = row.get(7).map_err(to_query_error)?;
    let request_logging_enabled: i64 = row.get(8).map_err(to_query_error)?;
    let model_access_mode: String = row.get(9).map_err(to_query_error)?;
    let created_at: i64 = row.get(10).map_err(to_query_error)?;
    let updated_at: i64 = row.get(11).map_err(to_query_error)?;

    Ok(UserRecord {
        user_id: parse_uuid(&user_id)?,
        name: row.get(1).map_err(to_query_error)?,
        email: row.get(2).map_err(to_query_error)?,
        email_normalized: row.get(3).map_err(to_query_error)?,
        global_role: GlobalRole::from_db(&global_role).ok_or_else(|| {
            StoreError::Serialization(format!("unknown global role `{global_role}`"))
        })?,
        auth_mode: AuthMode::from_db(&auth_mode)
            .ok_or_else(|| StoreError::Serialization(format!("unknown auth mode `{auth_mode}`")))?,
        status: row.get(6).map_err(to_query_error)?,
        must_change_password: must_change_password == 1,
        request_logging_enabled: request_logging_enabled == 1,
        model_access_mode: ModelAccessMode::from_db(&model_access_mode).ok_or_else(|| {
            StoreError::Serialization(format!("unknown model access mode `{model_access_mode}`"))
        })?,
        created_at: unix_to_datetime(created_at)?,
        updated_at: unix_to_datetime(updated_at)?,
    })
}

pub(super) fn decode_identity_user_record(
    row: &libsql::Row,
) -> Result<IdentityUserRecord, StoreError> {
    let team_id: Option<String> = row.get(12).map_err(to_query_error)?;
    let membership_role: Option<String> = row.get(14).map_err(to_query_error)?;
    let membership_role = match membership_role {
        Some(role) => Some(MembershipRole::from_db(&role).ok_or_else(|| {
            StoreError::Serialization(format!("unknown membership role `{role}`"))
        })?),
        None => None,
    };

    Ok(IdentityUserRecord {
        user: decode_user_record(row)?,
        team_id: team_id.as_deref().map(parse_uuid).transpose()?,
        team_name: row.get(13).map_err(to_query_error)?,
        membership_role,
        oidc_provider_id: row.get(15).map_err(to_query_error)?,
        oidc_provider_key: row.get(16).map_err(to_query_error)?,
    })
}

pub(super) fn decode_user_password_auth_record(
    row: &libsql::Row,
) -> Result<UserPasswordAuthRecord, StoreError> {
    let user_id: String = row.get(0).map_err(to_query_error)?;
    let password_updated_at: i64 = row.get(2).map_err(to_query_error)?;

    Ok(UserPasswordAuthRecord {
        user_id: parse_uuid(&user_id)?,
        password_hash: row.get(1).map_err(to_query_error)?,
        password_updated_at: unix_to_datetime(password_updated_at)?,
    })
}

pub(super) fn decode_team_record(row: &libsql::Row) -> Result<TeamRecord, StoreError> {
    let team_id: String = row.get(0).map_err(to_query_error)?;
    let model_access_mode: String = row.get(4).map_err(to_query_error)?;
    let created_at: i64 = row.get(5).map_err(to_query_error)?;
    let updated_at: i64 = row.get(6).map_err(to_query_error)?;

    Ok(TeamRecord {
        team_id: parse_uuid(&team_id)?,
        team_key: row.get(1).map_err(to_query_error)?,
        team_name: row.get(2).map_err(to_query_error)?,
        status: row.get(3).map_err(to_query_error)?,
        model_access_mode: ModelAccessMode::from_db(&model_access_mode).ok_or_else(|| {
            StoreError::Serialization(format!("unknown model access mode `{model_access_mode}`"))
        })?,
        created_at: unix_to_datetime(created_at)?,
        updated_at: unix_to_datetime(updated_at)?,
    })
}

pub(super) fn decode_team_membership_record(
    row: &libsql::Row,
) -> Result<TeamMembershipRecord, StoreError> {
    let team_id: String = row.get(0).map_err(to_query_error)?;
    let user_id: String = row.get(1).map_err(to_query_error)?;
    let role: String = row.get(2).map_err(to_query_error)?;
    let created_at: i64 = row.get(3).map_err(to_query_error)?;
    let updated_at: i64 = row.get(4).map_err(to_query_error)?;

    Ok(TeamMembershipRecord {
        team_id: parse_uuid(&team_id)?,
        user_id: parse_uuid(&user_id)?,
        role: MembershipRole::from_db(&role).ok_or_else(|| {
            StoreError::Serialization(format!("unknown membership role `{role}`"))
        })?,
        created_at: unix_to_datetime(created_at)?,
        updated_at: unix_to_datetime(updated_at)?,
    })
}

pub(super) fn decode_oidc_provider_record(
    row: &libsql::Row,
) -> Result<OidcProviderRecord, StoreError> {
    let scopes_json: String = row.get(5).map_err(to_query_error)?;
    let enabled: i64 = row.get(6).map_err(to_query_error)?;
    let created_at: i64 = row.get(7).map_err(to_query_error)?;
    let updated_at: i64 = row.get(8).map_err(to_query_error)?;

    Ok(OidcProviderRecord {
        oidc_provider_id: row.get(0).map_err(to_query_error)?,
        provider_key: row.get(1).map_err(to_query_error)?,
        provider_type: row.get(2).map_err(to_query_error)?,
        issuer_url: row.get(3).map_err(to_query_error)?,
        client_id: row.get(4).map_err(to_query_error)?,
        scopes: serde_json::from_str(&scopes_json)
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
        enabled: enabled == 1,
        created_at: unix_to_datetime(created_at)?,
        updated_at: unix_to_datetime(updated_at)?,
    })
}

pub(super) fn decode_password_invitation_record(
    row: &libsql::Row,
) -> Result<PasswordInvitationRecord, StoreError> {
    let invitation_id: String = row.get(0).map_err(to_query_error)?;
    let user_id: String = row.get(1).map_err(to_query_error)?;
    let expires_at: i64 = row.get(3).map_err(to_query_error)?;
    let consumed_at: Option<i64> = row.get(4).map_err(to_query_error)?;
    let revoked_at: Option<i64> = row.get(5).map_err(to_query_error)?;
    let created_at: i64 = row.get(6).map_err(to_query_error)?;

    Ok(PasswordInvitationRecord {
        invitation_id: parse_uuid(&invitation_id)?,
        user_id: parse_uuid(&user_id)?,
        token_hash: row.get(2).map_err(to_query_error)?,
        expires_at: unix_to_datetime(expires_at)?,
        consumed_at: consumed_at.map(unix_to_datetime).transpose()?,
        revoked_at: revoked_at.map(unix_to_datetime).transpose()?,
        created_at: unix_to_datetime(created_at)?,
    })
}

pub(super) fn decode_user_session_record(
    row: &libsql::Row,
) -> Result<UserSessionRecord, StoreError> {
    let session_id: String = row.get(0).map_err(to_query_error)?;
    let user_id: String = row.get(1).map_err(to_query_error)?;
    let expires_at: i64 = row.get(3).map_err(to_query_error)?;
    let created_at: i64 = row.get(4).map_err(to_query_error)?;
    let last_seen_at: i64 = row.get(5).map_err(to_query_error)?;
    let revoked_at: Option<i64> = row.get(6).map_err(to_query_error)?;

    Ok(UserSessionRecord {
        session_id: parse_uuid(&session_id)?,
        user_id: parse_uuid(&user_id)?,
        token_hash: row.get(2).map_err(to_query_error)?,
        expires_at: unix_to_datetime(expires_at)?,
        created_at: unix_to_datetime(created_at)?,
        last_seen_at: unix_to_datetime(last_seen_at)?,
        revoked_at: revoked_at.map(unix_to_datetime).transpose()?,
    })
}

pub(super) fn decode_user_oidc_auth_record(
    row: &libsql::Row,
) -> Result<UserOidcAuthRecord, StoreError> {
    let user_id: String = row.get(0).map_err(to_query_error)?;
    let created_at: i64 = row.get(4).map_err(to_query_error)?;

    Ok(UserOidcAuthRecord {
        user_id: parse_uuid(&user_id)?,
        oidc_provider_id: row.get(1).map_err(to_query_error)?,
        subject: row.get(2).map_err(to_query_error)?,
        email_claim: row.get(3).map_err(to_query_error)?,
        created_at: unix_to_datetime(created_at)?,
    })
}

pub(super) fn decode_user_budget_record(row: &libsql::Row) -> Result<UserBudgetRecord, StoreError> {
    let user_budget_id: String = row.get(0).map_err(to_query_error)?;
    let user_id: String = row.get(1).map_err(to_query_error)?;
    let cadence: String = row.get(2).map_err(to_query_error)?;
    let amount_10000: i64 = row.get(3).map_err(to_query_error)?;
    let hard_limit: i64 = row.get(4).map_err(to_query_error)?;
    let is_active: i64 = row.get(6).map_err(to_query_error)?;
    let created_at: i64 = row.get(7).map_err(to_query_error)?;
    let updated_at: i64 = row.get(8).map_err(to_query_error)?;

    Ok(UserBudgetRecord {
        user_budget_id: parse_uuid(&user_budget_id)?,
        user_id: parse_uuid(&user_id)?,
        cadence: BudgetCadence::from_db(&cadence).ok_or_else(|| {
            StoreError::Serialization(format!("unknown budget cadence `{cadence}`"))
        })?,
        amount_usd: Money4::from_scaled(amount_10000),
        hard_limit: hard_limit == 1,
        timezone: row.get(5).map_err(to_query_error)?,
        is_active: is_active == 1,
        created_at: unix_to_datetime(created_at)?,
        updated_at: unix_to_datetime(updated_at)?,
    })
}

pub(super) fn to_query_error(error: libsql::Error) -> StoreError {
    StoreError::Query(error.to_string())
}

pub(super) fn to_write_error(error: libsql::Error) -> StoreError {
    let message = error.to_string();
    if message.contains("UNIQUE constraint failed")
        || message.contains("CHECK constraint failed")
        || message.contains("FOREIGN KEY constraint failed")
    {
        return StoreError::Conflict(message);
    }

    StoreError::Query(message)
}
