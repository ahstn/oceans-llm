use std::{
    collections::{BTreeSet, HashMap},
    sync::Arc,
};

use gateway_core::{
    AdminApiKeyRepository, AdminIdentityRepository, ApiKeyOwnerKind, ApiKeyRecord,
    ApiKeySecretMaterialRecord, ApiKeySecretStorageKind, ApiKeyStatus, BudgetRepository,
    BudgetScope, GatewayError, GatewayModel, IdentityUserRecord, ModelRepository, NewApiKeyRecord,
    ServiceAccountRecord, StoreError, TeamRecord, UserStatus,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    decrypt_gateway_api_key_secret, encrypt_gateway_api_key_secret, hash_gateway_key_secret,
};

type ApiKeyOwnerIds = (Option<Uuid>, Option<Uuid>, Option<Uuid>);

#[derive(Debug, Clone)]
pub struct AdminApiKeyService<R> {
    repo: Arc<R>,
}

#[derive(Debug, Clone)]
pub struct AdminApiKeysPayload {
    pub items: Vec<AdminApiKeySummary>,
    pub users: Vec<AdminApiKeyUserOwner>,
    pub service_accounts: Vec<AdminApiKeyServiceAccountOwner>,
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
    pub owner_service_account_key: Option<String>,
    pub owner_service_account_team_id: Option<Uuid>,
    pub owner_service_account_team_key: Option<String>,
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
pub struct AdminApiKeyServiceAccountOwner {
    pub id: Uuid,
    pub name: String,
    pub key: String,
    pub team_id: Uuid,
    pub team_key: String,
    pub team_name: String,
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
    pub owner_service_account_id: Option<String>,
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

pub struct RevealAdminApiKeySecretResult {
    pub raw_key: String,
}

impl<R> AdminApiKeyService<R>
where
    R: AdminApiKeyRepository
        + AdminIdentityRepository
        + ModelRepository
        + BudgetRepository
        + Send
        + Sync
        + 'static,
{
    #[must_use]
    pub fn new(repo: Arc<R>) -> Self {
        Self { repo }
    }

    pub async fn list_api_keys(&self) -> Result<AdminApiKeysPayload, GatewayError> {
        let api_keys = self.repo.list_api_keys().await?;
        let users = self.repo.list_identity_users().await?;
        let teams = self.repo.list_teams().await?;
        let service_accounts = self.repo.list_service_accounts().await?;
        let active_service_accounts = self.repo.list_active_service_accounts().await?;
        let models = self.repo.list_models().await?;
        let service_account_owners =
            build_service_account_owner_options(&active_service_accounts, &teams)?;

        let mut items = Vec::with_capacity(api_keys.len());
        for api_key in api_keys {
            let granted_models = self.repo.list_models_for_api_key(api_key.id).await?;
            items.push(build_api_key_summary(
                &api_key,
                &users,
                &teams,
                &service_accounts,
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
            service_accounts: service_account_owners,
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
        let service_accounts = self.repo.list_active_service_accounts().await?;
        let models = self.repo.list_models().await?;

        let name = request.name.trim();
        if name.is_empty() {
            return Err(GatewayError::InvalidRequest(
                "api key name is required".to_string(),
            ));
        }

        let owner_kind = ApiKeyOwnerKind::from_db(request.owner_kind.trim()).ok_or_else(|| {
            GatewayError::InvalidRequest(
                "owner_kind must be `user` or `service_account`".to_string(),
            )
        })?;
        let (owner_user_id, owner_team_id, owner_service_account_id) = validate_owner(
            &request,
            owner_kind,
            &users,
            &active_teams,
            &service_accounts,
        )?;
        if let Some(service_account_id) = owner_service_account_id {
            let scope = BudgetScope::ServiceAccount { service_account_id };
            if self
                .repo
                .get_active_budget_by_scope(&scope)
                .await?
                .is_none()
            {
                return Err(GatewayError::InvalidRequest(
                    "service-account-owned api keys require an active service account budget"
                        .to_string(),
                ));
            }
        }
        let granted_models = select_granted_models(&request.model_keys, &models)?;

        let public_id = Uuid::new_v4().simple().to_string();
        let secret = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        let raw_key = format!("gwk_{public_id}.{secret}");
        let secret_hash = hash_gateway_key_secret(&secret)
            .map_err(|error| GatewayError::Internal(error.to_string()))?;
        let now = OffsetDateTime::now_utc();
        let secret_material = if owner_kind == ApiKeyOwnerKind::ServiceAccount {
            Some(encrypt_gateway_api_key_secret(&raw_key)?)
        } else {
            None
        };

        let api_key = self
            .repo
            .create_api_key(&NewApiKeyRecord {
                name: name.to_string(),
                public_id,
                secret_hash,
                owner_kind,
                owner_user_id,
                owner_team_id,
                owner_service_account_id,
                created_at: now,
            })
            .await?;
        if let Some(encrypted) = secret_material {
            self.repo
                .upsert_api_key_secret_material(&ApiKeySecretMaterialRecord {
                    api_key_id: api_key.id,
                    storage_kind: ApiKeySecretStorageKind::EncryptedBlob,
                    secret_ciphertext: encrypted.ciphertext,
                    secret_nonce: encrypted.nonce,
                    secret_key_id: encrypted.key_id.to_string(),
                    created_at: now,
                    updated_at: now,
                    last_retrieved_at: None,
                })
                .await?;
        }
        let model_ids = granted_models
            .iter()
            .map(|model| model.id)
            .collect::<Vec<_>>();
        self.repo
            .replace_api_key_model_grants(api_key.id, &model_ids)
            .await?;

        let granted_models = self.repo.list_models_for_api_key(api_key.id).await?;
        let api_key =
            build_api_key_summary(&api_key, &users, &teams, &service_accounts, &granted_models)?;

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
        let service_accounts = self.repo.list_service_accounts().await?;
        let api_key = self
            .repo
            .get_api_key_by_id(api_key_id)
            .await?
            .ok_or_else(|| {
                GatewayError::Internal(format!("api key `{api_key_id}` missing after revoke"))
            })?;
        let granted_models = self.repo.list_models_for_api_key(api_key.id).await?;

        build_api_key_summary(&api_key, &users, &teams, &service_accounts, &granted_models)
    }

    pub async fn reveal_api_key_secret(
        &self,
        api_key_id: Uuid,
    ) -> Result<RevealAdminApiKeySecretResult, GatewayError> {
        let api_key = self
            .repo
            .get_api_key_by_id(api_key_id)
            .await?
            .ok_or_else(|| StoreError::NotFound(format!("api key `{api_key_id}`")))?;
        if api_key.status != ApiKeyStatus::Active {
            return Err(GatewayError::InvalidRequest(
                "revoked api keys cannot be revealed".to_string(),
            ));
        }

        let material = self
            .repo
            .get_api_key_secret_material(api_key_id)
            .await?
            .ok_or_else(|| {
                GatewayError::InvalidRequest(
                    "api key secret is not retrievable for this key".to_string(),
                )
            })?;
        let raw_key = decrypt_gateway_api_key_secret(
            &material.secret_ciphertext,
            &material.secret_nonce,
            &material.secret_key_id,
        )?;
        self.repo
            .touch_api_key_secret_material_retrieved(api_key_id, OffsetDateTime::now_utc())
            .await?;

        Ok(RevealAdminApiKeySecretResult { raw_key })
    }

    pub async fn update_api_key(
        &self,
        api_key_id: Uuid,
        request: UpdateAdminApiKeyInput,
    ) -> Result<AdminApiKeySummary, GatewayError> {
        let users = self.repo.list_identity_users().await?;
        let teams = self.repo.list_teams().await?;
        let service_accounts = self.repo.list_service_accounts().await?;
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
        build_api_key_summary(&api_key, &users, &teams, &service_accounts, &granted_models)
    }
}

fn build_api_key_summary(
    api_key: &ApiKeyRecord,
    users: &[IdentityUserRecord],
    teams: &[TeamRecord],
    service_accounts: &[ServiceAccountRecord],
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
    let service_account_map = service_accounts
        .iter()
        .map(|service_account| (service_account.service_account_id, service_account))
        .collect::<HashMap<_, _>>();

    let (
        owner_id,
        owner_name,
        owner_email,
        owner_team_key,
        owner_service_account_key,
        owner_service_account_team_id,
        owner_service_account_team_key,
    ) = match api_key.owner_kind {
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
                None,
                None,
                None,
            )
        }
        ApiKeyOwnerKind::ServiceAccount => {
            let owner_id = api_key.owner_service_account_id.ok_or_else(|| {
                GatewayError::Internal(format!(
                    "api key `{}` is missing owner_service_account_id",
                    api_key.id
                ))
            })?;
            let owner = service_account_map.get(&owner_id).ok_or_else(|| {
                GatewayError::Internal(format!(
                    "api key `{}` references missing service account `{owner_id}`",
                    api_key.id
                ))
            })?;
            let team = team_map.get(&owner.team_id).ok_or_else(|| {
                GatewayError::Internal(format!(
                    "service account `{owner_id}` references missing team `{}`",
                    owner.team_id
                ))
            })?;
            (
                owner_id,
                owner.service_account_name.clone(),
                None,
                Some(team.team_key.clone()),
                Some(owner.service_account_key.clone()),
                Some(owner.team_id),
                Some(team.team_key.clone()),
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
        owner_service_account_key,
        owner_service_account_team_id,
        owner_service_account_team_key,
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
    service_accounts: &[ServiceAccountRecord],
) -> Result<ApiKeyOwnerIds, GatewayError> {
    match owner_kind {
        ApiKeyOwnerKind::User => {
            if request.owner_team_id.is_some() || request.owner_service_account_id.is_some() {
                return Err(GatewayError::InvalidRequest(
                    "user-owned api keys cannot include service account or team owner fields"
                        .to_string(),
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
            Ok((Some(user_id), None, None))
        }
        ApiKeyOwnerKind::ServiceAccount => {
            if request.owner_user_id.is_some() {
                return Err(GatewayError::InvalidRequest(
                    "service-account-owned api keys cannot include owner_user_id".to_string(),
                ));
            }
            let service_account_id = request
                .owner_service_account_id
                .as_deref()
                .ok_or_else(|| {
                    GatewayError::InvalidRequest(
                        "owner_service_account_id is required for service-account-owned api keys"
                            .to_string(),
                    )
                })
                .and_then(|value| parse_uuid(value, "owner_service_account_id"))?;
            let service_account = service_accounts
                .iter()
                .find(|service_account| service_account.service_account_id == service_account_id)
                .ok_or_else(|| {
                    StoreError::NotFound(format!("service account `{service_account_id}`"))
                })?;
            let team_id = service_account.team_id;
            if let Some(request_team_id) = request
                .owner_team_id
                .as_deref()
                .map(|value| parse_uuid(value, "owner_team_id"))
                .transpose()?
                && request_team_id != team_id
            {
                return Err(GatewayError::InvalidRequest(
                    "owner_team_id must match the service account team".to_string(),
                ));
            }
            let team = teams
                .iter()
                .find(|team| team.team_id == team_id)
                .ok_or_else(|| StoreError::NotFound(format!("team `{team_id}`")))?;
            if team.status != "active" {
                return Err(GatewayError::InvalidRequest(
                    "service-account-owned api keys require an active team".to_string(),
                ));
            }
            Ok((None, Some(team_id), Some(service_account_id)))
        }
    }
}

fn build_service_account_owner_options(
    service_accounts: &[ServiceAccountRecord],
    teams: &[TeamRecord],
) -> Result<Vec<AdminApiKeyServiceAccountOwner>, GatewayError> {
    let team_map = teams
        .iter()
        .map(|team| (team.team_id, team))
        .collect::<HashMap<_, _>>();

    service_accounts
        .iter()
        .map(|service_account| {
            let team = team_map.get(&service_account.team_id).ok_or_else(|| {
                GatewayError::Internal(format!(
                    "service account `{}` references missing team `{}`",
                    service_account.service_account_id, service_account.team_id
                ))
            })?;
            Ok(AdminApiKeyServiceAccountOwner {
                id: service_account.service_account_id,
                name: service_account.service_account_name.clone(),
                key: service_account.service_account_key.clone(),
                team_id: team.team_id,
                team_key: team.team_key.clone(),
                team_name: team.team_name.clone(),
            })
        })
        .collect()
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
