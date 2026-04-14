use std::{
    collections::{BTreeSet, HashMap},
    sync::Arc,
};

use gateway_core::{
    AdminApiKeyRepository, AdminIdentityRepository, ApiKeyOwnerKind, ApiKeyRecord, ApiKeyStatus,
    GatewayError, GatewayModel, IdentityUserRecord, ModelRepository, NewApiKeyRecord, StoreError,
    TeamRecord, UserStatus,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::hash_gateway_key_secret;

#[derive(Debug, Clone)]
pub struct AdminApiKeyService<R> {
    repo: Arc<R>,
}

#[derive(Debug, Clone)]
pub struct AdminApiKeysPayload {
    pub items: Vec<AdminApiKeySummary>,
    pub users: Vec<AdminApiKeyUserOwner>,
    pub teams: Vec<AdminApiKeyTeamOwner>,
    pub models: Vec<AdminApiKeyModelOption>,
}

#[derive(Debug, Clone)]
pub struct AdminApiKeySummary {
    pub id: Uuid,
    pub name: String,
    pub prefix: String,
    pub status: ApiKeyStatus,
    pub owner_kind: ApiKeyOwnerKind,
    pub owner_id: Uuid,
    pub owner_name: String,
    pub owner_email: Option<String>,
    pub owner_team_key: Option<String>,
    pub model_keys: Vec<String>,
    pub created_at: OffsetDateTime,
    pub last_used_at: Option<OffsetDateTime>,
    pub revoked_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone)]
pub struct AdminApiKeyUserOwner {
    pub id: Uuid,
    pub name: String,
    pub email: String,
}

#[derive(Debug, Clone)]
pub struct AdminApiKeyTeamOwner {
    pub id: Uuid,
    pub name: String,
    pub key: String,
}

#[derive(Debug, Clone)]
pub struct AdminApiKeyModelOption {
    pub id: Uuid,
    pub key: String,
    pub description: Option<String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct CreateAdminApiKeyInput {
    pub name: String,
    pub owner_kind: String,
    pub owner_user_id: Option<String>,
    pub owner_team_id: Option<String>,
    pub model_keys: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct CreateAdminApiKeyResult {
    pub api_key: AdminApiKeySummary,
    pub raw_key: String,
}

#[derive(Debug, Clone)]
pub struct UpdateAdminApiKeyInput {
    pub model_keys: Vec<String>,
}

impl<R> AdminApiKeyService<R>
where
    R: AdminApiKeyRepository + AdminIdentityRepository + ModelRepository + Send + Sync + 'static,
{
    #[must_use]
    pub fn new(repo: Arc<R>) -> Self {
        Self { repo }
    }

    pub async fn list_api_keys(&self) -> Result<AdminApiKeysPayload, GatewayError> {
        let api_keys = self.repo.list_api_keys().await?;
        let users = self.repo.list_identity_users().await?;
        let active_teams = self.repo.list_active_teams().await?;
        let teams = self.repo.list_teams().await?;
        let models = self.repo.list_models().await?;

        let mut items = Vec::with_capacity(api_keys.len());
        for api_key in api_keys {
            let granted_models = self.repo.list_models_for_api_key(api_key.id).await?;
            items.push(build_api_key_summary(
                &api_key,
                &users,
                &teams,
                &granted_models,
            )?);
        }

        Ok(AdminApiKeysPayload {
            items,
            users: users
                .iter()
                .filter(|user| user.user.status == UserStatus::Active)
                .map(|user| AdminApiKeyUserOwner {
                    id: user.user.user_id,
                    name: user.user.name.clone(),
                    email: user.user.email.clone(),
                })
                .collect(),
            teams: active_teams
                .iter()
                .map(|team| AdminApiKeyTeamOwner {
                    id: team.team_id,
                    name: team.team_name.clone(),
                    key: team.team_key.clone(),
                })
                .collect(),
            models: models
                .into_iter()
                .map(|model| AdminApiKeyModelOption {
                    id: model.id,
                    key: model.model_key,
                    description: model.description,
                    tags: model.tags,
                })
                .collect(),
        })
    }

    pub async fn create_api_key(
        &self,
        request: CreateAdminApiKeyInput,
    ) -> Result<CreateAdminApiKeyResult, GatewayError> {
        let users = self.repo.list_identity_users().await?;
        let active_teams = self.repo.list_active_teams().await?;
        let teams = self.repo.list_teams().await?;
        let models = self.repo.list_models().await?;

        let name = request.name.trim();
        if name.is_empty() {
            return Err(GatewayError::InvalidRequest(
                "api key name is required".to_string(),
            ));
        }

        let owner_kind = ApiKeyOwnerKind::from_db(request.owner_kind.trim()).ok_or_else(|| {
            GatewayError::InvalidRequest("owner_kind must be `user` or `team`".to_string())
        })?;
        let (owner_user_id, owner_team_id) =
            validate_owner(&request, owner_kind, &users, &active_teams)?;
        let granted_models = select_granted_models(&request.model_keys, &models)?;

        let public_id = Uuid::new_v4().simple().to_string();
        let secret = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        let raw_key = format!("gwk_{public_id}.{secret}");
        let secret_hash = hash_gateway_key_secret(&secret)
            .map_err(|error| GatewayError::Internal(error.to_string()))?;
        let now = OffsetDateTime::now_utc();

        let api_key = self
            .repo
            .create_api_key(&NewApiKeyRecord {
                name: name.to_string(),
                public_id,
                secret_hash,
                owner_kind,
                owner_user_id,
                owner_team_id,
                created_at: now,
            })
            .await?;
        let model_ids = granted_models
            .iter()
            .map(|model| model.id)
            .collect::<Vec<_>>();
        self.repo
            .replace_api_key_model_grants(api_key.id, &model_ids)
            .await?;

        let granted_models = self.repo.list_models_for_api_key(api_key.id).await?;
        let api_key = build_api_key_summary(&api_key, &users, &teams, &granted_models)?;

        Ok(CreateAdminApiKeyResult { api_key, raw_key })
    }

    pub async fn revoke_api_key(
        &self,
        api_key_id: Uuid,
    ) -> Result<AdminApiKeySummary, GatewayError> {
        self.repo
            .get_api_key_by_id(api_key_id)
            .await?
            .ok_or_else(|| StoreError::NotFound(format!("api key `{api_key_id}`")))?;

        self.repo
            .revoke_api_key(api_key_id, OffsetDateTime::now_utc())
            .await?;

        let users = self.repo.list_identity_users().await?;
        let teams = self.repo.list_teams().await?;
        let api_key = self
            .repo
            .get_api_key_by_id(api_key_id)
            .await?
            .ok_or_else(|| {
                GatewayError::Internal(format!("api key `{api_key_id}` missing after revoke"))
            })?;
        let granted_models = self.repo.list_models_for_api_key(api_key.id).await?;

        build_api_key_summary(&api_key, &users, &teams, &granted_models)
    }

    pub async fn update_api_key(
        &self,
        api_key_id: Uuid,
        request: UpdateAdminApiKeyInput,
    ) -> Result<AdminApiKeySummary, GatewayError> {
        let users = self.repo.list_identity_users().await?;
        let teams = self.repo.list_teams().await?;
        let models = self.repo.list_models().await?;
        let api_key = self
            .repo
            .get_api_key_by_id(api_key_id)
            .await?
            .ok_or_else(|| StoreError::NotFound(format!("api key `{api_key_id}`")))?;

        if api_key.status != ApiKeyStatus::Active {
            return Err(GatewayError::InvalidRequest(
                "revoked api keys cannot be updated".to_string(),
            ));
        }

        let granted_models = select_granted_models(&request.model_keys, &models)?;
        let model_ids = granted_models
            .iter()
            .map(|model| model.id)
            .collect::<Vec<_>>();
        self.repo
            .replace_api_key_model_grants(api_key.id, &model_ids)
            .await?;

        let granted_models = self.repo.list_models_for_api_key(api_key.id).await?;
        build_api_key_summary(&api_key, &users, &teams, &granted_models)
    }
}

fn build_api_key_summary(
    api_key: &ApiKeyRecord,
    users: &[IdentityUserRecord],
    teams: &[TeamRecord],
    granted_models: &[GatewayModel],
) -> Result<AdminApiKeySummary, GatewayError> {
    let user_map = users
        .iter()
        .map(|user| (user.user.user_id, user))
        .collect::<HashMap<_, _>>();
    let team_map = teams
        .iter()
        .map(|team| (team.team_id, team))
        .collect::<HashMap<_, _>>();

    let (owner_id, owner_name, owner_email, owner_team_key) = match api_key.owner_kind {
        ApiKeyOwnerKind::User => {
            let owner_id = api_key.owner_user_id.ok_or_else(|| {
                GatewayError::Internal(format!("api key `{}` is missing owner_user_id", api_key.id))
            })?;
            let owner = user_map.get(&owner_id).ok_or_else(|| {
                GatewayError::Internal(format!(
                    "api key `{}` references missing user `{owner_id}`",
                    api_key.id
                ))
            })?;
            (
                owner_id,
                owner.user.name.clone(),
                Some(owner.user.email.clone()),
                None,
            )
        }
        ApiKeyOwnerKind::Team => {
            let owner_id = api_key.owner_team_id.ok_or_else(|| {
                GatewayError::Internal(format!("api key `{}` is missing owner_team_id", api_key.id))
            })?;
            let owner = team_map.get(&owner_id).ok_or_else(|| {
                GatewayError::Internal(format!(
                    "api key `{}` references missing team `{owner_id}`",
                    api_key.id
                ))
            })?;
            (
                owner_id,
                owner.team_name.clone(),
                None,
                Some(owner.team_key.clone()),
            )
        }
    };

    Ok(AdminApiKeySummary {
        id: api_key.id,
        name: api_key.name.clone(),
        prefix: format!("gwk_{}", api_key.public_id),
        status: api_key.status,
        owner_kind: api_key.owner_kind,
        owner_id,
        owner_name,
        owner_email,
        owner_team_key,
        model_keys: granted_models
            .iter()
            .map(|model| model.model_key.clone())
            .collect(),
        created_at: api_key.created_at,
        last_used_at: api_key.last_used_at,
        revoked_at: api_key.revoked_at,
    })
}

fn validate_owner(
    request: &CreateAdminApiKeyInput,
    owner_kind: ApiKeyOwnerKind,
    users: &[IdentityUserRecord],
    teams: &[TeamRecord],
) -> Result<(Option<Uuid>, Option<Uuid>), GatewayError> {
    match owner_kind {
        ApiKeyOwnerKind::User => {
            if request.owner_team_id.is_some() {
                return Err(GatewayError::InvalidRequest(
                    "user-owned api keys cannot include owner_team_id".to_string(),
                ));
            }
            let user_id = request
                .owner_user_id
                .as_deref()
                .ok_or_else(|| {
                    GatewayError::InvalidRequest(
                        "owner_user_id is required for user-owned api keys".to_string(),
                    )
                })
                .and_then(|value| parse_uuid(value, "owner_user_id"))?;
            let user = users
                .iter()
                .find(|user| user.user.user_id == user_id)
                .ok_or_else(|| StoreError::NotFound(format!("user `{user_id}`")))?;
            if user.user.status != UserStatus::Active {
                return Err(GatewayError::InvalidRequest(
                    "user-owned api keys require an active user".to_string(),
                ));
            }
            Ok((Some(user_id), None))
        }
        ApiKeyOwnerKind::Team => {
            if request.owner_user_id.is_some() {
                return Err(GatewayError::InvalidRequest(
                    "team-owned api keys cannot include owner_user_id".to_string(),
                ));
            }
            let team_id = request
                .owner_team_id
                .as_deref()
                .ok_or_else(|| {
                    GatewayError::InvalidRequest(
                        "owner_team_id is required for team-owned api keys".to_string(),
                    )
                })
                .and_then(|value| parse_uuid(value, "owner_team_id"))?;
            let team = teams
                .iter()
                .find(|team| team.team_id == team_id)
                .ok_or_else(|| StoreError::NotFound(format!("team `{team_id}`")))?;
            if team.status != "active" {
                return Err(GatewayError::InvalidRequest(
                    "team-owned api keys require an active team".to_string(),
                ));
            }
            Ok((None, Some(team_id)))
        }
    }
}

fn select_granted_models(
    raw_model_keys: &[String],
    models: &[GatewayModel],
) -> Result<Vec<GatewayModel>, GatewayError> {
    let mut seen = BTreeSet::new();
    let model_map = models
        .iter()
        .map(|model| (model.model_key.as_str(), model))
        .collect::<HashMap<_, _>>();

    let mut selected = Vec::new();
    for raw_model_key in raw_model_keys {
        let model_key = raw_model_key.trim();
        if model_key.is_empty() || !seen.insert(model_key.to_string()) {
            continue;
        }

        let model = model_map.get(model_key).ok_or_else(|| {
            GatewayError::InvalidRequest(format!("unknown model_key `{model_key}`"))
        })?;
        selected.push((*model).clone());
    }

    if selected.is_empty() {
        return Err(GatewayError::InvalidRequest(
            "at least one model_key is required".to_string(),
        ));
    }

    Ok(selected)
}

fn parse_uuid(raw: &str, field_name: &str) -> Result<Uuid, GatewayError> {
    Uuid::parse_str(raw)
        .map_err(|_| GatewayError::InvalidRequest(format!("{field_name} must be a valid uuid")))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use async_trait::async_trait;
    use gateway_core::{
        AdminIdentityRepository, ApiKeyOwnerKind, AuthMode, GlobalRole, IdentityRepository,
        ModelAccessMode, ModelRoute, StoreError, TeamMembershipRecord, UserRecord,
    };

    use super::*;

    #[derive(Clone, Default)]
    struct InMemoryRepo {
        api_keys: Arc<std::sync::Mutex<HashMap<Uuid, ApiKeyRecord>>>,
        models: HashMap<String, GatewayModel>,
        grants: Arc<std::sync::Mutex<HashMap<Uuid, Vec<Uuid>>>>,
        users: Vec<IdentityUserRecord>,
        teams: Vec<TeamRecord>,
    }

    #[async_trait]
    impl AdminApiKeyRepository for InMemoryRepo {
        async fn list_api_keys(&self) -> Result<Vec<ApiKeyRecord>, StoreError> {
            Ok(self
                .api_keys
                .lock()
                .expect("api keys lock")
                .values()
                .cloned()
                .collect())
        }

        async fn get_api_key_by_id(
            &self,
            api_key_id: Uuid,
        ) -> Result<Option<ApiKeyRecord>, StoreError> {
            Ok(self
                .api_keys
                .lock()
                .expect("api keys lock")
                .get(&api_key_id)
                .cloned())
        }

        async fn create_api_key(
            &self,
            api_key: &NewApiKeyRecord,
        ) -> Result<ApiKeyRecord, StoreError> {
            let record = ApiKeyRecord {
                id: Uuid::new_v4(),
                public_id: api_key.public_id.clone(),
                secret_hash: api_key.secret_hash.clone(),
                name: api_key.name.clone(),
                status: ApiKeyStatus::Active,
                owner_kind: api_key.owner_kind,
                owner_user_id: api_key.owner_user_id,
                owner_team_id: api_key.owner_team_id,
                created_at: api_key.created_at,
                last_used_at: None,
                revoked_at: None,
            };
            self.api_keys
                .lock()
                .expect("api keys lock")
                .insert(record.id, record.clone());
            Ok(record)
        }

        async fn replace_api_key_model_grants(
            &self,
            api_key_id: Uuid,
            model_ids: &[Uuid],
        ) -> Result<(), StoreError> {
            self.grants
                .lock()
                .expect("grants lock")
                .insert(api_key_id, model_ids.to_vec());
            Ok(())
        }

        async fn revoke_api_key(
            &self,
            api_key_id: Uuid,
            revoked_at: OffsetDateTime,
        ) -> Result<bool, StoreError> {
            let mut api_keys = self.api_keys.lock().expect("api keys lock");
            let Some(record) = api_keys.get_mut(&api_key_id) else {
                return Ok(false);
            };
            if record.revoked_at.is_some() {
                return Ok(false);
            }
            record.status = ApiKeyStatus::Revoked;
            record.revoked_at = Some(revoked_at);
            Ok(true)
        }
    }

    #[async_trait]
    impl AdminIdentityRepository for InMemoryRepo {
        async fn list_identity_users(&self) -> Result<Vec<IdentityUserRecord>, StoreError> {
            Ok(self.users.clone())
        }

        async fn list_active_teams(&self) -> Result<Vec<TeamRecord>, StoreError> {
            Ok(self
                .teams
                .iter()
                .filter(|team| team.status == "active")
                .cloned()
                .collect())
        }

        async fn list_teams(&self) -> Result<Vec<TeamRecord>, StoreError> {
            Ok(self.teams.clone())
        }
    }

    #[async_trait]
    impl ModelRepository for InMemoryRepo {
        async fn list_models(&self) -> Result<Vec<GatewayModel>, StoreError> {
            Ok(self.models.values().cloned().collect())
        }

        async fn get_model_by_key(
            &self,
            model_key: &str,
        ) -> Result<Option<GatewayModel>, StoreError> {
            Ok(self.models.get(model_key).cloned())
        }

        async fn list_models_for_api_key(
            &self,
            api_key_id: Uuid,
        ) -> Result<Vec<GatewayModel>, StoreError> {
            let grants = self.grants.lock().expect("grants lock");
            let Some(model_ids) = grants.get(&api_key_id) else {
                return Ok(Vec::new());
            };
            Ok(model_ids
                .iter()
                .filter_map(|model_id| {
                    self.models
                        .values()
                        .find(|model| &model.id == model_id)
                        .cloned()
                })
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
    impl IdentityRepository for InMemoryRepo {
        async fn get_user_by_id(&self, _user_id: Uuid) -> Result<Option<UserRecord>, StoreError> {
            Ok(None)
        }

        async fn get_team_by_id(&self, _team_id: Uuid) -> Result<Option<TeamRecord>, StoreError> {
            Ok(None)
        }

        async fn get_team_membership_for_user(
            &self,
            _user_id: Uuid,
        ) -> Result<Option<TeamMembershipRecord>, StoreError> {
            Ok(None)
        }

        async fn list_allowed_model_keys_for_user(
            &self,
            _user_id: Uuid,
        ) -> Result<Vec<String>, StoreError> {
            Ok(Vec::new())
        }

        async fn list_allowed_model_keys_for_team(
            &self,
            _team_id: Uuid,
        ) -> Result<Vec<String>, StoreError> {
            Ok(Vec::new())
        }
    }

    fn model(model_key: &str) -> GatewayModel {
        GatewayModel {
            id: Uuid::new_v4(),
            model_key: model_key.to_string(),
            alias_target_model_key: None,
            description: Some(format!("{model_key} tier")),
            tags: vec![model_key.to_string()],
            rank: 1,
        }
    }

    fn user(user_id: Uuid, status: UserStatus) -> IdentityUserRecord {
        IdentityUserRecord {
            user: UserRecord {
                user_id,
                name: "Member".to_string(),
                email: "member@example.com".to_string(),
                email_normalized: "member@example.com".to_string(),
                global_role: GlobalRole::User,
                auth_mode: AuthMode::Password,
                status,
                must_change_password: false,
                request_logging_enabled: true,
                model_access_mode: ModelAccessMode::All,
                created_at: OffsetDateTime::now_utc(),
                updated_at: OffsetDateTime::now_utc(),
            },
            team_id: None,
            team_name: None,
            membership_role: None,
            oidc_provider_id: None,
            oidc_provider_key: None,
        }
    }

    fn team(team_id: Uuid, status: &str) -> TeamRecord {
        TeamRecord {
            team_id,
            team_key: "core-platform".to_string(),
            team_name: "Core Platform".to_string(),
            status: status.to_string(),
            model_access_mode: ModelAccessMode::All,
            created_at: OffsetDateTime::now_utc(),
            updated_at: OffsetDateTime::now_utc(),
        }
    }

    fn repo_with_defaults() -> InMemoryRepo {
        let fast = model("fast");
        let reasoning = model("reasoning");
        let user_id = Uuid::new_v4();
        let team_id = Uuid::new_v4();

        InMemoryRepo {
            api_keys: Arc::new(std::sync::Mutex::new(HashMap::new())),
            models: HashMap::from([
                (fast.model_key.clone(), fast),
                (reasoning.model_key.clone(), reasoning),
            ]),
            grants: Arc::new(std::sync::Mutex::new(HashMap::new())),
            users: vec![user(user_id, UserStatus::Active)],
            teams: vec![team(team_id, "active")],
        }
    }

    #[tokio::test]
    async fn creates_user_owned_keys() {
        let repo = repo_with_defaults();
        let user_id = repo.users[0].user.user_id;
        let service = AdminApiKeyService::new(Arc::new(repo.clone()));

        let result = service
            .create_api_key(CreateAdminApiKeyInput {
                name: "Production Web".to_string(),
                owner_kind: "user".to_string(),
                owner_user_id: Some(user_id.to_string()),
                owner_team_id: None,
                model_keys: vec!["fast".to_string()],
            })
            .await
            .expect("create api key");

        assert!(result.raw_key.starts_with("gwk_"));
        assert_eq!(result.api_key.owner_kind, ApiKeyOwnerKind::User);
        assert_eq!(result.api_key.owner_id, user_id);
        assert_eq!(result.api_key.model_keys, vec!["fast".to_string()]);
    }

    #[tokio::test]
    async fn creates_team_owned_keys() {
        let repo = repo_with_defaults();
        let team_id = repo.teams[0].team_id;
        let service = AdminApiKeyService::new(Arc::new(repo.clone()));

        let result = service
            .create_api_key(CreateAdminApiKeyInput {
                name: "Batch Jobs".to_string(),
                owner_kind: "team".to_string(),
                owner_user_id: None,
                owner_team_id: Some(team_id.to_string()),
                model_keys: vec!["reasoning".to_string()],
            })
            .await
            .expect("create api key");

        assert_eq!(result.api_key.owner_kind, ApiKeyOwnerKind::Team);
        assert_eq!(result.api_key.owner_id, team_id);
        assert_eq!(result.api_key.model_keys, vec!["reasoning".to_string()]);
    }

    #[tokio::test]
    async fn rejects_empty_name() {
        let repo = repo_with_defaults();
        let user_id = repo.users[0].user.user_id;
        let service = AdminApiKeyService::new(Arc::new(repo.clone()));

        let error = service
            .create_api_key(CreateAdminApiKeyInput {
                name: "   ".to_string(),
                owner_kind: "user".to_string(),
                owner_user_id: Some(user_id.to_string()),
                owner_team_id: None,
                model_keys: vec!["fast".to_string()],
            })
            .await
            .expect_err("empty name should fail");

        assert_eq!(error.error_code(), "invalid_request");
        assert!(error.to_string().contains("name is required"));
    }

    #[tokio::test]
    async fn rejects_invalid_owner_kind() {
        let repo = repo_with_defaults();
        let service = AdminApiKeyService::new(Arc::new(repo.clone()));

        let error = service
            .create_api_key(CreateAdminApiKeyInput {
                name: "Bad Owner".to_string(),
                owner_kind: "system".to_string(),
                owner_user_id: None,
                owner_team_id: None,
                model_keys: vec!["fast".to_string()],
            })
            .await
            .expect_err("invalid owner kind should fail");

        assert_eq!(error.error_code(), "invalid_request");
        assert!(error.to_string().contains("owner_kind"));
    }

    #[tokio::test]
    async fn rejects_inactive_or_missing_owner() {
        let mut repo = repo_with_defaults();
        let user_id = repo.users[0].user.user_id;
        repo.users = vec![user(user_id, UserStatus::Disabled)];
        let service = AdminApiKeyService::new(Arc::new(repo.clone()));

        let inactive_error = service
            .create_api_key(CreateAdminApiKeyInput {
                name: "Inactive User".to_string(),
                owner_kind: "user".to_string(),
                owner_user_id: Some(user_id.to_string()),
                owner_team_id: None,
                model_keys: vec!["fast".to_string()],
            })
            .await
            .expect_err("inactive owner should fail");
        assert_eq!(inactive_error.error_code(), "invalid_request");

        let missing_error = service
            .create_api_key(CreateAdminApiKeyInput {
                name: "Missing User".to_string(),
                owner_kind: "user".to_string(),
                owner_user_id: Some(Uuid::new_v4().to_string()),
                owner_team_id: None,
                model_keys: vec!["fast".to_string()],
            })
            .await
            .expect_err("missing owner should fail");
        assert_eq!(missing_error.error_code(), "not_found");
    }

    #[tokio::test]
    async fn rejects_unknown_or_empty_model_grants() {
        let repo = repo_with_defaults();
        let user_id = repo.users[0].user.user_id;
        let service = AdminApiKeyService::new(Arc::new(repo.clone()));

        let unknown_error = service
            .create_api_key(CreateAdminApiKeyInput {
                name: "Unknown Model".to_string(),
                owner_kind: "user".to_string(),
                owner_user_id: Some(user_id.to_string()),
                owner_team_id: None,
                model_keys: vec!["missing".to_string()],
            })
            .await
            .expect_err("unknown model should fail");
        assert_eq!(unknown_error.error_code(), "invalid_request");

        let empty_error = service
            .create_api_key(CreateAdminApiKeyInput {
                name: "Empty Grants".to_string(),
                owner_kind: "user".to_string(),
                owner_user_id: Some(user_id.to_string()),
                owner_team_id: None,
                model_keys: vec!["   ".to_string()],
            })
            .await
            .expect_err("empty grants should fail");
        assert_eq!(empty_error.error_code(), "invalid_request");
    }

    #[tokio::test]
    async fn revoke_reload_reflects_revoked_status() {
        let repo = repo_with_defaults();
        let user_id = repo.users[0].user.user_id;
        let service = AdminApiKeyService::new(Arc::new(repo.clone()));
        let created = service
            .create_api_key(CreateAdminApiKeyInput {
                name: "Revoke Me".to_string(),
                owner_kind: "user".to_string(),
                owner_user_id: Some(user_id.to_string()),
                owner_team_id: None,
                model_keys: vec!["fast".to_string()],
            })
            .await
            .expect("create api key");

        let revoked = service
            .revoke_api_key(created.api_key.id)
            .await
            .expect("revoke api key");

        assert_eq!(revoked.status, ApiKeyStatus::Revoked);
        assert!(revoked.revoked_at.is_some());
    }

    #[tokio::test]
    async fn updates_model_grants_for_active_keys() {
        let repo = repo_with_defaults();
        let user_id = repo.users[0].user.user_id;
        let service = AdminApiKeyService::new(Arc::new(repo.clone()));
        let created = service
            .create_api_key(CreateAdminApiKeyInput {
                name: "Manage Me".to_string(),
                owner_kind: "user".to_string(),
                owner_user_id: Some(user_id.to_string()),
                owner_team_id: None,
                model_keys: vec!["fast".to_string()],
            })
            .await
            .expect("create api key");

        let updated = service
            .update_api_key(
                created.api_key.id,
                UpdateAdminApiKeyInput {
                    model_keys: vec!["reasoning".to_string(), "fast".to_string()],
                },
            )
            .await
            .expect("update api key");

        assert_eq!(
            updated.model_keys,
            vec!["reasoning".to_string(), "fast".to_string()]
        );
    }

    #[tokio::test]
    async fn rejects_updates_for_revoked_keys() {
        let repo = repo_with_defaults();
        let user_id = repo.users[0].user.user_id;
        let service = AdminApiKeyService::new(Arc::new(repo.clone()));
        let created = service
            .create_api_key(CreateAdminApiKeyInput {
                name: "Revoked Key".to_string(),
                owner_kind: "user".to_string(),
                owner_user_id: Some(user_id.to_string()),
                owner_team_id: None,
                model_keys: vec!["fast".to_string()],
            })
            .await
            .expect("create api key");

        service
            .revoke_api_key(created.api_key.id)
            .await
            .expect("revoke api key");

        let error = service
            .update_api_key(
                created.api_key.id,
                UpdateAdminApiKeyInput {
                    model_keys: vec!["reasoning".to_string()],
                },
            )
            .await
            .expect_err("revoked api key update should fail");

        assert_eq!(error.error_code(), "invalid_request");
        assert!(error.to_string().contains("cannot be updated"));
    }

    #[tokio::test]
    async fn lists_and_revokes_keys_for_inactive_team_owners() {
        let mut repo = repo_with_defaults();
        let team_id = repo.teams[0].team_id;
        repo.teams = vec![team(team_id, "inactive")];
        let repo = Arc::new(repo);
        let service = AdminApiKeyService::new(repo.clone());
        let now = OffsetDateTime::now_utc();
        let api_key = repo
            .create_api_key(&NewApiKeyRecord {
                name: "Dormant Team".to_string(),
                public_id: Uuid::new_v4().simple().to_string(),
                secret_hash: "secret-hash".to_string(),
                owner_kind: ApiKeyOwnerKind::Team,
                owner_user_id: None,
                owner_team_id: Some(team_id),
                created_at: now,
            })
            .await
            .expect("seed api key");

        repo.replace_api_key_model_grants(api_key.id, &[repo.models["fast"].id])
            .await
            .expect("seed grants");

        let listed = service.list_api_keys().await.expect("list api keys");
        assert_eq!(listed.items.len(), 1);
        assert_eq!(listed.items[0].owner_id, team_id);
        assert!(listed.teams.is_empty());

        let revoked = service
            .revoke_api_key(api_key.id)
            .await
            .expect("revoke api key");
        assert_eq!(revoked.owner_id, team_id);
        assert_eq!(revoked.status, ApiKeyStatus::Revoked);
    }

    #[tokio::test]
    async fn list_payload_preserves_owner_and_grant_metadata() {
        let repo = repo_with_defaults();
        let user_id = repo.users[0].user.user_id;
        let service = AdminApiKeyService::new(Arc::new(repo.clone()));
        service
            .create_api_key(CreateAdminApiKeyInput {
                name: "Listed Key".to_string(),
                owner_kind: "user".to_string(),
                owner_user_id: Some(user_id.to_string()),
                owner_team_id: None,
                model_keys: vec!["fast".to_string(), "reasoning".to_string()],
            })
            .await
            .expect("create api key");

        let payload = service.list_api_keys().await.expect("list api keys");

        assert_eq!(payload.items.len(), 1);
        assert_eq!(payload.items[0].owner_name, "Member");
        assert_eq!(
            payload.items[0].model_keys,
            vec!["fast".to_string(), "reasoning".to_string()]
        );
        assert_eq!(payload.users.len(), 1);
        assert_eq!(payload.teams.len(), 1);
        assert_eq!(payload.models.len(), 2);
    }
}
