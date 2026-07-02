use std::{collections::HashSet, sync::Arc};

use gateway_core::{
    ApiKeyModelGrantMode, AuthError, AuthenticatedApiKey, GatewayError, GatewayModel,
    IdentityRepository, ModelAccessMode, ModelRepository, RouteError, UserStatus,
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

        let effective_models = self.effective_models_for_api_key(api_key).await?;
        let has_grant = effective_models
            .iter()
            .any(|granted| granted.model_key == requested_model);

        if !has_grant {
            return Err(AuthError::ModelNotGranted(requested_model.to_string()).into());
        }

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
        let granted_models = match api_key.model_grant_mode {
            ApiKeyModelGrantMode::All => self.repo.list_models().await?,
            ApiKeyModelGrantMode::Explicit => self.repo.list_models_for_api_key(api_key.id).await?,
        };
        let mut allowed_model_keys: Option<HashSet<String>> = None;
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

        if let Some(team_id) = effective_team_id {
            let team = self
                .repo
                .get_team_by_id(team_id)
                .await?
                .ok_or(AuthError::ApiKeyOwnerInvalid)?;
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

        if let Some(user_id) = api_key.owner_user_id {
            let user = self
                .repo
                .get_user_by_id(user_id)
                .await?
                .ok_or(AuthError::ApiKeyOwnerInvalid)?;
            if user.status != UserStatus::Active {
                return Err(AuthError::ApiKeyOwnerInvalid.into());
            }
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

        let effective_models = granted_models
            .into_iter()
            .filter(|model| match &allowed_model_keys {
                Some(allowed) => allowed.contains(&model.model_key),
                None => true,
            })
            .sorted_by(|left, right| {
                left.rank
                    .cmp(&right.rank)
                    .then(left.model_key.cmp(&right.model_key))
            })
            .collect::<Vec<_>>();

        Ok(effective_models)
    }
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
        GlobalRole, IdentityRepository, MembershipRole, ModelAccessMode, ModelRepository,
        ModelRoute, ServiceAccountRecord, ServiceAccountStatus, StoreError, TeamMembershipRecord,
        TeamRecord, UserRecord, UserStatus,
    };
    use serde_json::json;
    use time::OffsetDateTime;
    use uuid::Uuid;

    use super::ModelAccess;

    #[derive(Default)]
    struct AccessRepo {
        models: Mutex<Vec<GatewayModel>>,
        grants_by_api_key: Mutex<HashMap<Uuid, Vec<String>>>,
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
    async fn all_mode_intersects_service_account_restrictions() {
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

        assert_eq!(
            model_keys(access.list_models_for_api_key(&auth).await.expect("models")),
            ["fast"]
        );
    }

    fn model(model_key: &str, rank: i32) -> GatewayModel {
        GatewayModel {
            id: Uuid::new_v4(),
            model_key: model_key.to_string(),
            alias_target_model_key: None,
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
            created_at: now,
            updated_at: now,
            disabled_at: None,
        }
    }
}
