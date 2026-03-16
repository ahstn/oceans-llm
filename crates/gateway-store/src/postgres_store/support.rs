use super::*;
use crate::shared::{json_object_from_str, parse_uuid, unix_to_datetime};

pub(super) async fn list_allowed_model_keys(
    pool: &PgPool,
    sql: &str,
    owner_id: String,
) -> Result<Vec<String>, StoreError> {
    let rows = sqlx::query(sql)
        .bind(owner_id)
        .fetch_all(pool)
        .await
        .map_err(to_query_error)?;

    rows.iter()
        .map(|row| row.try_get::<String, _>(0).map_err(to_query_error))
        .collect()
}

pub(super) fn decode_api_key(row: &PgRow) -> Result<ApiKeyRecord, StoreError> {
    let owner_kind: String = row.try_get(5).map_err(to_query_error)?;
    let owner_user_id: Option<String> = row.try_get(6).map_err(to_query_error)?;
    let owner_team_id: Option<String> = row.try_get(7).map_err(to_query_error)?;
    let created_at: i64 = row.try_get(8).map_err(to_query_error)?;
    let last_used_at: Option<i64> = row.try_get(9).map_err(to_query_error)?;
    let revoked_at: Option<i64> = row.try_get(10).map_err(to_query_error)?;

    Ok(ApiKeyRecord {
        id: parse_uuid(&row.try_get::<String, _>(0).map_err(to_query_error)?)?,
        public_id: row.try_get(1).map_err(to_query_error)?,
        secret_hash: row.try_get(2).map_err(to_query_error)?,
        name: row.try_get(3).map_err(to_query_error)?,
        status: row.try_get(4).map_err(to_query_error)?,
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

pub(super) fn decode_gateway_model(row: &PgRow) -> Result<GatewayModel, StoreError> {
    let tags_json: String = row.try_get(4).map_err(to_query_error)?;
    Ok(GatewayModel {
        id: parse_uuid(&row.try_get::<String, _>(0).map_err(to_query_error)?)?,
        model_key: row.try_get(1).map_err(to_query_error)?,
        alias_target_model_key: row.try_get(2).map_err(to_query_error)?,
        description: row.try_get(3).map_err(to_query_error)?,
        tags: serde_json::from_str(&tags_json)
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
        rank: row.try_get(5).map_err(to_query_error)?,
    })
}

pub(super) fn decode_model_route(row: &PgRow) -> Result<ModelRoute, StoreError> {
    let enabled: i64 = row.try_get(6).map_err(to_query_error)?;
    let extra_headers_json: String = row.try_get(7).map_err(to_query_error)?;
    let extra_body_json: String = row.try_get(8).map_err(to_query_error)?;
    let capabilities_json: String = row.try_get(9).map_err(to_query_error)?;
    let capabilities = serde_json::from_str(&capabilities_json)
        .map_err(|error| StoreError::Serialization(error.to_string()))?;

    Ok(ModelRoute {
        id: parse_uuid(&row.try_get::<String, _>(0).map_err(to_query_error)?)?,
        model_id: parse_uuid(&row.try_get::<String, _>(1).map_err(to_query_error)?)?,
        provider_key: row.try_get(2).map_err(to_query_error)?,
        upstream_model: row.try_get(3).map_err(to_query_error)?,
        priority: row.try_get(4).map_err(to_query_error)?,
        weight: row.try_get(5).map_err(to_query_error)?,
        enabled: enabled == 1,
        extra_headers: json_object_from_str(&extra_headers_json)?,
        extra_body: json_object_from_str(&extra_body_json)?,
        capabilities,
    })
}

pub(super) fn decode_provider_connection(row: &PgRow) -> Result<ProviderConnection, StoreError> {
    let config_json: String = row.try_get(2).map_err(to_query_error)?;
    let secrets_json: Option<String> = row.try_get(3).map_err(to_query_error)?;

    Ok(ProviderConnection {
        provider_key: row.try_get(0).map_err(to_query_error)?,
        provider_type: row.try_get(1).map_err(to_query_error)?,
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

pub(super) fn decode_user_record(row: &PgRow) -> Result<UserRecord, StoreError> {
    let global_role: String = row.try_get(4).map_err(to_query_error)?;
    let auth_mode: String = row.try_get(5).map_err(to_query_error)?;
    let must_change_password: i64 = row.try_get(7).map_err(to_query_error)?;
    let request_logging_enabled: i64 = row.try_get(8).map_err(to_query_error)?;
    let model_access_mode: String = row.try_get(9).map_err(to_query_error)?;
    let created_at: i64 = row.try_get(10).map_err(to_query_error)?;
    let updated_at: i64 = row.try_get(11).map_err(to_query_error)?;

    Ok(UserRecord {
        user_id: parse_uuid(&row.try_get::<String, _>(0).map_err(to_query_error)?)?,
        name: row.try_get(1).map_err(to_query_error)?,
        email: row.try_get(2).map_err(to_query_error)?,
        email_normalized: row.try_get(3).map_err(to_query_error)?,
        global_role: GlobalRole::from_db(&global_role).ok_or_else(|| {
            StoreError::Serialization(format!("unknown global role `{global_role}`"))
        })?,
        auth_mode: AuthMode::from_db(&auth_mode)
            .ok_or_else(|| StoreError::Serialization(format!("unknown auth mode `{auth_mode}`")))?,
        status: row.try_get(6).map_err(to_query_error)?,
        must_change_password: must_change_password == 1,
        request_logging_enabled: request_logging_enabled == 1,
        model_access_mode: ModelAccessMode::from_db(&model_access_mode).ok_or_else(|| {
            StoreError::Serialization(format!("unknown model access mode `{model_access_mode}`"))
        })?,
        created_at: unix_to_datetime(created_at)?,
        updated_at: unix_to_datetime(updated_at)?,
    })
}

pub(super) fn decode_identity_user_record(row: &PgRow) -> Result<IdentityUserRecord, StoreError> {
    let team_id: Option<String> = row.try_get(12).map_err(to_query_error)?;
    let membership_role_raw: Option<String> = row.try_get(14).map_err(to_query_error)?;
    let membership_role = membership_role_raw
        .as_deref()
        .map(|role| {
            MembershipRole::from_db(role).ok_or_else(|| {
                StoreError::Serialization(format!("unknown membership role `{role}`"))
            })
        })
        .transpose()?;

    Ok(IdentityUserRecord {
        user: decode_user_record(row)?,
        team_id: team_id.as_deref().map(parse_uuid).transpose()?,
        team_name: row.try_get(13).map_err(to_query_error)?,
        membership_role,
        oidc_provider_id: row.try_get(15).map_err(to_query_error)?,
        oidc_provider_key: row.try_get(16).map_err(to_query_error)?,
    })
}

pub(super) fn decode_user_password_auth_record(
    row: &PgRow,
) -> Result<UserPasswordAuthRecord, StoreError> {
    let password_updated_at: i64 = row.try_get(2).map_err(to_query_error)?;
    Ok(UserPasswordAuthRecord {
        user_id: parse_uuid(&row.try_get::<String, _>(0).map_err(to_query_error)?)?,
        password_hash: row.try_get(1).map_err(to_query_error)?,
        password_updated_at: unix_to_datetime(password_updated_at)?,
    })
}

pub(super) fn decode_team_record(row: &PgRow) -> Result<TeamRecord, StoreError> {
    let model_access_mode: String = row.try_get(4).map_err(to_query_error)?;
    let created_at: i64 = row.try_get(5).map_err(to_query_error)?;
    let updated_at: i64 = row.try_get(6).map_err(to_query_error)?;

    Ok(TeamRecord {
        team_id: parse_uuid(&row.try_get::<String, _>(0).map_err(to_query_error)?)?,
        team_key: row.try_get(1).map_err(to_query_error)?,
        team_name: row.try_get(2).map_err(to_query_error)?,
        status: row.try_get(3).map_err(to_query_error)?,
        model_access_mode: ModelAccessMode::from_db(&model_access_mode).ok_or_else(|| {
            StoreError::Serialization(format!("unknown model access mode `{model_access_mode}`"))
        })?,
        created_at: unix_to_datetime(created_at)?,
        updated_at: unix_to_datetime(updated_at)?,
    })
}

pub(super) fn decode_team_membership_record(
    row: &PgRow,
) -> Result<TeamMembershipRecord, StoreError> {
    let role: String = row.try_get(2).map_err(to_query_error)?;
    let created_at: i64 = row.try_get(3).map_err(to_query_error)?;
    let updated_at: i64 = row.try_get(4).map_err(to_query_error)?;

    Ok(TeamMembershipRecord {
        team_id: parse_uuid(&row.try_get::<String, _>(0).map_err(to_query_error)?)?,
        user_id: parse_uuid(&row.try_get::<String, _>(1).map_err(to_query_error)?)?,
        role: MembershipRole::from_db(&role).ok_or_else(|| {
            StoreError::Serialization(format!("unknown membership role `{role}`"))
        })?,
        created_at: unix_to_datetime(created_at)?,
        updated_at: unix_to_datetime(updated_at)?,
    })
}

pub(super) fn decode_oidc_provider_record(row: &PgRow) -> Result<OidcProviderRecord, StoreError> {
    let scopes_json: String = row.try_get(5).map_err(to_query_error)?;
    let enabled: i64 = row.try_get(6).map_err(to_query_error)?;
    let created_at: i64 = row.try_get(7).map_err(to_query_error)?;
    let updated_at: i64 = row.try_get(8).map_err(to_query_error)?;

    Ok(OidcProviderRecord {
        oidc_provider_id: row.try_get(0).map_err(to_query_error)?,
        provider_key: row.try_get(1).map_err(to_query_error)?,
        provider_type: row.try_get(2).map_err(to_query_error)?,
        issuer_url: row.try_get(3).map_err(to_query_error)?,
        client_id: row.try_get(4).map_err(to_query_error)?,
        scopes: serde_json::from_str(&scopes_json)
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
        enabled: enabled == 1,
        created_at: unix_to_datetime(created_at)?,
        updated_at: unix_to_datetime(updated_at)?,
    })
}

pub(super) fn decode_password_invitation_record(
    row: &PgRow,
) -> Result<PasswordInvitationRecord, StoreError> {
    let expires_at: i64 = row.try_get(3).map_err(to_query_error)?;
    let consumed_at: Option<i64> = row.try_get(4).map_err(to_query_error)?;
    let revoked_at: Option<i64> = row.try_get(5).map_err(to_query_error)?;
    let created_at: i64 = row.try_get(6).map_err(to_query_error)?;

    Ok(PasswordInvitationRecord {
        invitation_id: parse_uuid(&row.try_get::<String, _>(0).map_err(to_query_error)?)?,
        user_id: parse_uuid(&row.try_get::<String, _>(1).map_err(to_query_error)?)?,
        token_hash: row.try_get(2).map_err(to_query_error)?,
        expires_at: unix_to_datetime(expires_at)?,
        consumed_at: consumed_at.map(unix_to_datetime).transpose()?,
        revoked_at: revoked_at.map(unix_to_datetime).transpose()?,
        created_at: unix_to_datetime(created_at)?,
    })
}

pub(super) fn decode_user_session_record(row: &PgRow) -> Result<UserSessionRecord, StoreError> {
    let expires_at: i64 = row.try_get(3).map_err(to_query_error)?;
    let created_at: i64 = row.try_get(4).map_err(to_query_error)?;
    let last_seen_at: i64 = row.try_get(5).map_err(to_query_error)?;
    let revoked_at: Option<i64> = row.try_get(6).map_err(to_query_error)?;

    Ok(UserSessionRecord {
        session_id: parse_uuid(&row.try_get::<String, _>(0).map_err(to_query_error)?)?,
        user_id: parse_uuid(&row.try_get::<String, _>(1).map_err(to_query_error)?)?,
        token_hash: row.try_get(2).map_err(to_query_error)?,
        expires_at: unix_to_datetime(expires_at)?,
        created_at: unix_to_datetime(created_at)?,
        last_seen_at: unix_to_datetime(last_seen_at)?,
        revoked_at: revoked_at.map(unix_to_datetime).transpose()?,
    })
}

pub(super) fn decode_user_oidc_auth_record(row: &PgRow) -> Result<UserOidcAuthRecord, StoreError> {
    let created_at: i64 = row.try_get(4).map_err(to_query_error)?;
    Ok(UserOidcAuthRecord {
        user_id: parse_uuid(&row.try_get::<String, _>(0).map_err(to_query_error)?)?,
        oidc_provider_id: row.try_get(1).map_err(to_query_error)?,
        subject: row.try_get(2).map_err(to_query_error)?,
        email_claim: row.try_get(3).map_err(to_query_error)?,
        created_at: unix_to_datetime(created_at)?,
    })
}

pub(super) fn decode_user_budget_record(row: &PgRow) -> Result<UserBudgetRecord, StoreError> {
    let cadence: String = row.try_get(2).map_err(to_query_error)?;
    let amount_10000: i64 = row.try_get(3).map_err(to_query_error)?;
    let hard_limit: i64 = row.try_get(4).map_err(to_query_error)?;
    let is_active: i64 = row.try_get(6).map_err(to_query_error)?;
    let created_at: i64 = row.try_get(7).map_err(to_query_error)?;
    let updated_at: i64 = row.try_get(8).map_err(to_query_error)?;

    Ok(UserBudgetRecord {
        user_budget_id: parse_uuid(&row.try_get::<String, _>(0).map_err(to_query_error)?)?,
        user_id: parse_uuid(&row.try_get::<String, _>(1).map_err(to_query_error)?)?,
        cadence: BudgetCadence::from_db(&cadence).ok_or_else(|| {
            StoreError::Serialization(format!("unknown budget cadence `{cadence}`"))
        })?,
        amount_usd: Money4::from_scaled(amount_10000),
        hard_limit: hard_limit == 1,
        timezone: row.try_get(5).map_err(to_query_error)?,
        is_active: is_active == 1,
        created_at: unix_to_datetime(created_at)?,
        updated_at: unix_to_datetime(updated_at)?,
    })
}

pub(super) fn decode_team_budget_record(row: &PgRow) -> Result<TeamBudgetRecord, StoreError> {
    let cadence: String = row.try_get(2).map_err(to_query_error)?;
    let amount_10000: i64 = row.try_get(3).map_err(to_query_error)?;
    let hard_limit: i64 = row.try_get(4).map_err(to_query_error)?;
    let is_active: i64 = row.try_get(6).map_err(to_query_error)?;
    let created_at: i64 = row.try_get(7).map_err(to_query_error)?;
    let updated_at: i64 = row.try_get(8).map_err(to_query_error)?;

    Ok(TeamBudgetRecord {
        team_budget_id: parse_uuid(&row.try_get::<String, _>(0).map_err(to_query_error)?)?,
        team_id: parse_uuid(&row.try_get::<String, _>(1).map_err(to_query_error)?)?,
        cadence: BudgetCadence::from_db(&cadence).ok_or_else(|| {
            StoreError::Serialization(format!("unknown budget cadence `{cadence}`"))
        })?,
        amount_usd: Money4::from_scaled(amount_10000),
        hard_limit: hard_limit == 1,
        timezone: row.try_get(5).map_err(to_query_error)?,
        is_active: is_active == 1,
        created_at: unix_to_datetime(created_at)?,
        updated_at: unix_to_datetime(updated_at)?,
    })
}

pub(super) fn decode_pricing_catalog_cache_record(
    row: &PgRow,
) -> Result<PricingCatalogCacheRecord, StoreError> {
    let fetched_at: i64 = row.try_get(3).map_err(to_query_error)?;
    Ok(PricingCatalogCacheRecord {
        catalog_key: row.try_get(0).map_err(to_query_error)?,
        source: row.try_get(1).map_err(to_query_error)?,
        etag: row.try_get(2).map_err(to_query_error)?,
        fetched_at: unix_to_datetime(fetched_at)?,
        snapshot_json: row.try_get(4).map_err(to_query_error)?,
    })
}

pub(super) fn decode_usage_ledger_record(row: &PgRow) -> Result<UsageLedgerRecord, StoreError> {
    let usage_event_id = row.try_get::<String, _>(0).map_err(to_query_error)?;
    let api_key_id = row.try_get::<String, _>(3).map_err(to_query_error)?;
    let user_id: Option<String> = row.try_get(4).map_err(to_query_error)?;
    let team_id: Option<String> = row.try_get(5).map_err(to_query_error)?;
    let actor_user_id: Option<String> = row.try_get(6).map_err(to_query_error)?;
    let model_id: Option<String> = row.try_get(7).map_err(to_query_error)?;
    let provider_usage_json: String = row.try_get(13).map_err(to_query_error)?;
    let pricing_status: String = row.try_get(14).map_err(to_query_error)?;
    let pricing_row_id: Option<String> = row.try_get(16).map_err(to_query_error)?;
    let pricing_source_fetched_at: Option<i64> = row.try_get(21).map_err(to_query_error)?;
    let input_cost_per_million_tokens_10000: Option<i64> =
        row.try_get(23).map_err(to_query_error)?;
    let output_cost_per_million_tokens_10000: Option<i64> =
        row.try_get(24).map_err(to_query_error)?;
    let computed_cost_10000: i64 = row.try_get(25).map_err(to_query_error)?;
    let occurred_at: i64 = row.try_get(26).map_err(to_query_error)?;

    Ok(UsageLedgerRecord {
        usage_event_id: parse_uuid(&usage_event_id)?,
        request_id: row.try_get(1).map_err(to_query_error)?,
        ownership_scope_key: row.try_get(2).map_err(to_query_error)?,
        api_key_id: parse_uuid(&api_key_id)?,
        user_id: user_id.as_deref().map(parse_uuid).transpose()?,
        team_id: team_id.as_deref().map(parse_uuid).transpose()?,
        actor_user_id: actor_user_id.as_deref().map(parse_uuid).transpose()?,
        model_id: model_id.as_deref().map(parse_uuid).transpose()?,
        provider_key: row.try_get(8).map_err(to_query_error)?,
        upstream_model: row.try_get(9).map_err(to_query_error)?,
        prompt_tokens: row.try_get(10).map_err(to_query_error)?,
        completion_tokens: row.try_get(11).map_err(to_query_error)?,
        total_tokens: row.try_get(12).map_err(to_query_error)?,
        provider_usage: serde_json::from_str(&provider_usage_json)
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
        pricing_status: UsagePricingStatus::from_db(&pricing_status).ok_or_else(|| {
            StoreError::Serialization(format!("unknown usage pricing status `{pricing_status}`"))
        })?,
        unpriced_reason: row.try_get(15).map_err(to_query_error)?,
        pricing_row_id: pricing_row_id.as_deref().map(parse_uuid).transpose()?,
        pricing_provider_id: row.try_get(17).map_err(to_query_error)?,
        pricing_model_id: row.try_get(18).map_err(to_query_error)?,
        pricing_source: row.try_get(19).map_err(to_query_error)?,
        pricing_source_etag: row.try_get(20).map_err(to_query_error)?,
        pricing_source_fetched_at: pricing_source_fetched_at
            .map(unix_to_datetime)
            .transpose()?,
        pricing_last_updated: row.try_get(22).map_err(to_query_error)?,
        input_cost_per_million_tokens: input_cost_per_million_tokens_10000.map(Money4::from_scaled),
        output_cost_per_million_tokens: output_cost_per_million_tokens_10000
            .map(Money4::from_scaled),
        computed_cost_usd: Money4::from_scaled(computed_cost_10000),
        occurred_at: unix_to_datetime(occurred_at)?,
    })
}

pub(super) fn decode_model_pricing_record(row: &PgRow) -> Result<ModelPricingRecord, StoreError> {
    let model_pricing_id = row.try_get::<String, _>(0).map_err(to_query_error)?;
    let input_cost_per_million_tokens_10000: Option<i64> =
        row.try_get(4).map_err(to_query_error)?;
    let output_cost_per_million_tokens_10000: Option<i64> =
        row.try_get(5).map_err(to_query_error)?;
    let cache_read_cost_per_million_tokens_10000: Option<i64> =
        row.try_get(6).map_err(to_query_error)?;
    let cache_write_cost_per_million_tokens_10000: Option<i64> =
        row.try_get(7).map_err(to_query_error)?;
    let input_audio_cost_per_million_tokens_10000: Option<i64> =
        row.try_get(8).map_err(to_query_error)?;
    let output_audio_cost_per_million_tokens_10000: Option<i64> =
        row.try_get(9).map_err(to_query_error)?;
    let effective_start_at: i64 = row.try_get(12).map_err(to_query_error)?;
    let effective_end_at: Option<i64> = row.try_get(13).map_err(to_query_error)?;
    let limits_json: String = row.try_get(14).map_err(to_query_error)?;
    let modalities_json: String = row.try_get(15).map_err(to_query_error)?;
    let provenance_fetched_at: i64 = row.try_get(18).map_err(to_query_error)?;
    let created_at: i64 = row.try_get(19).map_err(to_query_error)?;
    let updated_at: i64 = row.try_get(20).map_err(to_query_error)?;

    Ok(ModelPricingRecord {
        model_pricing_id: parse_uuid(&model_pricing_id)?,
        pricing_provider_id: row.try_get(1).map_err(to_query_error)?,
        pricing_model_id: row.try_get(2).map_err(to_query_error)?,
        display_name: row.try_get(3).map_err(to_query_error)?,
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
        release_date: row.try_get(10).map_err(to_query_error)?,
        last_updated: row.try_get(11).map_err(to_query_error)?,
        effective_start_at: unix_to_datetime(effective_start_at)?,
        effective_end_at: effective_end_at.map(unix_to_datetime).transpose()?,
        limits: serde_json::from_str::<PricingLimits>(&limits_json)
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
        modalities: serde_json::from_str::<PricingModalities>(&modalities_json)
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
        provenance: PricingProvenance {
            source: row.try_get(16).map_err(to_query_error)?,
            etag: row.try_get(17).map_err(to_query_error)?,
            fetched_at: unix_to_datetime(provenance_fetched_at)?,
        },
        created_at: unix_to_datetime(created_at)?,
        updated_at: unix_to_datetime(updated_at)?,
    })
}

pub(super) fn to_query_error(error: sqlx::Error) -> StoreError {
    let message = error.to_string();
    if let sqlx::Error::Database(db) = &error
        && matches!(db.code().as_deref(), Some("23505" | "23503" | "23514"))
    {
        return StoreError::Conflict(message);
    }

    StoreError::Query(message)
}

pub(super) fn to_write_error(error: sqlx::Error) -> StoreError {
    to_query_error(error)
}
