use std::sync::Arc;

use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng},
};
use gateway_core::{
    ApiKeyOwnerKind, ApiKeyRepository, ApiKeyStatus, AuthError, AuthenticatedApiKey,
    BudgetRepository, BudgetScope, GatewayError, ServiceAccountStatus, extract_bearer_token,
    parse_gateway_api_key,
};

#[derive(Clone)]
pub struct Authenticator<R> {
    repo: Arc<R>,
    argon2: Argon2<'static>,
}

impl<R> Authenticator<R>
where
    R: ApiKeyRepository + BudgetRepository,
{
    #[must_use]
    pub fn new(repo: Arc<R>) -> Self {
        Self {
            repo,
            argon2: Argon2::default(),
        }
    }

    pub async fn authenticate_authorization_header(
        &self,
        authorization_header: Option<&str>,
    ) -> Result<AuthenticatedApiKey, GatewayError> {
        let header = authorization_header.ok_or(AuthError::MissingAuthorizationHeader)?;
        let token = extract_bearer_token(header)?;
        self.authenticate_bearer_token(token).await
    }

    pub async fn authenticate_bearer_token(
        &self,
        bearer_token: &str,
    ) -> Result<AuthenticatedApiKey, GatewayError> {
        let parsed = parse_gateway_api_key(bearer_token)?;
        let record = self
            .repo
            .get_api_key_by_public_id(&parsed.public_id)
            .await?
            .ok_or(AuthError::ApiKeyNotFound)?;

        if record.status != ApiKeyStatus::Active || record.revoked_at.is_some() {
            return Err(AuthError::ApiKeyRevoked.into());
        }

        let owner_is_valid = match record.owner_kind {
            ApiKeyOwnerKind::User => {
                record.owner_user_id.is_some()
                    && record.owner_team_id.is_none()
                    && record.owner_service_account_id.is_none()
            }
            ApiKeyOwnerKind::ServiceAccount => {
                record.owner_user_id.is_none()
                    && record.owner_team_id.is_some()
                    && record.owner_service_account_id.is_some()
            }
        };
        if !owner_is_valid {
            return Err(AuthError::ApiKeyOwnerInvalid.into());
        }

        if let ApiKeyOwnerKind::ServiceAccount = record.owner_kind {
            let service_account_id = record
                .owner_service_account_id
                .ok_or(AuthError::ApiKeyOwnerInvalid)?;
            let service_account = self
                .repo
                .get_service_account_by_id(service_account_id)
                .await?
                .ok_or(AuthError::ApiKeyOwnerInvalid)?;
            if service_account.status != ServiceAccountStatus::Active
                || Some(service_account.team_id) != record.owner_team_id
            {
                return Err(AuthError::ApiKeyOwnerInvalid.into());
            }
            let scope = BudgetScope::ServiceAccount { service_account_id };
            if self
                .repo
                .get_active_budget_by_scope(&scope)
                .await?
                .is_none()
            {
                return Err(AuthError::ServiceAccountBudgetRequired.into());
            }
        }

        let password_hash = PasswordHash::new(&record.secret_hash)
            .map_err(|error| AuthError::HashVerification(error.to_string()))?;

        self.argon2
            .verify_password(parsed.secret.as_bytes(), &password_hash)
            .map_err(|_| AuthError::ApiKeySecretMismatch)?;

        self.repo.touch_api_key_last_used(record.id).await?;

        Ok(AuthenticatedApiKey {
            id: record.id,
            public_id: record.public_id,
            name: record.name,
            model_grant_mode: record.model_grant_mode,
            owner_kind: record.owner_kind,
            owner_user_id: record.owner_user_id,
            owner_team_id: record.owner_team_id,
            owner_service_account_id: record.owner_service_account_id,
        })
    }
}

pub fn hash_gateway_key_secret(secret: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(secret.as_bytes(), &salt)
        .map_err(|error| anyhow::anyhow!("failed to hash gateway key secret: {error}"))?
        .to_string();

    Ok(hash)
}

pub fn verify_gateway_key_secret(secret: &str, expected_hash: &str) -> anyhow::Result<bool> {
    let password_hash = PasswordHash::new(expected_hash)
        .map_err(|error| anyhow::anyhow!("failed to parse password hash: {error}"))?;

    Ok(Argon2::default()
        .verify_password(secret.as_bytes(), &password_hash)
        .is_ok())
}
