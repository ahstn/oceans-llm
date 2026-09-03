use std::{collections::HashSet, sync::Arc};

use gateway_core::{
    ApiKeyModelGrantMode, ApiKeyOwnerKind, AuthError, AuthenticatedApiKey, GatewayError,
    GatewayModel, IdentityRepository, ModelAccessMode, ModelAllowlistPolicy, ModelRepository,
    RouteError, UserStatus,
};
use itertools::Itertools;

#[derive(Clone)]
pub struct ModelAccess<R> {
    repo: Arc<R>,
}

impl<R> ModelAccess<R>
where
    R: ModelRepository + IdentityRepository,
{
    #[must_use]
    pub fn new(repo: Arc<R>) -> Self {
        Self { repo }
    }

    pub async fn list_models_for_api_key(
        &self,
        api_key: &AuthenticatedApiKey,
    ) -> Result<Vec<GatewayModel>, GatewayError> {
        self.effective_models_for_api_key(api_key).await
    }

    #[tracing::instrument(
        name = "gateway.model.access",
        skip_all,
        fields(gen_ai.request.model = requested_model)
    )]
    pub async fn resolve_requested_model(
        &self,
        api_key: &AuthenticatedApiKey,
        requested_model: &str,
    ) -> Result<GatewayModel, GatewayError> {
        if let Some(tag_expression) = requested_model.strip_prefix("tag:") {
            return self.resolve_tag_expression(api_key, tag_expression).await;
        }

        let model = self
            .repo
            .get_model_by_key(requested_model)
            .await?
            .ok_or_else(|| RouteError::ModelNotFound(requested_model.to_string()))?;

        self.ensure_api_key_can_access_model(api_key, &model)
            .await?;

        Ok(model)
    }

    async fn resolve_tag_expression(
        &self,
        api_key: &AuthenticatedApiKey,
        tag_expression: &str,
    ) -> Result<GatewayModel, GatewayError> {
        let requested_tags = tag_expression
            .split(',')
            .map(str::trim)
            .filter(|tag| !tag.is_empty())
            .map(ToString::to_string)
            .collect_vec();

        if requested_tags.is_empty() {
            return Err(GatewayError::InvalidRequest(
                "tag expression must include at least one tag".to_string(),
            ));
        }

        let effective_models = self.effective_models_for_api_key(api_key).await?;

        effective_models
            .into_iter()
            .filter(|model| {
                requested_tags
                    .iter()
                    .all(|requested_tag| model.tags.iter().any(|tag| tag == requested_tag))
            })
            .sorted_by(|left, right| {
                left.rank
                    .cmp(&right.rank)
                    .then(left.model_key.cmp(&right.model_key))
            })
            .next()
            .ok_or_else(|| RouteError::ModelNotFound(format!("tag:{tag_expression}")).into())
    }

    async fn effective_models_for_api_key(
        &self,
        api_key: &AuthenticatedApiKey,
    ) -> Result<Vec<GatewayModel>, GatewayError> {
        validate_api_key_grant_mode(api_key)?;

        let granted_models = match api_key.model_grant_mode {
            ApiKeyModelGrantMode::All => self.repo.list_models().await?,
            ApiKeyModelGrantMode::Explicit => self.repo.list_models_for_api_key(api_key.id).await?,
        };
        let context = self.access_context_for_api_key(api_key).await?;

        let effective_models = granted_models
            .into_iter()
            .filter(|model| match &context.allowed_model_keys {
                Some(allowed) => allowed.contains(&model.model_key),
                None => true,
            })
            .collect::<Vec<_>>();
        let effective_models = self
            .filter_models_allowed_by_model_allowlists(effective_models, &context)
            .await?
            .into_iter()
            .sorted_by(|left, right| {
                left.rank
                    .cmp(&right.rank)
                    .then(left.model_key.cmp(&right.model_key))
            })
            .collect::<Vec<_>>();

        Ok(effective_models)
    }

    async fn ensure_api_key_can_access_model(
        &self,
        api_key: &AuthenticatedApiKey,
        model: &GatewayModel,
    ) -> Result<(), GatewayError> {
        validate_api_key_grant_mode(api_key)?;

        if api_key.model_grant_mode == ApiKeyModelGrantMode::Explicit {
            let has_explicit_grant = self
                .repo
                .list_models_for_api_key(api_key.id)
                .await?
                .into_iter()
                .any(|granted_model| granted_model.model_key == model.model_key);
            if !has_explicit_grant {
                return Err(AuthError::ModelNotGranted(model.model_key.clone()).into());
            }
        }

        let context = self.access_context_for_api_key(api_key).await?;
        if let Some(allowed_model_keys) = &context.allowed_model_keys
            && !allowed_model_keys.contains(&model.model_key)
        {
            return Err(AuthError::ModelNotGranted(model.model_key.clone()).into());
        }

        self.ensure_model_allowlist_allows(model, &context).await
    }

    async fn filter_models_allowed_by_model_allowlists(
        &self,
        models: Vec<GatewayModel>,
        context: &ApiKeyAccessContext,
    ) -> Result<Vec<GatewayModel>, GatewayError> {
        if models.is_empty() {
            return Ok(models);
        }

        let model_ids = models.iter().map(|model| model.id).collect_vec();
        let policies = self
            .repo
            .list_model_allowlists_for_models(&model_ids)
            .await?;

        Ok(models
            .into_iter()
            .filter(|model| match policies.get(&model.id) {
                Some(policy) => model_allowlist_policy_allows(policy, context),
                None => true,
            })
            .collect())
    }

    async fn ensure_model_allowlist_allows(
        &self,
        model: &GatewayModel,
        context: &ApiKeyAccessContext,
    ) -> Result<(), GatewayError> {
        if let Some(policy) = self.repo.get_model_allowlist(model.id).await?
            && !model_allowlist_policy_allows(&policy, context)
        {
            return Err(AuthError::ModelNotGranted(model.model_key.clone()).into());
        }

        Ok(())
    }

    async fn access_context_for_api_key(
        &self,
        api_key: &AuthenticatedApiKey,
    ) -> Result<ApiKeyAccessContext, GatewayError> {
        let mut allowed_model_keys = None;
        let mut effective_team_id = api_key.owner_team_id;

        if effective_team_id.is_none()
            && let Some(user_id) = api_key.owner_user_id
        {
            effective_team_id = self
                .repo
                .get_team_membership_for_user(user_id)
                .await?
                .map(|membership| membership.team_id);
        }

        let mut effective_team_key = None;
        if let Some(team_id) = effective_team_id {
            let team = self
                .repo
                .get_team_by_id(team_id)
                .await?
                .ok_or(AuthError::ApiKeyOwnerInvalid)?;
            effective_team_key = Some(team.team_key.clone());
            if team.model_access_mode == ModelAccessMode::Restricted {
                let allowed_for_team = self
                    .repo
                    .list_allowed_model_keys_for_team(team_id)
                    .await?
                    .into_iter()
                    .collect::<HashSet<_>>();
                allowed_model_keys = intersect_allowed(allowed_model_keys, allowed_for_team);
            }
        }

        if let Some(service_account_id) = api_key.owner_service_account_id {
            let service_account = self
                .repo
                .get_service_account_by_id(service_account_id)
                .await?
                .ok_or(AuthError::ApiKeyOwnerInvalid)?;
            if service_account.model_access_mode == ModelAccessMode::Restricted {
                let allowed_for_service_account = self
                    .repo
                    .list_allowed_model_keys_for_service_account(service_account_id)
                    .await?
                    .into_iter()
                    .collect::<HashSet<_>>();
                allowed_model_keys =
                    intersect_allowed(allowed_model_keys, allowed_for_service_account);
            }
        }

        let mut user_email_normalized = None;
        if let Some(user_id) = api_key.owner_user_id {
            let user = self
                .repo
                .get_user_by_id(user_id)
                .await?
                .ok_or(AuthError::ApiKeyOwnerInvalid)?;
            if user.status != UserStatus::Active {
                return Err(AuthError::ApiKeyOwnerInvalid.into());
            }
            user_email_normalized = Some(user.email_normalized.clone());
            if user.model_access_mode == ModelAccessMode::Restricted {
                let allowed_for_user = self
                    .repo
                    .list_allowed_model_keys_for_user(user_id)
                    .await?
                    .into_iter()
                    .collect::<HashSet<_>>();
                allowed_model_keys = intersect_allowed(allowed_model_keys, allowed_for_user);
            }
        }

        Ok(ApiKeyAccessContext {
            allowed_model_keys,
            user_email_normalized,
            effective_team_key,
            service_account_owned: api_key.owner_kind == ApiKeyOwnerKind::ServiceAccount,
        })
    }
}

struct ApiKeyAccessContext {
    allowed_model_keys: Option<HashSet<String>>,
    user_email_normalized: Option<String>,
    effective_team_key: Option<String>,
    service_account_owned: bool,
}

fn model_allowlist_policy_allows(
    policy: &ModelAllowlistPolicy,
    context: &ApiKeyAccessContext,
) -> bool {
    // v1 policy: service-account-owned keys are never granted model-level allowlists.
    if context.service_account_owned {
        return false;
    }

    context
        .user_email_normalized
        .as_ref()
        .is_some_and(|email| policy.users.iter().any(|allowed| allowed == email))
        || context
            .effective_team_key
            .as_ref()
            .is_some_and(|team_key| policy.teams.iter().any(|allowed| allowed == team_key))
}

fn validate_api_key_grant_mode(api_key: &AuthenticatedApiKey) -> Result<(), GatewayError> {
    if api_key.owner_kind == ApiKeyOwnerKind::ServiceAccount
        && api_key.model_grant_mode == ApiKeyModelGrantMode::All
    {
        return Err(AuthError::ApiKeyOwnerInvalid.into());
    }

    Ok(())
}

fn intersect_allowed(
    current: Option<HashSet<String>>,
    next: HashSet<String>,
) -> Option<HashSet<String>> {
    match current {
        None => Some(next),
        Some(existing) => Some(existing.intersection(&next).cloned().collect()),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
    };

    use async_trait::async_trait;
    use gateway_core::{
        ApiKeyModelGrantMode, ApiKeyOwnerKind, AuthMode, AuthenticatedApiKey, GatewayModel,
        GlobalRole, IdentityRepository, MembershipRole, ModelAccessMode, ModelAllowlistPolicy,
        ModelRepository, ModelRoute, ServiceAccountRecord, ServiceAccountStatus, StoreError,
        TeamMembershipRecord, TeamRecord, UserRecord, UserStatus,
    };
    use serde_json::json;
    use time::OffsetDateTime;
    use uuid::Uuid;

    use super::ModelAccess;

    #[derive(Default)]
    struct AccessRepo {
        models: Mutex<Vec<GatewayModel>>,
        list_models_calls: Mutex<usize>,
        grants_by_api_key: Mutex<HashMap<Uuid, Vec<String>>>,
        model_allowlists: Mutex<HashMap<Uuid, ModelAllowlistPolicy>>,
        list_model_allowlists_calls: Mutex<Vec<Vec<Uuid>>>,
        get_model_allowlist_calls: Mutex<Vec<Uuid>>,
        teams: Mutex<HashMap<Uuid, TeamRecord>>,
        users: Mutex<HashMap<Uuid, UserRecord>>,
        memberships: Mutex<HashMap<Uuid, TeamMembershipRecord>>,
        service_accounts: Mutex<HashMap<Uuid, ServiceAccountRecord>>,
        team_allowlists: Mutex<HashMap<Uuid, Vec<String>>>,
        user_allowlists: Mutex<HashMap<Uuid, Vec<String>>>,
        service_account_allowlists: Mutex<HashMap<Uuid, Vec<String>>>,
    }

    #[async_trait]
    impl ModelRepository for AccessRepo {
        async fn list_models(&self) -> Result<Vec<GatewayModel>, StoreError> {
            *self
                .list_models_calls
                .lock()
                .expect("list models calls lock") += 1;
            Ok(self.models.lock().expect("models lock").clone())
        }

        async fn get_model_by_key(
            &self,
            model_key: &str,
        ) -> Result<Option<GatewayModel>, StoreError> {
            Ok(self
                .models
                .lock()
                .expect("models lock")
                .iter()
                .find(|model| model.model_key == model_key)
                .cloned())
        }

        async fn list_models_for_api_key(
            &self,
            api_key_id: Uuid,
        ) -> Result<Vec<GatewayModel>, StoreError> {
            let grants = self
                .grants_by_api_key
                .lock()
                .expect("grants lock")
                .get(&api_key_id)
                .cloned()
                .unwrap_or_default();
            Ok(self
                .models
                .lock()
                .expect("models lock")
                .iter()
                .filter(|model| grants.iter().any(|grant| grant == &model.model_key))
                .cloned()
                .collect())
        }

        async fn list_model_allowlists_for_models(
            &self,
            model_ids: &[Uuid],
        ) -> Result<HashMap<Uuid, ModelAllowlistPolicy>, StoreError> {
            self.list_model_allowlists_calls
                .lock()
                .expect("list model allowlists calls lock")
                .push(model_ids.to_vec());
            let policies = self.model_allowlists.lock().expect("model allowlists lock");
            Ok(model_ids
                .iter()
                .filter_map(|model_id| {
                    policies
                        .get(model_id)
                        .cloned()
                        .map(|policy| (*model_id, policy))
                })
                .collect())
        }

        async fn get_model_allowlist(
            &self,
            model_id: Uuid,
        ) -> Result<Option<ModelAllowlistPolicy>, StoreError> {
            self.get_model_allowlist_calls
                .lock()
                .expect("get model allowlist calls lock")
                .push(model_id);
            Ok(self
                .model_allowlists
                .lock()
                .expect("model allowlists lock")
                .get(&model_id)
                .cloned())
        }

        async fn list_routes_for_model(
            &self,
            _model_id: Uuid,
        ) -> Result<Vec<ModelRoute>, StoreError> {
            Ok(Vec::new())
        }
    }

    #[async_trait]
    impl IdentityRepository for AccessRepo {
        async fn get_user_by_id(&self, user_id: Uuid) -> Result<Option<UserRecord>, StoreError> {
            Ok(self
                .users
                .lock()
                .expect("users lock")
                .get(&user_id)
                .cloned())
        }

        async fn get_team_by_id(&self, team_id: Uuid) -> Result<Option<TeamRecord>, StoreError> {
            Ok(self
                .teams
                .lock()
                .expect("teams lock")
                .get(&team_id)
                .cloned())
        }

        async fn get_service_account_by_id(
            &self,
            service_account_id: Uuid,
        ) -> Result<Option<ServiceAccountRecord>, StoreError> {
            Ok(self
                .service_accounts
                .lock()
                .expect("service accounts lock")
                .get(&service_account_id)
                .cloned())
        }

        async fn get_team_membership_for_user(
            &self,
            user_id: Uuid,
        ) -> Result<Option<TeamMembershipRecord>, StoreError> {
            Ok(self
                .memberships
                .lock()
                .expect("memberships lock")
                .get(&user_id)
                .cloned())
        }

        async fn list_allowed_model_keys_for_user(
            &self,
            user_id: Uuid,
        ) -> Result<Vec<String>, StoreError> {
            Ok(self
                .user_allowlists
                .lock()
                .expect("user allowlists lock")
                .get(&user_id)
                .cloned()
                .unwrap_or_default())
        }

        async fn list_allowed_model_keys_for_team(
            &self,
            team_id: Uuid,
        ) -> Result<Vec<String>, StoreError> {
            Ok(self
                .team_allowlists
                .lock()
                .expect("team allowlists lock")
                .get(&team_id)
                .cloned()
                .unwrap_or_default())
        }

        async fn list_allowed_model_keys_for_service_account(
            &self,
            service_account_id: Uuid,
        ) -> Result<Vec<String>, StoreError> {
            Ok(self
                .service_account_allowlists
                .lock()
                .expect("service account allowlists lock")
                .get(&service_account_id)
                .cloned()
                .unwrap_or_default())
        }
    }

    #[tokio::test]
    async fn all_mode_uses_current_model_catalog() {
        let repo = Arc::new(AccessRepo::default());
        repo.models
            .lock()
            .expect("models lock")
            .extend([model("fast", 10), model("reasoning", 20)]);

        let access = ModelAccess::new(repo.clone());
        let auth = user_auth(ApiKeyModelGrantMode::All, Uuid::new_v4(), None);

        assert_eq!(
            model_keys(access.list_models_for_api_key(&auth).await.expect("models")),
            ["fast", "reasoning"]
        );

        repo.models
            .lock()
            .expect("models lock")
            .push(model("new-model", 30));

        assert_eq!(
            model_keys(access.list_models_for_api_key(&auth).await.expect("models")),
            ["fast", "reasoning", "new-model"]
        );
    }

    #[tokio::test]
    async fn explicit_mode_uses_stored_grants_only() {
        let repo = Arc::new(AccessRepo::default());
        let api_key_id = Uuid::new_v4();
        repo.models
            .lock()
            .expect("models lock")
            .extend([model("fast", 10), model("reasoning", 20)]);
        repo.grants_by_api_key
            .lock()
            .expect("grants lock")
            .insert(api_key_id, vec!["fast".to_string()]);

        let access = ModelAccess::new(repo);
        let auth = user_auth(ApiKeyModelGrantMode::Explicit, api_key_id, None);

        assert_eq!(
            model_keys(access.list_models_for_api_key(&auth).await.expect("models")),
            ["fast"]
        );
    }

    #[tokio::test]
    async fn all_mode_intersects_team_and_user_restrictions() {
        let repo = Arc::new(AccessRepo::default());
        let team_id = Uuid::new_v4();
        let user_id = Uuid::new_v4();
        repo.models.lock().expect("models lock").extend([
            model("fast", 10),
            model("reasoning", 20),
            model("opus", 30),
        ]);
        repo.teams
            .lock()
            .expect("teams lock")
            .insert(team_id, team(team_id, ModelAccessMode::Restricted));
        repo.users
            .lock()
            .expect("users lock")
            .insert(user_id, user(user_id, ModelAccessMode::Restricted));
        repo.memberships.lock().expect("memberships lock").insert(
            user_id,
            TeamMembershipRecord {
                team_id,
                user_id,
                role: MembershipRole::Member,
                created_at: OffsetDateTime::now_utc(),
                updated_at: OffsetDateTime::now_utc(),
            },
        );
        repo.team_allowlists
            .lock()
            .expect("team allowlists lock")
            .insert(team_id, vec!["fast".to_string(), "reasoning".to_string()]);
        repo.user_allowlists
            .lock()
            .expect("user allowlists lock")
            .insert(user_id, vec!["reasoning".to_string(), "opus".to_string()]);

        let access = ModelAccess::new(repo);
        let auth = user_auth(ApiKeyModelGrantMode::All, Uuid::new_v4(), Some(user_id));

        assert_eq!(
            model_keys(access.list_models_for_api_key(&auth).await.expect("models")),
            ["reasoning"]
        );
    }

    #[tokio::test]
    async fn service_account_all_mode_is_rejected_at_runtime() {
        let repo = Arc::new(AccessRepo::default());
        let team_id = Uuid::new_v4();
        let service_account_id = Uuid::new_v4();
        repo.models
            .lock()
            .expect("models lock")
            .extend([model("fast", 10), model("reasoning", 20)]);
        repo.teams
            .lock()
            .expect("teams lock")
            .insert(team_id, team(team_id, ModelAccessMode::All));
        repo.service_accounts
            .lock()
            .expect("service accounts lock")
            .insert(
                service_account_id,
                service_account(service_account_id, team_id, ModelAccessMode::Restricted),
            );
        repo.service_account_allowlists
            .lock()
            .expect("service account allowlists lock")
            .insert(service_account_id, vec!["fast".to_string()]);

        let access = ModelAccess::new(repo);
        let auth = AuthenticatedApiKey {
            id: Uuid::new_v4(),
            public_id: "dev123".to_string(),
            name: "dev".to_string(),
            model_grant_mode: ApiKeyModelGrantMode::All,
            owner_kind: ApiKeyOwnerKind::ServiceAccount,
            owner_user_id: None,
            owner_team_id: Some(team_id),
            owner_service_account_id: Some(service_account_id),
        };

        let error = access
            .list_models_for_api_key(&auth)
            .await
            .expect_err("service-account all-mode keys should be invalid");
        assert_eq!(error.error_code(), "api_key_owner_invalid");
    }

    #[tokio::test]
    async fn all_mode_single_model_resolution_does_not_load_catalog() {
        let repo = Arc::new(AccessRepo::default());
        repo.models
            .lock()
            .expect("models lock")
            .extend([model("fast", 10), model("reasoning", 20)]);

        let access = ModelAccess::new(repo.clone());
        let auth = user_auth(ApiKeyModelGrantMode::All, Uuid::new_v4(), None);

        let model = access
            .resolve_requested_model(&auth, "fast")
            .await
            .expect("resolve model");

        assert_eq!(model.model_key, "fast");
        assert_eq!(
            *repo
                .list_models_calls
                .lock()
                .expect("list models calls lock"),
            0
        );
    }

    #[tokio::test]
    async fn direct_model_allowlist_allows_matching_user_without_loading_catalog() {
        let repo = Arc::new(AccessRepo::default());
        let user_id = Uuid::new_v4();
        let fast = model("fast", 10);
        repo.models.lock().expect("models lock").push(fast.clone());
        repo.users
            .lock()
            .expect("users lock")
            .insert(user_id, user(user_id, ModelAccessMode::All));
        repo.model_allowlists
            .lock()
            .expect("model allowlists lock")
            .insert(
                fast.id,
                ModelAllowlistPolicy {
                    users: vec!["user@example.com".to_string()],
                    teams: Vec::new(),
                },
            );

        let access = ModelAccess::new(repo.clone());
        let auth = user_auth(ApiKeyModelGrantMode::All, Uuid::new_v4(), Some(user_id));

        let resolved = access
            .resolve_requested_model(&auth, "fast")
            .await
            .expect("resolve model");

        assert_eq!(resolved.model_key, "fast");
        assert_eq!(
            *repo
                .list_models_calls
                .lock()
                .expect("list models calls lock"),
            0
        );
        assert_eq!(
            *repo
                .get_model_allowlist_calls
                .lock()
                .expect("get model allowlist calls lock"),
            [fast.id]
        );
    }

    #[tokio::test]
    async fn direct_model_allowlist_denies_non_matching_user() {
        let repo = Arc::new(AccessRepo::default());
        let user_id = Uuid::new_v4();
        let fast = model("fast", 10);
        repo.models.lock().expect("models lock").push(fast.clone());
        repo.users
            .lock()
            .expect("users lock")
            .insert(user_id, user(user_id, ModelAccessMode::All));
        repo.model_allowlists
            .lock()
            .expect("model allowlists lock")
            .insert(
                fast.id,
                ModelAllowlistPolicy {
                    users: vec!["other@example.com".to_string()],
                    teams: Vec::new(),
                },
            );

        let access = ModelAccess::new(repo);
        let auth = user_auth(ApiKeyModelGrantMode::All, Uuid::new_v4(), Some(user_id));

        let error = access
            .resolve_requested_model(&auth, "fast")
            .await
            .expect_err("model allowlist should deny non-matching user");

        assert_eq!(error.error_code(), "model_not_granted");
    }

    #[tokio::test]
    async fn list_models_filters_model_allowlists_by_user_email_and_team_key() {
        let repo = Arc::new(AccessRepo::default());
        let team_id = Uuid::new_v4();
        let user_id = Uuid::new_v4();
        let user_model = model("user-model", 10);
        let team_model = model("team-model", 20);
        let blocked_model = model("blocked-model", 30);
        let open_model = model("open-model", 40);
        repo.models.lock().expect("models lock").extend([
            user_model.clone(),
            team_model.clone(),
            blocked_model.clone(),
            open_model.clone(),
        ]);
        repo.teams
            .lock()
            .expect("teams lock")
            .insert(team_id, team(team_id, ModelAccessMode::All));
        repo.users
            .lock()
            .expect("users lock")
            .insert(user_id, user(user_id, ModelAccessMode::All));
        repo.memberships.lock().expect("memberships lock").insert(
            user_id,
            TeamMembershipRecord {
                team_id,
                user_id,
                role: MembershipRole::Member,
                created_at: OffsetDateTime::now_utc(),
                updated_at: OffsetDateTime::now_utc(),
            },
        );
        repo.model_allowlists
            .lock()
            .expect("model allowlists lock")
            .extend([
                (
                    user_model.id,
                    ModelAllowlistPolicy {
                        users: vec!["user@example.com".to_string()],
                        teams: Vec::new(),
                    },
                ),
                (
                    team_model.id,
                    ModelAllowlistPolicy {
                        users: Vec::new(),
                        teams: vec!["team".to_string()],
                    },
                ),
                (
                    blocked_model.id,
                    ModelAllowlistPolicy {
                        users: vec!["other@example.com".to_string()],
                        teams: vec!["other-team".to_string()],
                    },
                ),
            ]);

        let access = ModelAccess::new(repo.clone());
        let auth = user_auth(ApiKeyModelGrantMode::All, Uuid::new_v4(), Some(user_id));

        assert_eq!(
            model_keys(access.list_models_for_api_key(&auth).await.expect("models")),
            ["user-model", "team-model", "open-model"]
        );
        assert_eq!(
            repo.list_model_allowlists_calls
                .lock()
                .expect("list model allowlists calls lock")
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn service_account_keys_skip_models_with_model_allowlists() {
        let repo = Arc::new(AccessRepo::default());
        let team_id = Uuid::new_v4();
        let service_account_id = Uuid::new_v4();
        let api_key_id = Uuid::new_v4();
        let blocked = model("blocked", 10);
        let open = model("open", 20);
        repo.models
            .lock()
            .expect("models lock")
            .extend([blocked.clone(), open.clone()]);
        repo.teams
            .lock()
            .expect("teams lock")
            .insert(team_id, team(team_id, ModelAccessMode::All));
        repo.service_accounts
            .lock()
            .expect("service accounts lock")
            .insert(
                service_account_id,
                service_account(service_account_id, team_id, ModelAccessMode::All),
            );
        repo.grants_by_api_key
            .lock()
            .expect("grants lock")
            .insert(api_key_id, vec!["blocked".to_string(), "open".to_string()]);
        repo.model_allowlists
            .lock()
            .expect("model allowlists lock")
            .insert(
                blocked.id,
                ModelAllowlistPolicy {
                    users: Vec::new(),
                    teams: vec!["team".to_string()],
                },
            );

        let access = ModelAccess::new(repo);
        let auth = service_account_auth(api_key_id, team_id, service_account_id);

        assert_eq!(
            model_keys(access.list_models_for_api_key(&auth).await.expect("models")),
            ["open"]
        );

        let error = access
            .resolve_requested_model(&auth, "blocked")
            .await
            .expect_err("service account should be denied when model allowlist exists");
        assert_eq!(error.error_code(), "model_not_granted");
        assert_eq!(
            access
                .resolve_requested_model(&auth, "open")
                .await
                .expect("open model")
                .model_key,
            "open"
        );
    }

    #[tokio::test]
    async fn tag_resolution_skips_candidates_blocked_by_model_allowlist() {
        let repo = Arc::new(AccessRepo::default());
        let user_id = Uuid::new_v4();
        let mut blocked = model("blocked", 10);
        blocked.tags = vec!["chat".to_string()];
        let mut allowed = model("allowed", 20);
        allowed.tags = vec!["chat".to_string()];
        repo.models
            .lock()
            .expect("models lock")
            .extend([blocked.clone(), allowed.clone()]);
        repo.users
            .lock()
            .expect("users lock")
            .insert(user_id, user(user_id, ModelAccessMode::All));
        repo.model_allowlists
            .lock()
            .expect("model allowlists lock")
            .extend([
                (
                    blocked.id,
                    ModelAllowlistPolicy {
                        users: vec!["other@example.com".to_string()],
                        teams: Vec::new(),
                    },
                ),
                (
                    allowed.id,
                    ModelAllowlistPolicy {
                        users: vec!["user@example.com".to_string()],
                        teams: Vec::new(),
                    },
                ),
            ]);

        let access = ModelAccess::new(repo);
        let auth = user_auth(ApiKeyModelGrantMode::All, Uuid::new_v4(), Some(user_id));

        let resolved = access
            .resolve_requested_model(&auth, "tag:chat")
            .await
            .expect("resolve tag");

        assert_eq!(resolved.model_key, "allowed");
    }

    #[tokio::test]
    async fn alias_model_allowlists_do_not_inherit_from_targets() {
        let repo = Arc::new(AccessRepo::default());
        let user_id = Uuid::new_v4();
        let mut alias = model("alias", 10);
        alias.alias_target_model_key = Some("target".to_string());
        let target = model("target", 20);
        repo.models
            .lock()
            .expect("models lock")
            .extend([alias.clone(), target.clone()]);
        repo.users
            .lock()
            .expect("users lock")
            .insert(user_id, user(user_id, ModelAccessMode::All));
        repo.model_allowlists
            .lock()
            .expect("model allowlists lock")
            .insert(
                target.id,
                ModelAllowlistPolicy {
                    users: vec!["other@example.com".to_string()],
                    teams: Vec::new(),
                },
            );

        let access = ModelAccess::new(repo.clone());
        let auth = user_auth(ApiKeyModelGrantMode::All, Uuid::new_v4(), Some(user_id));

        assert_eq!(
            model_keys(access.list_models_for_api_key(&auth).await.expect("models")),
            ["alias"]
        );
        assert_eq!(
            access
                .resolve_requested_model(&auth, "alias")
                .await
                .expect("alias should not inherit target policy")
                .model_key,
            "alias"
        );
        assert_eq!(
            access
                .resolve_requested_model(&auth, "target")
                .await
                .expect_err("target policy should still apply")
                .error_code(),
            "model_not_granted"
        );

        repo.model_allowlists
            .lock()
            .expect("model allowlists lock")
            .extend([
                (
                    alias.id,
                    ModelAllowlistPolicy {
                        users: vec!["other@example.com".to_string()],
                        teams: Vec::new(),
                    },
                ),
                (
                    target.id,
                    ModelAllowlistPolicy {
                        users: vec!["user@example.com".to_string()],
                        teams: Vec::new(),
                    },
                ),
            ]);

        assert_eq!(
            model_keys(access.list_models_for_api_key(&auth).await.expect("models")),
            ["target"]
        );
        assert_eq!(
            access
                .resolve_requested_model(&auth, "alias")
                .await
                .expect_err("alias policy should not inherit target access")
                .error_code(),
            "model_not_granted"
        );
        assert_eq!(
            access
                .resolve_requested_model(&auth, "target")
                .await
                .expect("target should use its own policy")
                .model_key,
            "target"
        );
    }
    fn model(model_key: &str, rank: i32) -> GatewayModel {
        GatewayModel {
            id: Uuid::new_v4(),
            model_key: model_key.to_string(),
            alias_target_model_key: None,
            max_reasoning_effort: None,
            description: None,
            tags: Vec::new(),
            rank,
        }
    }

    fn model_keys(models: Vec<GatewayModel>) -> Vec<String> {
        models.into_iter().map(|model| model.model_key).collect()
    }

    fn user_auth(
        model_grant_mode: ApiKeyModelGrantMode,
        api_key_id: Uuid,
        user_id: Option<Uuid>,
    ) -> AuthenticatedApiKey {
        AuthenticatedApiKey {
            id: api_key_id,
            public_id: "dev123".to_string(),
            name: "dev".to_string(),
            model_grant_mode,
            owner_kind: ApiKeyOwnerKind::User,
            owner_user_id: user_id,
            owner_team_id: None,
            owner_service_account_id: None,
        }
    }

    fn service_account_auth(
        api_key_id: Uuid,
        team_id: Uuid,
        service_account_id: Uuid,
    ) -> AuthenticatedApiKey {
        AuthenticatedApiKey {
            id: api_key_id,
            public_id: "dev123".to_string(),
            name: "dev".to_string(),
            model_grant_mode: ApiKeyModelGrantMode::Explicit,
            owner_kind: ApiKeyOwnerKind::ServiceAccount,
            owner_user_id: None,
            owner_team_id: Some(team_id),
            owner_service_account_id: Some(service_account_id),
        }
    }

    fn team(team_id: Uuid, model_access_mode: ModelAccessMode) -> TeamRecord {
        let now = OffsetDateTime::now_utc();
        TeamRecord {
            team_id,
            team_key: "team".to_string(),
            team_name: "Team".to_string(),
            status: "active".to_string(),
            model_access_mode,
            tags: Vec::new(),
            created_at: now,
            updated_at: now,
        }
    }

    fn user(user_id: Uuid, model_access_mode: ModelAccessMode) -> UserRecord {
        let now = OffsetDateTime::now_utc();
        UserRecord {
            user_id,
            name: "User".to_string(),
            email: "user@example.com".to_string(),
            email_normalized: "user@example.com".to_string(),
            global_role: GlobalRole::User,
            auth_mode: AuthMode::Password,
            status: UserStatus::Active,
            must_change_password: false,
            request_logging_enabled: true,
            model_access_mode,
            tags: Vec::new(),
            created_at: now,
            updated_at: now,
        }
    }

    fn service_account(
        service_account_id: Uuid,
        team_id: Uuid,
        model_access_mode: ModelAccessMode,
    ) -> ServiceAccountRecord {
        let now = OffsetDateTime::now_utc();
        ServiceAccountRecord {
            service_account_id,
            team_id,
            service_account_key: "service".to_string(),
            service_account_name: "Service".to_string(),
            status: ServiceAccountStatus::Active,
            model_access_mode,
            metadata: json!({}),
            tags: Vec::new(),
            created_at: now,
            updated_at: now,
            disabled_at: None,
        }
    }
}
