mod budgets;
use budgets::budget_settings;
pub(crate) use budgets::{
    apply_human_budget_defaults_for_user, reconcile_human_budget_defaults,
    upsert_unless_manually_overridden,
};
use std::collections::BTreeMap;

use gateway_core::{
    ApiKeyRecord, ApiKeySecretStorageKind, AuthMode, BatchEndpoint, BatchJobRecord,
    BatchPollUpdate, BatchPricingStatus, BatchStatus, BudgetScope, BudgetSource, GatewayModel,
    GlobalRole, IdentityUserRecord, MembershipRole, ModelRoute, Money4, NewBatchItem, NewBatchJob,
    OauthProviderRecord, OidcProviderRecord, ProviderBatchResult, ProviderBatchState,
    ProviderRequestContext, SeedApiKeySecretMaterial, SeedManagedServiceAccountApiKey,
    SeedServiceAccount, SeedTeam, SeedUser, StoreError, TeamRecord, UserStatus,
    encrypt_gateway_api_key_secret, generate_gateway_api_key_value, hash_gateway_key_secret,
    parse_gateway_api_key,
};
use serde_json::json;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::GatewayStore;

/// Batch-referenced route rows of one model that survived the reseed delete, by id and by
/// `(provider, upstream, priority)` key.
#[derive(Default)]
pub(crate) struct RouteSurvivors {
    by_key: std::collections::HashMap<(String, String, i64), Uuid>,
    by_id: std::collections::HashMap<Uuid, (String, String, i64)>,
    /// Every survivor with its original key, kept after claims for [`Self::parking_plan`].
    all: Vec<(Uuid, (String, String, i64))>,
}

impl RouteSurvivors {
    pub(crate) fn insert(
        &mut self,
        id: Uuid,
        provider_key: String,
        upstream_model: String,
        priority: i64,
    ) {
        let key = (provider_key, upstream_model, priority);
        self.by_key.insert(key.clone(), id);
        self.by_id.insert(id, key.clone());
        self.all.push((id, key));
    }

    /// Survivor rows to move to an unused priority before upserting, given each configured
    /// route's claimed row and target `(provider, upstream, priority)`. Upserts run one at a
    /// time against the `(model, provider, upstream, priority)` unique key, so a claimed row
    /// whose priority changes, or an unclaimed row sitting on a configured route's target, could
    /// block another route; two routes swapping priorities would block each other. Parked rows
    /// take priorities below every existing and configured priority of their provider and
    /// upstream group, or above it, or in an unused gap when the group spans the whole i32
    /// range, so they cannot collide. Claimed rows then get their real priority from
    /// the upsert; unclaimed ones stay parked as disabled history.
    pub(crate) fn parking_plan(
        &self,
        claimed: &[(Uuid, (String, String, i64))],
    ) -> Vec<(Uuid, i64)> {
        let target_of = claimed
            .iter()
            .map(|(id, key)| (*id, key))
            .collect::<std::collections::HashMap<_, _>>();
        let targets = claimed
            .iter()
            .map(|(_, key)| key)
            .collect::<std::collections::HashSet<_>>();
        // Priorities in use per provider and upstream group, across survivors and configured
        // targets. Parked priorities are added as they are assigned.
        let mut used =
            std::collections::HashMap::<(&str, &str), std::collections::BTreeSet<i64>>::new();
        for (provider, upstream, priority) in self
            .all
            .iter()
            .map(|(_, key)| key)
            .chain(claimed.iter().map(|(_, key)| key))
        {
            used.entry((provider.as_str(), upstream.as_str()))
                .or_default()
                .insert(*priority);
        }

        let mut blocking = self
            .all
            .iter()
            .filter(|(id, key)| match target_of.get(id) {
                Some(target) => *target != key,
                None => targets.contains(key),
            })
            .collect::<Vec<_>>();
        blocking.sort_by_key(|(id, _)| *id);
        blocking
            .into_iter()
            .map(|(id, (provider, upstream, _))| {
                let used = used
                    .get_mut(&(provider.as_str(), upstream.as_str()))
                    .expect("every survivor has a priority group");
                let parked = free_priority(used);
                used.insert(parked);
                (*id, parked)
            })
            .collect()
    }

    /// The row each configured route should upsert, given each route's own deterministic id and
    /// target `(provider, upstream, priority)`, in config order. Claims run in two passes so the
    /// result does not depend on route order: first every route whose own id survived keeps it,
    /// even when its priority changed; then each remaining route may reuse a survivor with the
    /// same `(provider, upstream, priority)`, which covers a renamed route. A route whose own id
    /// already had a row before this seed (`established`) was not renamed, so it keeps its own id
    /// rather than taking another row. Each survivor is claimed once, so two configured routes
    /// never share a row.
    pub(crate) fn claim_all(
        &mut self,
        routes: &[(Uuid, (String, String, i64))],
        established: &std::collections::HashSet<Uuid>,
    ) -> Vec<Uuid> {
        let mut claimed = routes
            .iter()
            .map(|(own_id, _)| {
                self.by_id.remove(own_id).map(|key| {
                    self.by_key.remove(&key);
                    *own_id
                })
            })
            .collect::<Vec<_>>();
        for ((own_id, key), slot) in routes.iter().zip(&mut claimed) {
            if slot.is_none() {
                let reused = if established.contains(own_id) {
                    None
                } else {
                    self.by_key.remove(key)
                };
                let id = match reused {
                    Some(id) => {
                        self.by_id.remove(&id);
                        id
                    }
                    None => *own_id,
                };
                *slot = Some(id);
            }
        }
        claimed.into_iter().flatten().collect()
    }
}

/// An unused priority for a parked row: below the group, else above it, else the first gap
/// inside the i32 range. The group is never the whole range, because it holds one priority per
/// configured route and survivor.
fn free_priority(used: &std::collections::BTreeSet<i64>) -> i64 {
    let (min, max) = (i64::from(i32::MIN), i64::from(i32::MAX));
    let low = used.first().copied().unwrap_or(0);
    let high = used.last().copied().unwrap_or(0);
    if low > min {
        return low - 1;
    }
    if high < max {
        return high + 1;
    }
    (min..=max)
        .find(|priority| !used.contains(priority))
        .expect("a priority group cannot fill the i32 range")
}

pub(crate) fn model_uuid(model_key: &str) -> Uuid {
    Uuid::new_v5(
        &Uuid::NAMESPACE_OID,
        format!("model:{model_key}").as_bytes(),
    )
}

/// The id a configured route upserts under. Before route ids defaulted to the provider key, a
/// route without an `id` took the positional legacy id, so after upgrading it carries a key equal
/// to its provider. Its legacy id stays while a row under it already exists, because responses
/// and request logs refer to route ids. The route moves to the keyed id once the legacy id would
/// have changed anyway, when its priority or position in the model changes. An existing keyed row
/// wins over a legacy one, so reverting that change, while batch history keeps the legacy row,
/// does not move the route back.
pub(crate) fn configured_route_uuid(
    model_key: &str,
    route: &gateway_core::SeedModelRoute,
    route_index: usize,
    existing_ids: &std::collections::HashSet<Uuid>,
) -> Uuid {
    let (keyed, legacy) = route_id_candidates(model_key, route, route_index);
    match legacy {
        Some(legacy) if !existing_ids.contains(&keyed) && existing_ids.contains(&legacy) => legacy,
        _ => keyed,
    }
}

/// A route's keyed id, and its legacy id when its key equals its provider key.
fn route_id_candidates(
    model_key: &str,
    route: &gateway_core::SeedModelRoute,
    route_index: usize,
) -> (Uuid, Option<Uuid>) {
    let keyed = route_uuid(
        model_key,
        route.route_key.as_deref(),
        &route.provider_key,
        &route.upstream_model,
        route.priority,
        route_index,
    );
    let legacy = (route.route_key.as_deref() == Some(route.provider_key.as_str())).then(|| {
        route_uuid(
            model_key,
            None,
            &route.provider_key,
            &route.upstream_model,
            route.priority,
            route_index,
        )
    });
    (keyed, legacy)
}

/// Every id [`configured_route_uuid`] may pick for these models. Seeding checks which of them
/// already exist, so it never reads route history beyond the current configuration.
pub(crate) fn configured_route_candidate_ids(models: &[gateway_core::SeedModel]) -> Vec<Uuid> {
    models
        .iter()
        .flat_map(|model| {
            model
                .routes
                .iter()
                .enumerate()
                .flat_map(|(route_index, route)| {
                    let (keyed, legacy) = route_id_candidates(&model.model_key, route, route_index);
                    std::iter::once(keyed).chain(legacy)
                })
        })
        .collect()
}

pub(crate) fn route_uuid(
    model_key: &str,
    route_key: Option<&str>,
    provider_key: &str,
    upstream_model: &str,
    priority: i32,
    route_index: usize,
) -> Uuid {
    let key = match route_key {
        Some(route_key) => format!(
            "route-key:v1:{}:{model_key}:{}:{route_key}:{}:{provider_key}:{}:{upstream_model}",
            model_key.len(),
            route_key.len(),
            provider_key.len(),
            upstream_model.len()
        ),
        None => {
            format!("route:{model_key}:{provider_key}:{upstream_model}:{priority}:{route_index}")
        }
    };
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
            owner_user_id: None,
            expected_provider_credential_id: None,
            chat_protocol: Default::default(),
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
    default_user_budget: Option<&gateway_core::SeedBudget>,
    now: OffsetDateTime,
) -> Result<(), StoreError>
where
    S: GatewayStore + ?Sized,
{
    for user in users {
        reconcile_seed_user(store, teams_by_key, user, default_user_budget, now).await?;
    }

    Ok(())
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
    default_user_budget: Option<&gateway_core::SeedBudget>,
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
        upsert_unless_manually_overridden(store, &scope, &budget_settings(budget), &source, now)
            .await?;
    } else if let Some(default_budget) = default_user_budget {
        // Replace the override in place. A crash before later reconciliation
        // must not leave a previously budgeted user without enforcement.
        store
            .upsert_active_budget_with_source_guard(
                &scope,
                &budget_settings(default_budget),
                &BudgetSource::config_user_default(),
                Some(&source),
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
                .await?;
            Ok(())
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

#[cfg(test)]
mod route_survivor_tests {
    use uuid::Uuid;

    use super::RouteSurvivors;

    fn key(priority: i64) -> (String, String, i64) {
        ("openai".to_string(), "gpt".to_string(), priority)
    }

    #[test]
    fn parks_above_the_group_when_below_would_leave_the_i32_range() {
        let route = Uuid::from_u128(1);
        let mut survivors = RouteSurvivors::default();
        survivors.insert(route, "openai".to_string(), "gpt".to_string(), 10);

        // The route moves to i32::MIN, so it must be parked before its upsert, but nothing fits
        // below the group.
        let targets = vec![(route, key(i64::from(i32::MIN)))];
        let claimed = survivors
            .claim_all(&targets, &std::collections::HashSet::new())
            .into_iter()
            .zip(targets.into_iter().map(|(_, key)| key))
            .collect::<Vec<_>>();
        let plan = survivors.parking_plan(&claimed);

        assert_eq!(plan, vec![(route, 11)]);
        assert!(i32::try_from(plan[0].1).is_ok());
    }
}

#[cfg(test)]
mod free_priority_tests {
    use std::collections::BTreeSet;

    use super::free_priority;

    #[test]
    fn finds_an_in_range_gap_when_both_edges_are_taken() {
        let used = BTreeSet::from([i64::from(i32::MIN), i64::from(i32::MAX)]);
        let parked = free_priority(&used);
        assert!(i32::try_from(parked).is_ok());
        assert!(!used.contains(&parked));
        assert_eq!(free_priority(&BTreeSet::from([10, 20])), 9);
        assert_eq!(
            free_priority(&BTreeSet::from([i64::from(i32::MIN), 5])),
            6,
            "parks above when the group reaches i32::MIN"
        );
    }
}
