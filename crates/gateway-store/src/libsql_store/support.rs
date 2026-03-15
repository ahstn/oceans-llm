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
    let tags_json: String = row.get(4).map_err(to_query_error)?;

    Ok(GatewayModel {
        id: parse_uuid(&id)?,
        model_key: row.get(1).map_err(to_query_error)?,
        alias_target_model_key: row.get(2).map_err(to_query_error)?,
        description: row.get(3).map_err(to_query_error)?,
        tags: serde_json::from_str(&tags_json)
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
        rank: row.get(5).map_err(to_query_error)?,
    })
}

pub(super) fn decode_model_route(row: &libsql::Row) -> Result<ModelRoute, StoreError> {
    let id: String = row.get(0).map_err(to_query_error)?;
    let model_id: String = row.get(1).map_err(to_query_error)?;
    let enabled: i64 = row.get(6).map_err(to_query_error)?;
    let extra_headers_json: String = row.get(7).map_err(to_query_error)?;
    let extra_body_json: String = row.get(8).map_err(to_query_error)?;
    let capabilities_json: String = row.get(9).map_err(to_query_error)?;
    let capabilities = serde_json::from_str(&capabilities_json)
        .map_err(|error| StoreError::Serialization(error.to_string()))?;

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
        capabilities,
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

pub(super) fn decode_pricing_catalog_cache_record(
    row: &libsql::Row,
) -> Result<PricingCatalogCacheRecord, StoreError> {
    let fetched_at: i64 = row.get(3).map_err(to_query_error)?;
    Ok(PricingCatalogCacheRecord {
        catalog_key: row.get(0).map_err(to_query_error)?,
        source: row.get(1).map_err(to_query_error)?,
        etag: row.get(2).map_err(to_query_error)?,
        fetched_at: unix_to_datetime(fetched_at)?,
        snapshot_json: row.get(4).map_err(to_query_error)?,
    })
}

pub(super) fn decode_usage_ledger_record(
    row: &libsql::Row,
) -> Result<UsageLedgerRecord, StoreError> {
    let usage_event_id: String = row.get(0).map_err(to_query_error)?;
    let api_key_id: String = row.get(3).map_err(to_query_error)?;
    let user_id: Option<String> = row.get(4).map_err(to_query_error)?;
    let team_id: Option<String> = row.get(5).map_err(to_query_error)?;
    let actor_user_id: Option<String> = row.get(6).map_err(to_query_error)?;
    let model_id: Option<String> = row.get(7).map_err(to_query_error)?;
    let provider_usage_json: String = row.get(13).map_err(to_query_error)?;
    let pricing_status: String = row.get(14).map_err(to_query_error)?;
    let pricing_row_id: Option<String> = row.get(16).map_err(to_query_error)?;
    let pricing_source_fetched_at: Option<i64> = row.get(21).map_err(to_query_error)?;
    let input_cost_per_million_tokens_10000: Option<i64> = row.get(23).map_err(to_query_error)?;
    let output_cost_per_million_tokens_10000: Option<i64> = row.get(24).map_err(to_query_error)?;
    let computed_cost_10000: i64 = row.get(25).map_err(to_query_error)?;
    let occurred_at: i64 = row.get(26).map_err(to_query_error)?;

    Ok(UsageLedgerRecord {
        usage_event_id: parse_uuid(&usage_event_id)?,
        request_id: row.get(1).map_err(to_query_error)?,
        ownership_scope_key: row.get(2).map_err(to_query_error)?,
        api_key_id: parse_uuid(&api_key_id)?,
        user_id: user_id.as_deref().map(parse_uuid).transpose()?,
        team_id: team_id.as_deref().map(parse_uuid).transpose()?,
        actor_user_id: actor_user_id.as_deref().map(parse_uuid).transpose()?,
        model_id: model_id.as_deref().map(parse_uuid).transpose()?,
        provider_key: row.get(8).map_err(to_query_error)?,
        upstream_model: row.get(9).map_err(to_query_error)?,
        prompt_tokens: row.get(10).map_err(to_query_error)?,
        completion_tokens: row.get(11).map_err(to_query_error)?,
        total_tokens: row.get(12).map_err(to_query_error)?,
        provider_usage: serde_json::from_str(&provider_usage_json)
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
        pricing_status: UsagePricingStatus::from_db(&pricing_status).ok_or_else(|| {
            StoreError::Serialization(format!("unknown usage pricing status `{pricing_status}`"))
        })?,
        unpriced_reason: row.get(15).map_err(to_query_error)?,
        pricing_row_id: pricing_row_id.as_deref().map(parse_uuid).transpose()?,
        pricing_provider_id: row.get(17).map_err(to_query_error)?,
        pricing_model_id: row.get(18).map_err(to_query_error)?,
        pricing_source: row.get(19).map_err(to_query_error)?,
        pricing_source_etag: row.get(20).map_err(to_query_error)?,
        pricing_source_fetched_at: pricing_source_fetched_at
            .map(unix_to_datetime)
            .transpose()?,
        pricing_last_updated: row.get(22).map_err(to_query_error)?,
        input_cost_per_million_tokens: input_cost_per_million_tokens_10000.map(Money4::from_scaled),
        output_cost_per_million_tokens: output_cost_per_million_tokens_10000
            .map(Money4::from_scaled),
        computed_cost_usd: Money4::from_scaled(computed_cost_10000),
        occurred_at: unix_to_datetime(occurred_at)?,
    })
}

pub(super) fn decode_model_pricing_record(
    row: &libsql::Row,
) -> Result<ModelPricingRecord, StoreError> {
    let model_pricing_id: String = row.get(0).map_err(to_query_error)?;
    let input_cost_per_million_tokens_10000: Option<i64> = row.get(4).map_err(to_query_error)?;
    let output_cost_per_million_tokens_10000: Option<i64> = row.get(5).map_err(to_query_error)?;
    let cache_read_cost_per_million_tokens_10000: Option<i64> =
        row.get(6).map_err(to_query_error)?;
    let cache_write_cost_per_million_tokens_10000: Option<i64> =
        row.get(7).map_err(to_query_error)?;
    let input_audio_cost_per_million_tokens_10000: Option<i64> =
        row.get(8).map_err(to_query_error)?;
    let output_audio_cost_per_million_tokens_10000: Option<i64> =
        row.get(9).map_err(to_query_error)?;
    let effective_start_at: i64 = row.get(12).map_err(to_query_error)?;
    let effective_end_at: Option<i64> = row.get(13).map_err(to_query_error)?;
    let limits_json: String = row.get(14).map_err(to_query_error)?;
    let modalities_json: String = row.get(15).map_err(to_query_error)?;
    let provenance_fetched_at: i64 = row.get(18).map_err(to_query_error)?;
    let created_at: i64 = row.get(19).map_err(to_query_error)?;
    let updated_at: i64 = row.get(20).map_err(to_query_error)?;

    Ok(ModelPricingRecord {
        model_pricing_id: parse_uuid(&model_pricing_id)?,
        pricing_provider_id: row.get(1).map_err(to_query_error)?,
        pricing_model_id: row.get(2).map_err(to_query_error)?,
        display_name: row.get(3).map_err(to_query_error)?,
        input_cost_per_million_tokens: input_cost_per_million_tokens_10000.map(Money4::from_scaled),
        output_cost_per_million_tokens: output_cost_per_million_tokens_10000
            .map(Money4::from_scaled),
        cache_read_cost_per_million_tokens: cache_read_cost_per_million_tokens_10000
            .map(Money4::from_scaled),
        cache_write_cost_per_million_tokens: cache_write_cost_per_million_tokens_10000
            .map(Money4::from_scaled),
        input_audio_cost_per_million_tokens: input_audio_cost_per_million_tokens_10000
            .map(Money4::from_scaled),
        output_audio_cost_per_million_tokens: output_audio_cost_per_million_tokens_10000
            .map(Money4::from_scaled),
        release_date: row.get(10).map_err(to_query_error)?,
        last_updated: row.get(11).map_err(to_query_error)?,
        effective_start_at: unix_to_datetime(effective_start_at)?,
        effective_end_at: effective_end_at.map(unix_to_datetime).transpose()?,
        limits: serde_json::from_str::<PricingLimits>(&limits_json)
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
        modalities: serde_json::from_str::<PricingModalities>(&modalities_json)
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
        provenance: PricingProvenance {
            source: row.get(16).map_err(to_query_error)?,
            etag: row.get(17).map_err(to_query_error)?,
            fetched_at: unix_to_datetime(provenance_fetched_at)?,
        },
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
