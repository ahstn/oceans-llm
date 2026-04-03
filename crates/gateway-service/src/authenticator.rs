use std::sync::Arc;

use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng},
};
use gateway_core::{
    ApiKeyOwnerKind, ApiKeyRepository, ApiKeyStatus, AuthError, AuthenticatedApiKey, GatewayError,
    extract_bearer_token, parse_gateway_api_key,
};

#[derive(Clone)]
pub struct Authenticator<R> {
    repo: Arc<R>,
    argon2: Argon2<'static>,
}

impl<R> Authenticator<R>
where
    R: ApiKeyRepository,
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
                record.owner_user_id.is_some() && record.owner_team_id.is_none()
            }
            ApiKeyOwnerKind::Team => {
                record.owner_team_id.is_some() && record.owner_user_id.is_none()
            }
        };
        if !owner_is_valid {
            return Err(AuthError::ApiKeyOwnerInvalid.into());
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
            owner_kind: record.owner_kind,
            owner_user_id: record.owner_user_id,
            owner_team_id: record.owner_team_id,
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use async_trait::async_trait;
    use gateway_core::{
        ApiKeyOwnerKind, ApiKeyRecord, ApiKeyRepository, ApiKeyStatus, AuthError, GatewayError,
        StoreError, parse_gateway_api_key,
    };
    use time::OffsetDateTime;
    use uuid::Uuid;

    use super::{Authenticator, hash_gateway_key_secret};

    #[derive(Clone)]
    struct InMemoryKeyRepo {
        key: Option<ApiKeyRecord>,
    }

    #[async_trait]
    impl ApiKeyRepository for InMemoryKeyRepo {
        async fn get_api_key_by_public_id(
            &self,
            public_id: &str,
        ) -> Result<Option<ApiKeyRecord>, StoreError> {
            Ok(self
                .key
                .clone()
                .filter(|record| record.public_id == public_id))
        }

        async fn touch_api_key_last_used(&self, _api_key_id: Uuid) -> Result<(), StoreError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn authenticates_valid_bearer_token() {
        let raw = "gwk_dev123.super-secret";
        let parsed = parse_gateway_api_key(raw).expect("parse token");
        let hash = hash_gateway_key_secret(&parsed.secret).expect("hash secret");
        let repo = Arc::new(InMemoryKeyRepo {
            key: Some(ApiKeyRecord {
                id: Uuid::new_v4(),
                public_id: parsed.public_id,
                secret_hash: hash,
                name: "dev".to_string(),
                status: ApiKeyStatus::Active,
                owner_kind: ApiKeyOwnerKind::Team,
                owner_user_id: None,
                owner_team_id: Some(Uuid::new_v4()),
                created_at: OffsetDateTime::now_utc(),
                last_used_at: None,
                revoked_at: None,
            }),
        });

        let authenticator = Authenticator::new(repo);
        let authenticated = authenticator
            .authenticate_authorization_header(Some("Bearer gwk_dev123.super-secret"))
            .await
            .expect("must authenticate");

        assert_eq!(authenticated.public_id, "dev123");
        assert!(authenticated.owner_team_id.is_some());
    }

    #[tokio::test]
    async fn rejects_wrong_secret() {
        let hash = hash_gateway_key_secret("correct-secret").expect("hash secret");
        let repo = Arc::new(InMemoryKeyRepo {
            key: Some(ApiKeyRecord {
                id: Uuid::new_v4(),
                public_id: "dev123".to_string(),
                secret_hash: hash,
                name: "dev".to_string(),
                status: ApiKeyStatus::Active,
                owner_kind: ApiKeyOwnerKind::Team,
                owner_user_id: None,
                owner_team_id: Some(Uuid::new_v4()),
                created_at: OffsetDateTime::now_utc(),
                last_used_at: None,
                revoked_at: None,
            }),
        });

        let authenticator = Authenticator::new(repo);
        let error = authenticator
            .authenticate_authorization_header(Some("Bearer gwk_dev123.wrong-secret"))
            .await
            .expect_err("must reject wrong secret");

        assert!(matches!(
            error,
            GatewayError::Auth(AuthError::ApiKeySecretMismatch)
        ));
    }

    #[tokio::test]
    async fn rejects_invalid_owner_metadata() {
        let hash = hash_gateway_key_secret("super-secret").expect("hash secret");
        let repo = Arc::new(InMemoryKeyRepo {
            key: Some(ApiKeyRecord {
                id: Uuid::new_v4(),
                public_id: "dev123".to_string(),
                secret_hash: hash,
                name: "dev".to_string(),
                status: ApiKeyStatus::Active,
                owner_kind: ApiKeyOwnerKind::Team,
                owner_user_id: Some(Uuid::new_v4()),
                owner_team_id: Some(Uuid::new_v4()),
                created_at: OffsetDateTime::now_utc(),
                last_used_at: None,
                revoked_at: None,
            }),
        });

        let authenticator = Authenticator::new(repo);
        let error = authenticator
            .authenticate_authorization_header(Some("Bearer gwk_dev123.super-secret"))
            .await
            .expect_err("must reject invalid owner metadata");

        assert!(matches!(
            error,
            GatewayError::Auth(AuthError::ApiKeyOwnerInvalid)
        ));
    }
}
