use std::collections::{BTreeMap, BTreeSet};

use gateway_core::{
    ApiKeyRecord, ApiKeySecretStorageKind, AuthMode, BatchEndpoint, BatchJobRecord,
    BatchPollUpdate, BatchPricingStatus, BatchStatus, BudgetModelSelector, BudgetScope,
    BudgetSettings, BudgetSource, BudgetSourceKind, GatewayModel, GlobalRole, IdentityUserRecord,
    MembershipRole, ModelRoute, Money4, NewBatchItem, NewBatchJob, OauthProviderRecord,
    OidcProviderRecord, ProviderBatchResult, ProviderBatchState, ProviderRequestContext,
    SYSTEM_BOOTSTRAP_ADMIN_USER_ID, SeedApiKeySecretMaterial, SeedHumanBudgetDefaults,
    SeedManagedServiceAccountApiKey, SeedServiceAccount, SeedTeam, SeedUser,
    SeedUserModelBudgetDefault, StoreError, TeamRecord, UserStatus, encrypt_gateway_api_key_secret,
    generate_gateway_api_key_value, hash_gateway_key_secret, parse_gateway_api_key,
};
use serde_json::json;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::GatewayStore;

pub(crate) fn model_uuid(model_key: &str) -> Uuid {
    Uuid::new_v5(
        &Uuid::NAMESPACE_OID,
        format!("model:{model_key}").as_bytes(),
    )
}

pub(crate) fn route_uuid(
    model_key: &str,
    provider_key: &str,
    upstream_model: &str,
    priority: i32,
    route_index: usize,
) -> Uuid {
    let key = format!("route:{model_key}:{provider_key}:{upstream_model}:{priority}:{route_index}");
    Uuid::new_v5(&Uuid::NAMESPACE_OID, key.as_bytes())
}

pub(crate) fn api_key_uuid(public_id: &str) -> Uuid {
    Uuid::new_v5(
        &Uuid::NAMESPACE_OID,
        format!("api_key:{public_id}").as_bytes(),
    )
}

pub(crate) fn managed_api_key_uuid(service_account_key: &str, config_key: &str) -> Uuid {
    Uuid::new_v5(
        &Uuid::NAMESPACE_OID,
        format!("managed_api_key:{service_account_key}:{config_key}").as_bytes(),
    )
}

pub(crate) fn service_account_uuid(service_account_key: &str) -> Uuid {
    Uuid::new_v5(
        &Uuid::NAMESPACE_OID,
        format!("service_account:{service_account_key}").as_bytes(),
    )
}

/// Add stable batch examples to the local demo data set.
///
/// The completed batch makes response inspection useful without an upstream
/// provider. The queued batch stays unclaimed so operators can exercise the
/// cancellation flow from the admin UI.
pub async fn seed_local_demo_batches<S>(
    store: &S,
    user_api_key: &ApiKeyRecord,
    service_account_api_key: &ApiKeyRecord,
    requested_model_key: &str,
    execution_model: &GatewayModel,
    route: &ModelRoute,
    now: OffsetDateTime,
) -> Result<(), StoreError>
where
    S: GatewayStore + ?Sized,
{
    seed_completed_demo_batch(
        store,
        user_api_key,
        requested_model_key,
        execution_model,
        route,
        now,
    )
    .await?;
    seed_queued_demo_batch(
        store,
        service_account_api_key,
        requested_model_key,
        execution_model,
        route,
        now,
    )
    .await
}

async fn seed_completed_demo_batch<S>(
    store: &S,
    api_key: &ApiKeyRecord,
    requested_model_key: &str,
    execution_model: &GatewayModel,
    route: &ModelRoute,
    now: OffsetDateTime,
) -> Result<(), StoreError>
where
    S: GatewayStore + ?Sized,
{
    const IDEMPOTENCY_KEY: &str = "local-demo-completed-analysis";
    if store
        .get_batch_by_idempotency_key(api_key.id, IDEMPOTENCY_KEY)
        .await?
        .is_some()
    {
        return Ok(());
    }

    let worker_id = "local-demo-seed";
    let created_at = now - time::Duration::hours(2);
    let completed_at = created_at + time::Duration::minutes(18);
    let batch_id = local_demo_batch_uuid(IDEMPOTENCY_KEY);
    let items = vec![
        NewBatchItem {
            batch_item_id: local_demo_batch_item_uuid(IDEMPOTENCY_KEY, "retention-summary"),
            custom_id: "retention-summary".to_string(),
            request_body: json!({
                "input": "Summarize the strongest retention signals in the Q2 cohort analysis.",
                "reasoning": {"effort": "medium"}
            }),
        },
        NewBatchItem {
            batch_item_id: local_demo_batch_item_uuid(IDEMPOTENCY_KEY, "risk-review"),
            custom_id: "risk-review".to_string(),
            request_body: json!({
                "input": "List the main evidence gaps and the next checks for the Q2 cohort analysis.",
                "reasoning": {"effort": "medium"}
            }),
        },
    ];
    let job = demo_batch_job(
        batch_id,
        IDEMPOTENCY_KEY,
        api_key,
        requested_model_key,
        execution_model,
        route,
        BatchEndpoint::Responses,
        BatchStatus::InProgress,
        i64::try_from(items.len()).unwrap_or(i64::MAX),
        created_at,
        Some(worker_id.to_string()),
        Some(now + time::Duration::minutes(5)),
        Some("batch_local_demo_completed".to_string()),
        Some(created_at + time::Duration::minutes(1)),
        Some(now),
    );
    store.insert_batch(&NewBatchJob { job, items }).await?;
    store
        .apply_batch_poll_update(
            batch_id,
            worker_id,
            &BatchPollUpdate {
                state: ProviderBatchState {
                    provider_batch_id: "batch_local_demo_completed".to_string(),
                    status: BatchStatus::Completed,
                    request_count: 2,
                    completed_count: 2,
                    failed_count: 0,
                    provider_usage: Some(json!({
                        "input_tokens": 1860,
                        "output_tokens": 724,
                        "total_tokens": 2584
                    })),
                    provider_cost_usd: Some(Money4::from_scaled(184)),
                    error: None,
                    submitted_at: Some(created_at + time::Duration::minutes(1)),
                    completed_at: Some(completed_at),
                },
                results: vec![
                    ProviderBatchResult {
                        custom_id: "retention-summary".to_string(),
                        response_body: Some(json!({
                            "id": "resp_local_demo_retention",
                            "status": "completed",
                            "output_text": "Activation in the first session and repeated use in week one are the strongest retention signals. The result is directional because the cohort has not completed a full quarter."
                        })),
                        error: None,
                        provider_request_id: Some("req_local_demo_retention".to_string()),
                        provider_usage: Some(json!({"input_tokens": 920, "output_tokens": 338})),
                        completed_at: Some(completed_at - time::Duration::minutes(2)),
                        cost_usd: Some(Money4::from_scaled(89)),
                    },
                    ProviderBatchResult {
                        custom_id: "risk-review".to_string(),
                        response_body: Some(json!({
                            "id": "resp_local_demo_risks",
                            "status": "completed",
                            "output_text": "The main gaps are short follow-up time, channel mix changes, and missing acquisition-cost data. Re-run after the quarter closes and stratify by channel and plan."
                        })),
                        error: None,
                        provider_request_id: Some("req_local_demo_risks".to_string()),
                        provider_usage: Some(json!({"input_tokens": 940, "output_tokens": 386})),
                        completed_at: Some(completed_at),
                        cost_usd: Some(Money4::from_scaled(95)),
                    },
                ],
                next_poll_at: None,
                pricing_status: Some(BatchPricingStatus::ProviderReported),
            },
        )
        .await
}

async fn seed_queued_demo_batch<S>(
    store: &S,
    api_key: &ApiKeyRecord,
    requested_model_key: &str,
    execution_model: &GatewayModel,
    route: &ModelRoute,
    now: OffsetDateTime,
) -> Result<(), StoreError>
where
    S: GatewayStore + ?Sized,
{
    const IDEMPOTENCY_KEY: &str = "local-demo-queued-evaluation";
    if store
        .get_batch_by_idempotency_key(api_key.id, IDEMPOTENCY_KEY)
        .await?
        .is_some()
    {
        return Ok(());
    }

    let created_at = now - time::Duration::minutes(12);
    let batch_id = local_demo_batch_uuid(IDEMPOTENCY_KEY);
    let items = [
        (
            "eval-grounding",
            "Evaluate grounding against the supplied reference set.",
        ),
        (
            "eval-refusal",
            "Evaluate refusal quality for unsafe requests.",
        ),
        ("eval-format", "Evaluate JSON schema compliance."),
    ]
    .into_iter()
    .map(|(custom_id, input)| NewBatchItem {
        batch_item_id: local_demo_batch_item_uuid(IDEMPOTENCY_KEY, custom_id),
        custom_id: custom_id.to_string(),
        request_body: json!({"input": input, "reasoning": {"effort": "low"}}),
    })
    .collect::<Vec<_>>();
    let job = demo_batch_job(
        batch_id,
        IDEMPOTENCY_KEY,
        api_key,
        requested_model_key,
        execution_model,
        route,
        BatchEndpoint::Responses,
        BatchStatus::Queued,
        i64::try_from(items.len()).unwrap_or(i64::MAX),
        created_at,
        None,
        None,
        None,
        None,
        Some(now + time::Duration::days(365)),
    );
    store.insert_batch(&NewBatchJob { job, items }).await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn demo_batch_job(
    batch_id: Uuid,
    idempotency_key: &str,
    api_key: &ApiKeyRecord,
    requested_model_key: &str,
    execution_model: &GatewayModel,
    route: &ModelRoute,
    endpoint: BatchEndpoint,
    status: BatchStatus,
    request_count: i64,
    created_at: OffsetDateTime,
    lease_owner: Option<String>,
    lease_expires_at: Option<OffsetDateTime>,
    provider_batch_id: Option<String>,
    submitted_at: Option<OffsetDateTime>,
    next_poll_at: Option<OffsetDateTime>,
) -> BatchJobRecord {
    BatchJobRecord {
        batch_id,
        idempotency_key: idempotency_key.to_string(),
        request_hash: format!("local-demo:{idempotency_key}"),
        api_key_id: api_key.id,
        user_id: api_key.owner_user_id,
        team_id: api_key.owner_team_id,
        service_account_id: api_key.owner_service_account_id,
        model_id: execution_model.id,
        model_key: requested_model_key.to_string(),
        resolved_model_key: execution_model.model_key.clone(),
        route_id: route.id,
        provider_key: route.provider_key.clone(),
        upstream_model: route.upstream_model.clone(),
        endpoint,
        status,
        provider_batch_id,
        request_count,
        completed_count: 0,
        failed_count: 0,
        cost_usd: None,
        pricing_status: BatchPricingStatus::Pending,
        provider_usage: None,
        error: None,
        created_at,
        submitted_at,
        completed_at: None,
        updated_at: created_at,
        next_poll_at,
        lease_owner,
        lease_expires_at,
        provider_context: ProviderRequestContext {
            request_id: format!("local-demo-{batch_id}"),
            model_key: requested_model_key.to_string(),
            provider_key: route.provider_key.clone(),
            upstream_model: route.upstream_model.clone(),
            extra_headers: route.extra_headers.clone(),
            extra_body: route.extra_body.clone(),
            request_headers: BTreeMap::new(),
            compatibility: route.compatibility.clone(),
        },
        pricing_snapshot: None,
    }
}

fn local_demo_batch_uuid(key: &str) -> Uuid {
    Uuid::new_v5(
        &Uuid::NAMESPACE_OID,
        format!("local_demo:batch:{key}").as_bytes(),
    )
}

fn local_demo_batch_item_uuid(batch_key: &str, custom_id: &str) -> Uuid {
    Uuid::new_v5(
        &Uuid::NAMESPACE_OID,
        format!("local_demo:batch_item:{batch_key}:{custom_id}").as_bytes(),
    )
}

pub(crate) fn generate_seed_api_key_material()
-> Result<(String, String, SeedApiKeySecretMaterial), StoreError> {
    let raw_key = generate_gateway_api_key_value();
    seed_api_key_material_from_raw(&raw_key)
}

pub(crate) fn seed_api_key_material_from_raw(
    raw_key: &str,
) -> Result<(String, String, SeedApiKeySecretMaterial), StoreError> {
    let parsed = parse_gateway_api_key(raw_key).map_err(|error| {
        StoreError::Conflict(format!("generated gateway key is invalid: {error}"))
    })?;
    let secret_hash = hash_gateway_key_secret(&parsed.secret)
        .map_err(|error| StoreError::Unexpected(error.to_string()))?;
    let encrypted = encrypt_gateway_api_key_secret(raw_key)
        .map_err(|error| StoreError::Unexpected(error.to_string()))?;

    Ok((
        parsed.public_id,
        secret_hash,
        SeedApiKeySecretMaterial {
            storage_kind: ApiKeySecretStorageKind::EncryptedBlob,
            secret_ciphertext: encrypted.ciphertext,
            secret_nonce: encrypted.nonce,
            secret_key_id: encrypted.key_id.to_string(),
        },
    ))
}

pub(crate) fn provided_seed_api_key_material(
    managed_key: &SeedManagedServiceAccountApiKey,
) -> Result<Option<(String, String, SeedApiKeySecretMaterial)>, StoreError> {
    match (
        managed_key.public_id.as_ref(),
        managed_key.secret_hash.as_ref(),
        managed_key.secret_material.as_ref(),
    ) {
        (Some(public_id), Some(secret_hash), Some(secret_material)) => Ok(Some((
            public_id.clone(),
            secret_hash.clone(),
            secret_material.clone(),
        ))),
        (None, None, None) => Ok(None),
        _ => Err(StoreError::Conflict(format!(
            "managed api key `{}` has incomplete secret material",
            managed_key.config_key
        ))),
    }
}

pub(crate) fn oidc_provider_uuid(provider_key: &str) -> String {
    Uuid::new_v5(
        &Uuid::NAMESPACE_OID,
        format!("oidc_provider:{provider_key}").as_bytes(),
    )
    .to_string()
}

pub(crate) fn oauth_provider_uuid(provider_key: &str) -> String {
    Uuid::new_v5(
        &Uuid::NAMESPACE_OID,
        format!("oauth_provider:{provider_key}").as_bytes(),
    )
    .to_string()
}

pub(crate) async fn reconcile_seed_teams<S>(
    store: &S,
    teams: &[SeedTeam],
    now: OffsetDateTime,
) -> Result<BTreeMap<String, TeamRecord>, StoreError>
where
    S: GatewayStore + ?Sized,
{
    let mut records = BTreeMap::new();

    for team in teams {
        let mut record = match store.get_team_by_key(&team.team_key).await? {
            Some(existing) => existing,
            None => store.create_team(&team.team_key, &team.team_name).await?,
        };

        if record.team_name != team.team_name {
            store
                .update_team_name(record.team_id, &team.team_name, now)
                .await?;
            record.team_name = team.team_name.clone();
            record.updated_at = now;
        }
        if let Some(tags) = team.tags.as_ref()
            && record.tags != *tags
        {
            store.update_team_tags(record.team_id, tags, now).await?;
            record.tags = tags.clone();
            record.updated_at = now;
        }

        records.insert(team.team_key.clone(), record);
    }

    Ok(records)
}

pub(crate) fn validate_seed_service_account_team_references(
    teams: &[SeedTeam],
    service_accounts: &[SeedServiceAccount],
) -> Result<(), StoreError> {
    for service_account in service_accounts {
        if !teams
            .iter()
            .any(|team| team.team_key == service_account.team_key)
        {
            return Err(StoreError::Conflict(format!(
                "seed service account '{}' references unknown team '{}'",
                service_account.service_account_key, service_account.team_key
            )));
        }
    }
    Ok(())
}

pub(crate) async fn prevalidate_seed_users<S>(
    store: &S,
    users: &[SeedUser],
) -> Result<(), StoreError>
where
    S: GatewayStore + ?Sized,
{
    let identity_users = store.list_identity_users().await?;

    for user in users {
        prevalidate_seed_user(store, &identity_users, user).await?;
    }

    Ok(())
}

pub(crate) async fn reconcile_seed_users<S>(
    store: &S,
    teams_by_key: &BTreeMap<String, TeamRecord>,
    users: &[SeedUser],
    now: OffsetDateTime,
) -> Result<(), StoreError>
where
    S: GatewayStore + ?Sized,
{
    for user in users {
        reconcile_seed_user(store, teams_by_key, user, now).await?;
    }

    Ok(())
}

pub(crate) async fn reconcile_human_budget_defaults<S>(
    store: &S,
    defaults: &SeedHumanBudgetDefaults,
    now: OffsetDateTime,
) -> Result<(), StoreError>
where
    S: GatewayStore + ?Sized,
{
    let mut user_ids = BTreeSet::new();
    for identity_user in store.list_identity_users().await? {
        user_ids.insert(identity_user.user.user_id);
    }

    let bootstrap_admin_user_id = Uuid::parse_str(SYSTEM_BOOTSTRAP_ADMIN_USER_ID)
        .map_err(|error| StoreError::Unexpected(error.to_string()))?;
    if store
        .get_user_by_id(bootstrap_admin_user_id)
        .await?
        .is_some()
    {
        user_ids.insert(bootstrap_admin_user_id);
    }

    for user_id in user_ids {
        apply_human_budget_defaults_for_user(store, defaults, user_id, now).await?;
    }

    deactivate_stale_config_default_budgets(store, defaults, now).await
}

pub(crate) async fn apply_human_budget_defaults_for_user<S>(
    store: &S,
    defaults: &SeedHumanBudgetDefaults,
    user_id: Uuid,
    now: OffsetDateTime,
) -> Result<(), StoreError>
where
    S: GatewayStore + ?Sized,
{
    if let Some(default_budget) = defaults.default_user_budget.as_ref() {
        let scope = BudgetScope::User { user_id };
        upsert_if_config_default_allowed(
            store,
            &scope,
            &budget_settings(default_budget),
            &BudgetSource::config_user_default(),
            now,
        )
        .await?;
    }

    for model_default in &defaults.model_defaults {
        apply_user_model_default(store, model_default, user_id, now).await?;
    }

    Ok(())
}

async fn apply_user_model_default<S>(
    store: &S,
    model_default: &SeedUserModelBudgetDefault,
    user_id: Uuid,
    now: OffsetDateTime,
) -> Result<(), StoreError>
where
    S: GatewayStore + ?Sized,
{
    let scope = BudgetScope::UserModel {
        user_id,
        selector: BudgetModelSelector::Model {
            model_id: model_default.model_id,
        },
    };
    upsert_if_config_default_allowed(
        store,
        &scope,
        &budget_settings(&model_default.budget),
        &BudgetSource::config_user_model_default(&model_default.model_key),
        now,
    )
    .await
}

async fn upsert_if_config_default_allowed<S>(
    store: &S,
    scope: &BudgetScope,
    settings: &BudgetSettings,
    source: &BudgetSource,
    now: OffsetDateTime,
) -> Result<(), StoreError>
where
    S: GatewayStore + ?Sized,
{
    let existing = store.get_active_budget_by_scope(scope).await?;
    if existing
        .as_ref()
        .is_some_and(|budget| !budget.source.matches(source))
    {
        return Ok(());
    }

    if let Some(latest) = store.get_latest_budget_by_scope(scope).await?
        && !latest.is_active
        && latest.source.is_manual_deactivation()
    {
        return Ok(());
    }

    let expected_current_source = existing.as_ref().map(|budget| &budget.source);
    store
        .upsert_active_budget_with_source_guard(
            scope,
            settings,
            source,
            expected_current_source,
            now,
        )
        .await?;
    Ok(())
}

async fn deactivate_stale_config_default_budgets<S>(
    store: &S,
    defaults: &SeedHumanBudgetDefaults,
    now: OffsetDateTime,
) -> Result<(), StoreError>
where
    S: GatewayStore + ?Sized,
{
    let active_model_source_keys = defaults
        .model_defaults
        .iter()
        .map(|default| {
            BudgetSource::config_user_model_default(&default.model_key)
                .key
                .expect("model default source key")
        })
        .collect::<std::collections::BTreeSet<_>>();

    for budget in store.list_active_budgets(None).await? {
        let should_deactivate = match budget.source.kind {
            BudgetSourceKind::ConfigUserDefault => defaults.default_user_budget.is_none(),
            BudgetSourceKind::ConfigUserModelDefault => match budget.source.key.as_ref() {
                Some(key) => !active_model_source_keys.contains(key),
                None => true,
            },
            BudgetSourceKind::Manual | BudgetSourceKind::ConfigUserOverride => false,
        };
        if should_deactivate {
            store
                .deactivate_active_budget_by_source(&budget.scope, &budget.source, now)
                .await?;
        }
    }

    Ok(())
}

fn budget_settings(budget: &gateway_core::SeedBudget) -> BudgetSettings {
    BudgetSettings {
        cadence: budget.cadence,
        amount_usd: budget.amount_usd,
        hard_limit: budget.hard_limit,
        timezone: budget.timezone.clone(),
    }
}

async fn prevalidate_seed_user<S>(
    store: &S,
    identity_users: &[IdentityUserRecord],
    seed_user: &SeedUser,
) -> Result<(), StoreError>
where
    S: GatewayStore + ?Sized,
{
    let oidc_provider = resolve_seed_oidc_provider(store, seed_user).await?;
    let oauth_provider = resolve_seed_oauth_provider(store, seed_user).await?;
    let Some(existing_user) = store
        .get_user_by_email_normalized(&seed_user.email_normalized)
        .await?
    else {
        return Ok(());
    };

    let identity_user = load_identity_user(store, existing_user.user_id).await?;
    ensure_seed_auth_mutation_allowed(
        &identity_user,
        seed_user.auth_mode,
        oidc_provider.as_ref(),
        oauth_provider.as_ref(),
    )?;
    ensure_seed_role_mutation_allowed(identity_users, &identity_user, seed_user.global_role)?;
    ensure_seed_membership_mutation_allowed(&identity_user)?;
    Ok(())
}

async fn reconcile_seed_user<S>(
    store: &S,
    teams_by_key: &BTreeMap<String, TeamRecord>,
    seed_user: &SeedUser,
    now: OffsetDateTime,
) -> Result<(), StoreError>
where
    S: GatewayStore + ?Sized,
{
    let oidc_provider = resolve_seed_oidc_provider(store, seed_user).await?;
    let oauth_provider = resolve_seed_oauth_provider(store, seed_user).await?;

    let (existing_user, existing_identity_user) = match store
        .get_user_by_email_normalized(&seed_user.email_normalized)
        .await?
    {
        Some(existing) => {
            let user_id = existing.user_id;
            (existing, Some(load_identity_user(store, user_id).await?))
        }
        None => (
            store
                .create_identity_user(
                    &seed_user.name,
                    &seed_user.email,
                    &seed_user.email_normalized,
                    seed_user.global_role,
                    seed_user.auth_mode,
                    UserStatus::Invited,
                )
                .await?,
            None,
        ),
    };

    if let Some(identity_user) = existing_identity_user.as_ref() {
        ensure_seed_auth_mutation_allowed(
            identity_user,
            seed_user.auth_mode,
            oidc_provider.as_ref(),
            oauth_provider.as_ref(),
        )?;
        let identity_users = store.list_identity_users().await?;
        ensure_seed_role_mutation_allowed(&identity_users, identity_user, seed_user.global_role)?;
        ensure_seed_membership_mutation_allowed(identity_user)?;
    }

    if existing_user.global_role != seed_user.global_role
        || existing_user.auth_mode != seed_user.auth_mode
    {
        store
            .update_identity_user(
                existing_user.user_id,
                seed_user.global_role,
                seed_user.auth_mode,
                now,
            )
            .await?;
    }

    store
        .seed_update_identity_user_profile(
            existing_user.user_id,
            &seed_user.name,
            &seed_user.email,
            &seed_user.email_normalized,
            seed_user.request_logging_enabled,
            now,
        )
        .await?;
    if let Some(tags) = seed_user.tags.as_ref()
        && existing_user.tags != *tags
    {
        store
            .update_user_tags(existing_user.user_id, tags, now)
            .await?;
    }

    let mut identity_user = load_identity_user(store, existing_user.user_id).await?;
    sync_seed_user_auth_mode(
        store,
        &identity_user,
        seed_user.auth_mode,
        oidc_provider.as_ref(),
        oauth_provider.as_ref(),
        now,
    )
    .await?;

    identity_user = load_identity_user(store, existing_user.user_id).await?;
    sync_seed_user_membership(store, &identity_user, teams_by_key, seed_user, now).await?;

    let scope = BudgetScope::User {
        user_id: existing_user.user_id,
    };
    let source = BudgetSource::config_user_override(&seed_user.email_normalized);
    if let Some(budget) = &seed_user.budget {
        let existing = store.get_active_budget_by_scope(&scope).await?;
        if existing
            .as_ref()
            .is_some_and(|budget| budget.source.kind == BudgetSourceKind::Manual)
        {
            return Ok(());
        }
        let settings = budget_settings(budget);
        let expected_current_source = existing.as_ref().map(|budget| &budget.source);
        store
            .upsert_active_budget_with_source_guard(
                &scope,
                &settings,
                &source,
                expected_current_source,
                now,
            )
            .await?;
    } else {
        store
            .deactivate_active_budget_by_source(&scope, &source, now)
            .await?;
    }

    Ok(())
}

async fn load_identity_user<S>(store: &S, user_id: Uuid) -> Result<IdentityUserRecord, StoreError>
where
    S: GatewayStore + ?Sized,
{
    store
        .get_identity_user(user_id)
        .await?
        .ok_or_else(|| StoreError::NotFound(format!("identity user `{user_id}` not found")))
}

fn ensure_seed_auth_mutation_allowed(
    user: &IdentityUserRecord,
    next_auth_mode: AuthMode,
    oidc_provider: Option<&OidcProviderRecord>,
    oauth_provider: Option<&OauthProviderRecord>,
) -> Result<(), StoreError> {
    if user.user.auth_mode != next_auth_mode && user.user.status != UserStatus::Invited {
        return Err(StoreError::Conflict(
            "auth mode can only change while the user is invited".to_string(),
        ));
    }

    let current_oidc_provider_id = user.oidc_provider_id.as_deref();
    let next_oidc_provider_id = oidc_provider.map(|provider| provider.oidc_provider_id.as_str());
    if next_auth_mode == AuthMode::Oidc
        && current_oidc_provider_id != next_oidc_provider_id
        && user.user.status != UserStatus::Invited
    {
        return Err(StoreError::Conflict(
            "oidc provider can only change while the user is invited".to_string(),
        ));
    }

    let current_oauth_provider_id = user.oauth_provider_id.as_deref();
    let next_oauth_provider_id = oauth_provider.map(|provider| provider.oauth_provider_id.as_str());
    if next_auth_mode == AuthMode::Oauth
        && current_oauth_provider_id != next_oauth_provider_id
        && user.user.status != UserStatus::Invited
    {
        return Err(StoreError::Conflict(
            "oauth provider can only change while the user is invited".to_string(),
        ));
    }

    Ok(())
}

fn ensure_seed_role_mutation_allowed(
    identity_users: &[IdentityUserRecord],
    user: &IdentityUserRecord,
    next_global_role: GlobalRole,
) -> Result<(), StoreError> {
    if user.user.global_role == GlobalRole::PlatformAdmin
        && user.user.status == UserStatus::Active
        && next_global_role != GlobalRole::PlatformAdmin
    {
        let remaining_active_admins = identity_users
            .iter()
            .filter(|candidate| {
                candidate.user.user_id != user.user.user_id
                    && candidate.user.global_role == GlobalRole::PlatformAdmin
                    && candidate.user.status == UserStatus::Active
            })
            .count();
        if remaining_active_admins == 0 {
            return Err(StoreError::Conflict(
                "the last active platform admin cannot be deactivated or demoted".to_string(),
            ));
        }
    }

    Ok(())
}

fn ensure_seed_membership_mutation_allowed(user: &IdentityUserRecord) -> Result<(), StoreError> {
    if user.membership_role == Some(MembershipRole::Owner) {
        return Err(StoreError::Conflict(
            "owner memberships cannot be created, removed, or transferred in this workflow"
                .to_string(),
        ));
    }

    Ok(())
}

async fn resolve_seed_oidc_provider<S>(
    store: &S,
    seed_user: &SeedUser,
) -> Result<Option<OidcProviderRecord>, StoreError>
where
    S: GatewayStore + ?Sized,
{
    match seed_user.auth_mode {
        AuthMode::Oidc => {
            let provider_key = seed_user.oidc_provider_key.as_deref().ok_or_else(|| {
                StoreError::Conflict("oidc_provider_key is required for oidc users".to_string())
            })?;
            Ok(Some(
                store
                    .get_enabled_oidc_provider_by_key(provider_key)
                    .await?
                    .ok_or_else(|| {
                        StoreError::NotFound(format!(
                            "oidc provider `{provider_key}` is not enabled"
                        ))
                    })?,
            ))
        }
        AuthMode::Password | AuthMode::Oauth => Ok(None),
    }
}

async fn resolve_seed_oauth_provider<S>(
    store: &S,
    seed_user: &SeedUser,
) -> Result<Option<OauthProviderRecord>, StoreError>
where
    S: GatewayStore + ?Sized,
{
    match seed_user.auth_mode {
        AuthMode::Oauth => {
            let provider_key = seed_user.oauth_provider_key.as_deref().ok_or_else(|| {
                StoreError::Conflict("oauth_provider_key is required for oauth users".to_string())
            })?;
            Ok(Some(
                store
                    .get_enabled_oauth_provider_by_key(provider_key)
                    .await?
                    .ok_or_else(|| {
                        StoreError::NotFound(format!(
                            "oauth provider `{provider_key}` is not enabled"
                        ))
                    })?,
            ))
        }
        AuthMode::Password | AuthMode::Oidc => Ok(None),
    }
}

async fn sync_seed_user_auth_mode<S>(
    store: &S,
    user: &IdentityUserRecord,
    next_auth_mode: AuthMode,
    oidc_provider: Option<&OidcProviderRecord>,
    oauth_provider: Option<&OauthProviderRecord>,
    now: OffsetDateTime,
) -> Result<(), StoreError>
where
    S: GatewayStore + ?Sized,
{
    if user.user.auth_mode == AuthMode::Password && next_auth_mode != AuthMode::Password {
        store.delete_user_password_auth(user.user.user_id).await?;
        store
            .revoke_password_invitations_for_user(user.user.user_id, now)
            .await?;
    }

    if let Some(current_provider_id) = user.oidc_provider_id.as_deref() {
        let next_provider_id = oidc_provider.map(|provider| provider.oidc_provider_id.as_str());
        if next_auth_mode != AuthMode::Oidc || next_provider_id != Some(current_provider_id) {
            store
                .delete_user_oidc_auth(user.user.user_id, current_provider_id)
                .await?;
        }
    }

    if let Some(current_provider_id) = user.oauth_provider_id.as_deref() {
        let next_provider_id = oauth_provider.map(|provider| provider.oauth_provider_id.as_str());
        if next_auth_mode != AuthMode::Oauth || next_provider_id != Some(current_provider_id) {
            store
                .delete_user_oauth_auth(user.user.user_id, current_provider_id)
                .await?;
        }
    }

    match next_auth_mode {
        AuthMode::Password => {
            store.clear_user_oidc_link(user.user.user_id).await?;
            store.clear_user_oauth_link(user.user.user_id).await?;
        }
        AuthMode::Oidc => {
            let provider = oidc_provider.ok_or_else(|| {
                StoreError::Conflict("oidc provider configuration is required".to_string())
            })?;
            store.clear_user_oauth_link(user.user.user_id).await?;
            store
                .set_user_oidc_link(user.user.user_id, &provider.oidc_provider_id, now)
                .await?;
        }
        AuthMode::Oauth => {
            let provider = oauth_provider.ok_or_else(|| {
                StoreError::Conflict("oauth provider configuration is required".to_string())
            })?;
            store.clear_user_oidc_link(user.user.user_id).await?;
            store
                .set_user_oauth_link(user.user.user_id, &provider.oauth_provider_id, now)
                .await?;
        }
    }

    Ok(())
}

async fn sync_seed_user_membership<S>(
    store: &S,
    user: &IdentityUserRecord,
    teams_by_key: &BTreeMap<String, TeamRecord>,
    seed_user: &SeedUser,
    now: OffsetDateTime,
) -> Result<(), StoreError>
where
    S: GatewayStore + ?Sized,
{
    let requested_membership = requested_seed_membership(teams_by_key, seed_user)?;
    if current_membership(user) == requested_membership {
        return Ok(());
    }
    if user.membership_role == Some(MembershipRole::Owner) {
        return Err(StoreError::Conflict(
            "owner memberships cannot be created, removed, or transferred in this workflow"
                .to_string(),
        ));
    }

    match (user.team_id, requested_membership) {
        (None, None) => Ok(()),
        (None, Some((team_id, role))) => {
            store
                .assign_team_membership(user.user.user_id, team_id, role)
                .await
        }
        (Some(team_id), None) => {
            store
                .remove_team_membership(team_id, user.user.user_id)
                .await?;
            Ok(())
        }
        (Some(current_team_id), Some((next_team_id, next_role)))
            if current_team_id == next_team_id =>
        {
            if user.membership_role != Some(next_role) {
                store
                    .update_team_membership_role(current_team_id, user.user.user_id, next_role, now)
                    .await?;
            }
            Ok(())
        }
        (Some(current_team_id), Some((next_team_id, next_role))) => {
            store
                .transfer_team_membership(
                    user.user.user_id,
                    current_team_id,
                    next_team_id,
                    next_role,
                    now,
                )
                .await
        }
    }
}

fn requested_seed_membership(
    teams_by_key: &BTreeMap<String, TeamRecord>,
    seed_user: &SeedUser,
) -> Result<Option<(Uuid, MembershipRole)>, StoreError> {
    let Some(membership) = seed_user.membership.as_ref() else {
        return Ok(None);
    };
    if membership.role == MembershipRole::Owner {
        return Err(StoreError::Conflict(
            "owner memberships cannot be created, removed, or transferred in this workflow"
                .to_string(),
        ));
    }
    let team = teams_by_key.get(&membership.team_key).ok_or_else(|| {
        StoreError::NotFound(format!(
            "seed user `{}` references unknown team `{}`",
            seed_user.email, membership.team_key
        ))
    })?;
    Ok(Some((team.team_id, membership.role)))
}

fn current_membership(user: &IdentityUserRecord) -> Option<(Uuid, MembershipRole)> {
    match (user.team_id, user.membership_role) {
        (Some(team_id), Some(role)) => Some((team_id, role)),
        _ => None,
    }
}
