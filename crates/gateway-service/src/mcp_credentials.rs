use std::collections::BTreeMap;

use aes_gcm::{
    Aes256Gcm, KeyInit,
    aead::{Aead, OsRng, rand_core::RngCore},
};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use gateway_core::{
    ApiKeyOwnerKind, AuthenticatedApiKey, ExternalMcpServerRecord, GatewayError,
    IdentityRepository, McpUpstreamCredentialBindingRecord, McpUpstreamCredentialMaterialKind,
    McpUpstreamCredentialOwnerScopeKind, McpUpstreamCredentialRepository,
    McpUpstreamSecretStorageKind, StoreError, UpsertMcpUpstreamCredentialBindingRecord,
};
use serde::{Deserialize, Serialize};
use serde_json::Map;
use time::OffsetDateTime;
use uuid::Uuid;

const CREDENTIAL_KEY_ENV: &str = "OCEANS_MCP_CREDENTIAL_ENCRYPTION_KEY";
const CREDENTIAL_KEY_ID: &str = "env/OCEANS_MCP_CREDENTIAL_ENCRYPTION_KEY";
const CREDENTIAL_SECRET_ENV_PREFIX: &str = "OCEANS_MCP_CREDENTIAL_";

#[derive(Debug, Clone)]
pub struct ResolvedMcpCredential {
    pub headers: BTreeMap<String, String>,
    pub credential_binding_id: Uuid,
    pub owner_scope_key: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RedactedMcpCredentialBinding {
    pub credential_binding_id: Uuid,
    pub mcp_server_id: Uuid,
    pub owner_scope_kind: McpUpstreamCredentialOwnerScopeKind,
    pub owner_scope_key: String,
    pub owner_user_id: Option<Uuid>,
    pub owner_team_id: Option<Uuid>,
    pub owner_service_account_id: Option<Uuid>,
    pub material_kind: McpUpstreamCredentialMaterialKind,
    pub header_name: Option<String>,
    pub storage_kind: McpUpstreamSecretStorageKind,
    pub secret_ref: Option<String>,
    pub expires_at: Option<OffsetDateTime>,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
    pub last_used_at: Option<OffsetDateTime>,
    pub revoked_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpsertMcpCredentialBindingInput {
    pub credential_binding_id: Option<Uuid>,
    pub mcp_server_id: Uuid,
    pub owner_scope_kind: McpUpstreamCredentialOwnerScopeKind,
    pub owner_user_id: Option<Uuid>,
    pub owner_team_id: Option<Uuid>,
    pub owner_service_account_id: Option<Uuid>,
    pub material_kind: McpUpstreamCredentialMaterialKind,
    pub header_name: Option<String>,
    pub secret: Option<String>,
    pub secret_ref: Option<String>,
    pub expires_at: Option<OffsetDateTime>,
    #[serde(default)]
    pub metadata: Map<String, serde_json::Value>,
}

#[derive(Clone)]
pub struct McpCredentialService<R> {
    repo: std::sync::Arc<R>,
}

impl<R> McpCredentialService<R>
where
    R: McpUpstreamCredentialRepository,
{
    #[must_use]
    pub fn new(repo: std::sync::Arc<R>) -> Self {
        Self { repo }
    }

    pub fn validate_runtime_configuration() -> Result<(), GatewayError> {
        if std::env::var(CREDENTIAL_KEY_ENV).is_ok() {
            credential_cipher_key()?;
        }
        Ok(())
    }

    pub async fn resolve_for_auth(
        &self,
        auth: &AuthenticatedApiKey,
        server: &ExternalMcpServerRecord,
    ) -> Result<ResolvedMcpCredential, GatewayError>
    where
        R: IdentityRepository,
    {
        let mut expired_candidate_seen = false;
        for owner_scope_key in credential_lookup_order(self.repo.as_ref(), auth).await? {
            let Some(binding) = self
                .repo
                .get_active_mcp_upstream_credential_binding(server.mcp_server_id, &owner_scope_key)
                .await?
            else {
                continue;
            };
            if binding
                .expires_at
                .is_some_and(|expires_at| expires_at <= OffsetDateTime::now_utc())
            {
                expired_candidate_seen = true;
                continue;
            }
            let headers = credential_headers(&binding, &server.server_key)?;
            let touched = self
                .repo
                .touch_mcp_upstream_credential_binding_last_used(
                    binding.credential_binding_id,
                    OffsetDateTime::now_utc(),
                )
                .await?;
            if !touched {
                return Err(GatewayError::McpCredentialRequired {
                    server_key: server.server_key.clone(),
                });
            }
            return Ok(ResolvedMcpCredential {
                headers,
                credential_binding_id: binding.credential_binding_id,
                owner_scope_key,
            });
        }
        if expired_candidate_seen {
            return Err(GatewayError::McpCredentialExpired {
                server_key: server.server_key.clone(),
            });
        }
        Err(GatewayError::McpCredentialRequired {
            server_key: server.server_key.clone(),
        })
    }

    pub async fn upsert_binding(
        &self,
        input: UpsertMcpCredentialBindingInput,
    ) -> Result<RedactedMcpCredentialBinding, GatewayError> {
        validate_credential_input(&input)?;
        let now = OffsetDateTime::now_utc();
        let owner_scope_key = credential_owner_scope_key(
            input.owner_scope_kind,
            input.owner_user_id,
            input.owner_team_id,
            input.owner_service_account_id,
        )?;
        let (storage_kind, secret_ciphertext, secret_nonce, secret_key_id, secret_ref) =
            match (input.secret.as_deref(), input.secret_ref.as_deref()) {
                (Some(secret), None) => {
                    let encrypted = encrypt_secret(secret)?;
                    (
                        McpUpstreamSecretStorageKind::EncryptedBlob,
                        Some(encrypted.ciphertext),
                        Some(encrypted.nonce),
                        Some(CREDENTIAL_KEY_ID.to_string()),
                        None,
                    )
                }
                (None, Some(secret_ref)) => {
                    validate_credential_secret_ref(secret_ref)?;
                    (
                        McpUpstreamSecretStorageKind::SecretRef,
                        None,
                        None,
                        None,
                        Some(secret_ref.to_string()),
                    )
                }
                _ => {
                    return Err(GatewayError::InvalidRequest(
                        "exactly one of secret or secret_ref is required".to_string(),
                    ));
                }
            };
        let record = UpsertMcpUpstreamCredentialBindingRecord {
            credential_binding_id: input.credential_binding_id,
            mcp_server_id: input.mcp_server_id,
            owner_scope_kind: input.owner_scope_kind,
            owner_scope_key,
            owner_user_id: input.owner_user_id,
            owner_team_id: input.owner_team_id,
            owner_service_account_id: input.owner_service_account_id,
            material_kind: input.material_kind,
            header_name: input.header_name,
            storage_kind,
            secret_ciphertext,
            secret_nonce,
            secret_key_id,
            secret_ref,
            expires_at: input.expires_at,
            metadata: input.metadata,
            updated_at: now,
        };
        Ok(redact_binding(
            self.repo
                .upsert_mcp_upstream_credential_binding(&record)
                .await?,
        ))
    }

    pub async fn list_bindings(
        &self,
        mcp_server_id: Option<Uuid>,
        owner_scope_kind: Option<McpUpstreamCredentialOwnerScopeKind>,
        owner_scope_id: Option<Uuid>,
        include_revoked: bool,
    ) -> Result<Vec<RedactedMcpCredentialBinding>, GatewayError> {
        Ok(self
            .repo
            .list_mcp_upstream_credential_bindings(
                mcp_server_id,
                owner_scope_kind,
                owner_scope_id,
                include_revoked,
            )
            .await?
            .into_iter()
            .map(redact_binding)
            .collect())
    }

    pub async fn revoke_binding(&self, credential_binding_id: Uuid) -> Result<bool, GatewayError> {
        Ok(self
            .repo
            .revoke_mcp_upstream_credential_binding(
                credential_binding_id,
                OffsetDateTime::now_utc(),
            )
            .await?)
    }
}

pub fn credential_owner_scope_key(
    owner_scope_kind: McpUpstreamCredentialOwnerScopeKind,
    owner_user_id: Option<Uuid>,
    owner_team_id: Option<Uuid>,
    owner_service_account_id: Option<Uuid>,
) -> Result<String, GatewayError> {
    match owner_scope_kind {
        McpUpstreamCredentialOwnerScopeKind::User => {
            let user_id = owner_user_id.ok_or_else(|| {
                GatewayError::InvalidRequest(
                    "user credential binding requires owner_user_id".to_string(),
                )
            })?;
            if owner_team_id.is_some() || owner_service_account_id.is_some() {
                return Err(GatewayError::InvalidRequest(
                    "user credential binding must not include team or service account owners"
                        .to_string(),
                ));
            }
            Ok(format!("mcp_credential:v1:user:{user_id}"))
        }
        McpUpstreamCredentialOwnerScopeKind::Team => {
            let team_id = owner_team_id.ok_or_else(|| {
                GatewayError::InvalidRequest(
                    "team credential binding requires owner_team_id".to_string(),
                )
            })?;
            if owner_user_id.is_some() || owner_service_account_id.is_some() {
                return Err(GatewayError::InvalidRequest(
                    "team credential binding must not include user or service account owners"
                        .to_string(),
                ));
            }
            Ok(format!("mcp_credential:v1:team:{team_id}"))
        }
        McpUpstreamCredentialOwnerScopeKind::ServiceAccount => {
            let team_id = owner_team_id.ok_or_else(|| {
                GatewayError::InvalidRequest(
                    "service-account credential binding requires owner_team_id".to_string(),
                )
            })?;
            let service_account_id = owner_service_account_id.ok_or_else(|| {
                GatewayError::InvalidRequest(
                    "service-account credential binding requires owner_service_account_id"
                        .to_string(),
                )
            })?;
            if owner_user_id.is_some() {
                return Err(GatewayError::InvalidRequest(
                    "service-account credential binding must not include a user owner".to_string(),
                ));
            }
            Ok(format!(
                "mcp_credential:v1:service_account:{team_id}:{service_account_id}"
            ))
        }
    }
}

async fn credential_lookup_order<R>(
    repo: &R,
    auth: &AuthenticatedApiKey,
) -> Result<Vec<String>, GatewayError>
where
    R: IdentityRepository + ?Sized,
{
    match auth.owner_kind {
        ApiKeyOwnerKind::User => {
            let user_id = auth
                .owner_user_id
                .ok_or(gateway_core::AuthError::ApiKeyOwnerInvalid)?;
            let mut keys = vec![credential_owner_scope_key(
                McpUpstreamCredentialOwnerScopeKind::User,
                Some(user_id),
                None,
                None,
            )?];
            let team_id = match auth.owner_team_id {
                Some(team_id) => Some(team_id),
                None => repo
                    .get_team_membership_for_user(user_id)
                    .await?
                    .map(|membership| membership.team_id),
            };
            if let Some(team_id) = team_id {
                keys.push(credential_owner_scope_key(
                    McpUpstreamCredentialOwnerScopeKind::Team,
                    None,
                    Some(team_id),
                    None,
                )?);
            }
            Ok(keys)
        }
        ApiKeyOwnerKind::ServiceAccount => {
            let team_id = auth
                .owner_team_id
                .ok_or(gateway_core::AuthError::ApiKeyOwnerInvalid)?;
            let service_account_id = auth
                .owner_service_account_id
                .ok_or(gateway_core::AuthError::ApiKeyOwnerInvalid)?;
            Ok(vec![
                credential_owner_scope_key(
                    McpUpstreamCredentialOwnerScopeKind::ServiceAccount,
                    None,
                    Some(team_id),
                    Some(service_account_id),
                )?,
                credential_owner_scope_key(
                    McpUpstreamCredentialOwnerScopeKind::Team,
                    None,
                    Some(team_id),
                    None,
                )?,
            ])
        }
    }
}

fn credential_headers(
    binding: &McpUpstreamCredentialBindingRecord,
    server_key: &str,
) -> Result<BTreeMap<String, String>, GatewayError> {
    let secret = match binding.storage_kind {
        McpUpstreamSecretStorageKind::EncryptedBlob => decrypt_binding_secret(binding)
            .map_err(|_| credential_material_unavailable(server_key))?,
        McpUpstreamSecretStorageKind::SecretRef => {
            let secret_ref = binding
                .secret_ref
                .as_deref()
                .ok_or_else(|| credential_material_unavailable(server_key))?;
            resolve_credential_secret_ref(secret_ref)
                .map_err(|_| credential_material_unavailable(server_key))?
        }
    };
    match binding.material_kind {
        McpUpstreamCredentialMaterialKind::StaticHeader => {
            let header_name = binding.header_name.as_deref().ok_or_else(|| {
                StoreError::Serialization(
                    "static header credential is missing header_name".to_string(),
                )
            })?;
            validate_header_name(header_name)?;
            Ok(BTreeMap::from([(header_name.to_string(), secret)]))
        }
        McpUpstreamCredentialMaterialKind::BearerToken
        | McpUpstreamCredentialMaterialKind::OauthTokens => Ok(BTreeMap::from([(
            "Authorization".to_string(),
            format!("Bearer {secret}"),
        )])),
    }
}

fn credential_material_unavailable(server_key: &str) -> GatewayError {
    GatewayError::McpCredentialRequired {
        server_key: server_key.to_string(),
    }
}

fn validate_credential_input(input: &UpsertMcpCredentialBindingInput) -> Result<(), GatewayError> {
    credential_owner_scope_key(
        input.owner_scope_kind,
        input.owner_user_id,
        input.owner_team_id,
        input.owner_service_account_id,
    )?;
    match input.material_kind {
        McpUpstreamCredentialMaterialKind::StaticHeader => {
            let header_name = input.header_name.as_deref().ok_or_else(|| {
                GatewayError::InvalidRequest(
                    "static_header credentials require header_name".to_string(),
                )
            })?;
            validate_header_name(header_name)?;
        }
        McpUpstreamCredentialMaterialKind::BearerToken
        | McpUpstreamCredentialMaterialKind::OauthTokens => {
            if input.header_name.is_some() {
                return Err(GatewayError::InvalidRequest(
                    "bearer credential material must not include header_name".to_string(),
                ));
            }
        }
    }
    Ok(())
}

fn validate_header_name(header_name: &str) -> Result<(), GatewayError> {
    if header_name.trim() != header_name {
        return Err(GatewayError::InvalidRequest(
            "header_name must not contain leading or trailing whitespace".to_string(),
        ));
    }
    reqwest::header::HeaderName::from_bytes(header_name.as_bytes()).map_err(|error| {
        GatewayError::InvalidRequest(format!("header_name is invalid: {error}"))
    })?;
    Ok(())
}

struct EncryptedSecret {
    ciphertext: String,
    nonce: String,
}

fn encrypt_secret(secret: &str) -> Result<EncryptedSecret, GatewayError> {
    let key = credential_cipher_key()?;
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|error| {
        GatewayError::Internal(format!("invalid credential cipher key: {error}"))
    })?;
    let mut nonce_bytes = [0_u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let ciphertext = cipher
        .encrypt((&nonce_bytes).into(), secret.as_bytes())
        .map_err(|error| {
            GatewayError::Internal(format!("failed encrypting MCP credential: {error}"))
        })?;
    Ok(EncryptedSecret {
        ciphertext: BASE64.encode(ciphertext),
        nonce: BASE64.encode(nonce_bytes),
    })
}

fn decrypt_binding_secret(
    binding: &McpUpstreamCredentialBindingRecord,
) -> Result<String, GatewayError> {
    if binding.secret_key_id.as_deref() != Some(CREDENTIAL_KEY_ID) {
        return Err(GatewayError::InvalidRequest(
            "MCP credential was encrypted with an unknown key id".to_string(),
        ));
    }
    let key = credential_cipher_key()?;
    let nonce = BASE64
        .decode(binding.secret_nonce.as_deref().unwrap_or_default())
        .map_err(|error| {
            GatewayError::InvalidRequest(format!("credential nonce is invalid: {error}"))
        })?;
    let ciphertext = BASE64
        .decode(binding.secret_ciphertext.as_deref().unwrap_or_default())
        .map_err(|error| {
            GatewayError::InvalidRequest(format!("credential ciphertext is invalid: {error}"))
        })?;
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|error| {
        GatewayError::Internal(format!("invalid credential cipher key: {error}"))
    })?;
    let plaintext = cipher
        .decrypt(nonce.as_slice().into(), ciphertext.as_ref())
        .map_err(|_| {
            GatewayError::InvalidRequest("MCP credential could not be decrypted".to_string())
        })?;
    String::from_utf8(plaintext).map_err(|error| {
        GatewayError::InvalidRequest(format!("credential secret is not UTF-8: {error}"))
    })
}

fn credential_cipher_key() -> Result<Vec<u8>, GatewayError> {
    let raw = std::env::var(CREDENTIAL_KEY_ENV).map_err(|_| {
        GatewayError::InvalidRequest(format!(
            "{CREDENTIAL_KEY_ENV} must be configured before encrypted MCP credentials can be used"
        ))
    })?;
    let key = BASE64.decode(raw.trim()).map_err(|error| {
        GatewayError::InvalidRequest(format!("{CREDENTIAL_KEY_ENV} must be base64: {error}"))
    })?;
    if key.len() != 32 {
        return Err(GatewayError::InvalidRequest(format!(
            "{CREDENTIAL_KEY_ENV} must decode to exactly 32 bytes"
        )));
    }
    Ok(key)
}

fn validate_credential_secret_ref(secret_ref: &str) -> Result<(), GatewayError> {
    if secret_ref.trim() != secret_ref {
        return Err(GatewayError::InvalidRequest(
            "secret_ref must not contain leading or trailing whitespace".to_string(),
        ));
    }
    let env_name = credential_secret_env_name(secret_ref)?;
    if !env_name.starts_with(CREDENTIAL_SECRET_ENV_PREFIX) {
        return Err(GatewayError::InvalidRequest(format!(
            "credential secret_ref environment variable must start with {CREDENTIAL_SECRET_ENV_PREFIX}"
        )));
    }
    Ok(())
}

fn resolve_credential_secret_ref(secret_ref: &str) -> Result<String, GatewayError> {
    validate_credential_secret_ref(secret_ref)?;
    let env_name = credential_secret_env_name(secret_ref)?;
    std::env::var(env_name).map_err(|_| {
        GatewayError::InvalidRequest(format!(
            "credential secret_ref env/{env_name} is not available for MCP use"
        ))
    })
}

fn credential_secret_env_name(secret_ref: &str) -> Result<&str, GatewayError> {
    let env_name = secret_ref.strip_prefix("env/").ok_or_else(|| {
        GatewayError::InvalidRequest(
            "credential secret_ref must reference an environment variable as env/NAME".to_string(),
        )
    })?;
    if env_name.is_empty()
        || !env_name
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return Err(GatewayError::InvalidRequest(
            "credential secret_ref env name may only contain uppercase letters, digits, and underscore"
                .to_string(),
        ));
    }
    Ok(env_name)
}

fn redact_binding(binding: McpUpstreamCredentialBindingRecord) -> RedactedMcpCredentialBinding {
    RedactedMcpCredentialBinding {
        credential_binding_id: binding.credential_binding_id,
        mcp_server_id: binding.mcp_server_id,
        owner_scope_kind: binding.owner_scope_kind,
        owner_scope_key: binding.owner_scope_key,
        owner_user_id: binding.owner_user_id,
        owner_team_id: binding.owner_team_id,
        owner_service_account_id: binding.owner_service_account_id,
        material_kind: binding.material_kind,
        header_name: binding.header_name,
        storage_kind: binding.storage_kind,
        secret_ref: binding.secret_ref,
        expires_at: binding.expires_at,
        created_at: binding.created_at,
        updated_at: binding.updated_at,
        last_used_at: binding.last_used_at,
        revoked_at: binding.revoked_at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use base64::engine::general_purpose::STANDARD as BASE64;
    use gateway_core::{
        ApiKeyModelGrantMode, ExternalMcpAuthMode, ExternalMcpDiscoveryStatus,
        ExternalMcpServerStatus, ExternalMcpTransport, MembershipRole, StoreError,
        TeamMembershipRecord, TeamRecord, UserRecord,
    };
    use std::{collections::HashMap, sync::Mutex};
    use time::Duration;

    #[test]
    fn service_account_scope_key_includes_team_and_service_account() {
        let team_id = Uuid::new_v4();
        let service_account_id = Uuid::new_v4();
        let key = credential_owner_scope_key(
            McpUpstreamCredentialOwnerScopeKind::ServiceAccount,
            None,
            Some(team_id),
            Some(service_account_id),
        )
        .expect("scope key");
        assert_eq!(
            key,
            format!("mcp_credential:v1:service_account:{team_id}:{service_account_id}")
        );
    }

    #[test]
    fn encryption_key_must_be_32_bytes() {
        let previous = std::env::var_os(CREDENTIAL_KEY_ENV);
        unsafe {
            std::env::set_var(CREDENTIAL_KEY_ENV, BASE64.encode([0_u8; 31]));
        }
        assert!(credential_cipher_key().is_err());
        match previous {
            Some(value) => unsafe {
                std::env::set_var(CREDENTIAL_KEY_ENV, value);
            },
            None => unsafe {
                std::env::remove_var(CREDENTIAL_KEY_ENV);
            },
        }
    }

    #[tokio::test]
    async fn service_account_uses_service_account_then_team_not_user() {
        let team_id = Uuid::new_v4();
        let user_id = Uuid::new_v4();
        let service_account_id = Uuid::new_v4();
        let server = server_record();
        let team_key = credential_owner_scope_key(
            McpUpstreamCredentialOwnerScopeKind::Team,
            None,
            Some(team_id),
            None,
        )
        .expect("team key");
        let user_key = credential_owner_scope_key(
            McpUpstreamCredentialOwnerScopeKind::User,
            Some(user_id),
            None,
            None,
        )
        .expect("user key");
        let repo = ArcCredentialRepo::new([
            (
                team_key.clone(),
                binding(&server, &team_key, "env/OCEANS_MCP_CREDENTIAL_TEAM"),
            ),
            (
                user_key.clone(),
                binding(&server, &user_key, "env/OCEANS_MCP_CREDENTIAL_USER"),
            ),
        ]);
        let _team_secret = EnvVarGuard::set("OCEANS_MCP_CREDENTIAL_TEAM", "team-token");
        let _user_secret = EnvVarGuard::set("OCEANS_MCP_CREDENTIAL_USER", "user-token");
        let service = McpCredentialService::new(std::sync::Arc::new(repo));
        let auth = AuthenticatedApiKey {
            id: Uuid::new_v4(),
            public_id: "gwk_test".to_string(),
            name: "test".to_string(),
            model_grant_mode: ApiKeyModelGrantMode::Explicit,
            owner_kind: ApiKeyOwnerKind::ServiceAccount,
            owner_user_id: None,
            owner_team_id: Some(team_id),
            owner_service_account_id: Some(service_account_id),
        };

        let resolved = service
            .resolve_for_auth(&auth, &server)
            .await
            .expect("resolve");

        assert_eq!(
            resolved.headers.get("Authorization").map(String::as_str),
            Some("Bearer team-token")
        );
        assert_eq!(resolved.owner_scope_key, team_key);
    }

    #[tokio::test]
    async fn expired_principal_binding_falls_back_to_team_binding() {
        let team_id = Uuid::new_v4();
        let user_id = Uuid::new_v4();
        let server = server_record();
        let user_key = credential_owner_scope_key(
            McpUpstreamCredentialOwnerScopeKind::User,
            Some(user_id),
            None,
            None,
        )
        .expect("user key");
        let team_key = credential_owner_scope_key(
            McpUpstreamCredentialOwnerScopeKind::Team,
            None,
            Some(team_id),
            None,
        )
        .expect("team key");
        let mut expired = binding(&server, &user_key, "env/OCEANS_MCP_CREDENTIAL_USER");
        expired.expires_at = Some(OffsetDateTime::now_utc() - Duration::seconds(1));
        let repo = ArcCredentialRepo::new([
            (user_key, expired),
            (
                team_key.clone(),
                binding(&server, &team_key, "env/OCEANS_MCP_CREDENTIAL_TEAM"),
            ),
        ])
        .with_membership(user_id, team_id);
        let _team_secret = EnvVarGuard::set("OCEANS_MCP_CREDENTIAL_TEAM", "team-token");
        let service = McpCredentialService::new(std::sync::Arc::new(repo));
        let auth = AuthenticatedApiKey {
            id: Uuid::new_v4(),
            public_id: "gwk_test".to_string(),
            name: "test".to_string(),
            model_grant_mode: ApiKeyModelGrantMode::Explicit,
            owner_kind: ApiKeyOwnerKind::User,
            owner_user_id: Some(user_id),
            owner_team_id: None,
            owner_service_account_id: None,
        };

        let resolved = service
            .resolve_for_auth(&auth, &server)
            .await
            .expect("resolve team fallback");

        assert_eq!(
            resolved.headers.get("Authorization").map(String::as_str),
            Some("Bearer team-token")
        );
        assert_eq!(resolved.owner_scope_key, team_key);
    }

    #[tokio::test]
    async fn revoked_during_touch_returns_credential_required() {
        let user_id = Uuid::new_v4();
        let server = server_record();
        let user_key = credential_owner_scope_key(
            McpUpstreamCredentialOwnerScopeKind::User,
            Some(user_id),
            None,
            None,
        )
        .expect("user key");
        let repo = ArcCredentialRepo::new([(
            user_key,
            binding(
                &server,
                &format!("mcp_credential:v1:user:{user_id}"),
                "env/OCEANS_MCP_CREDENTIAL_USER",
            ),
        )])
        .with_touch_succeeds(false);
        let _user_secret = EnvVarGuard::set("OCEANS_MCP_CREDENTIAL_USER", "user-token");
        let service = McpCredentialService::new(std::sync::Arc::new(repo));
        let auth = AuthenticatedApiKey {
            id: Uuid::new_v4(),
            public_id: "gwk_test".to_string(),
            name: "test".to_string(),
            model_grant_mode: ApiKeyModelGrantMode::Explicit,
            owner_kind: ApiKeyOwnerKind::User,
            owner_user_id: Some(user_id),
            owner_team_id: None,
            owner_service_account_id: None,
        };

        let error = service
            .resolve_for_auth(&auth, &server)
            .await
            .expect_err("lost revocation race");

        assert_eq!(error.error_code(), "credential_required");
    }

    #[tokio::test]
    async fn expired_credential_errors_when_no_fallback_exists() {
        let user_id = Uuid::new_v4();
        let server = server_record();
        let user_key = credential_owner_scope_key(
            McpUpstreamCredentialOwnerScopeKind::User,
            Some(user_id),
            None,
            None,
        )
        .expect("user key");
        let mut expired = binding(&server, &user_key, "env/OCEANS_MCP_CREDENTIAL_USER");
        expired.expires_at = Some(OffsetDateTime::now_utc() - Duration::seconds(1));
        let repo = ArcCredentialRepo::new([(user_key, expired)]);
        let service = McpCredentialService::new(std::sync::Arc::new(repo));
        let auth = AuthenticatedApiKey {
            id: Uuid::new_v4(),
            public_id: "gwk_test".to_string(),
            name: "test".to_string(),
            model_grant_mode: ApiKeyModelGrantMode::Explicit,
            owner_kind: ApiKeyOwnerKind::User,
            owner_user_id: Some(user_id),
            owner_team_id: None,
            owner_service_account_id: None,
        };

        let error = service
            .resolve_for_auth(&auth, &server)
            .await
            .expect_err("expired");

        assert_eq!(error.error_code(), "credential_expired");
    }

    #[tokio::test]
    async fn missing_secret_ref_material_returns_credential_required() {
        let user_id = Uuid::new_v4();
        let server = server_record();
        let user_key = credential_owner_scope_key(
            McpUpstreamCredentialOwnerScopeKind::User,
            Some(user_id),
            None,
            None,
        )
        .expect("user key");
        let repo = ArcCredentialRepo::new([(
            user_key,
            binding(
                &server,
                &format!("mcp_credential:v1:user:{user_id}"),
                "env/OCEANS_MCP_CREDENTIAL_MISSING",
            ),
        )]);
        let _missing_secret = EnvVarGuard::remove("OCEANS_MCP_CREDENTIAL_MISSING");
        let service = McpCredentialService::new(std::sync::Arc::new(repo));
        let auth = AuthenticatedApiKey {
            id: Uuid::new_v4(),
            public_id: "gwk_test".to_string(),
            name: "test".to_string(),
            model_grant_mode: ApiKeyModelGrantMode::Explicit,
            owner_kind: ApiKeyOwnerKind::User,
            owner_user_id: Some(user_id),
            owner_team_id: None,
            owner_service_account_id: None,
        };

        let error = service
            .resolve_for_auth(&auth, &server)
            .await
            .expect_err("missing secret");

        assert_eq!(error.error_code(), "credential_required");
        assert!(!error.to_string().contains("OCEANS_MCP_CREDENTIAL_MISSING"));
    }

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var_os(key);
            unsafe {
                std::env::set_var(key, value);
            }
            Self { key, previous }
        }

        fn remove(key: &'static str) -> Self {
            let previous = std::env::var_os(key);
            unsafe {
                std::env::remove_var(key);
            }
            Self { key, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => unsafe {
                    std::env::set_var(self.key, value);
                },
                None => unsafe {
                    std::env::remove_var(self.key);
                },
            }
        }
    }

    struct ArcCredentialRepo {
        bindings: Mutex<HashMap<(Uuid, String), McpUpstreamCredentialBindingRecord>>,
        memberships: Mutex<HashMap<Uuid, Uuid>>,
        touch_succeeds: Mutex<bool>,
    }

    impl ArcCredentialRepo {
        fn new(
            bindings: impl IntoIterator<Item = (String, McpUpstreamCredentialBindingRecord)>,
        ) -> Self {
            Self {
                bindings: Mutex::new(
                    bindings
                        .into_iter()
                        .map(|(key, binding)| ((binding.mcp_server_id, key), binding))
                        .collect(),
                ),
                memberships: Mutex::new(HashMap::new()),
                touch_succeeds: Mutex::new(true),
            }
        }

        fn with_membership(self, user_id: Uuid, team_id: Uuid) -> Self {
            self.memberships
                .lock()
                .expect("memberships")
                .insert(user_id, team_id);
            self
        }

        fn with_touch_succeeds(self, succeeds: bool) -> Self {
            *self.touch_succeeds.lock().expect("touch succeeds") = succeeds;
            self
        }
    }

    #[async_trait]
    impl McpUpstreamCredentialRepository for ArcCredentialRepo {
        async fn upsert_mcp_upstream_credential_binding(
            &self,
            _input: &UpsertMcpUpstreamCredentialBindingRecord,
        ) -> Result<McpUpstreamCredentialBindingRecord, StoreError> {
            unimplemented!()
        }

        async fn get_active_mcp_upstream_credential_binding(
            &self,
            mcp_server_id: Uuid,
            owner_scope_key: &str,
        ) -> Result<Option<McpUpstreamCredentialBindingRecord>, StoreError> {
            Ok(self
                .bindings
                .lock()
                .expect("bindings")
                .get(&(mcp_server_id, owner_scope_key.to_string()))
                .cloned())
        }

        async fn list_mcp_upstream_credential_bindings(
            &self,
            _mcp_server_id: Option<Uuid>,
            _owner_scope_kind: Option<McpUpstreamCredentialOwnerScopeKind>,
            _owner_scope_id: Option<Uuid>,
            _include_revoked: bool,
        ) -> Result<Vec<McpUpstreamCredentialBindingRecord>, StoreError> {
            unimplemented!()
        }

        async fn revoke_mcp_upstream_credential_binding(
            &self,
            _credential_binding_id: Uuid,
            _revoked_at: OffsetDateTime,
        ) -> Result<bool, StoreError> {
            unimplemented!()
        }

        async fn touch_mcp_upstream_credential_binding_last_used(
            &self,
            _credential_binding_id: Uuid,
            _last_used_at: OffsetDateTime,
        ) -> Result<bool, StoreError> {
            Ok(*self.touch_succeeds.lock().expect("touch succeeds"))
        }
    }

    #[async_trait]
    impl IdentityRepository for ArcCredentialRepo {
        async fn get_user_by_id(&self, _user_id: Uuid) -> Result<Option<UserRecord>, StoreError> {
            unimplemented!()
        }

        async fn get_team_by_id(&self, _team_id: Uuid) -> Result<Option<TeamRecord>, StoreError> {
            unimplemented!()
        }

        async fn get_team_membership_for_user(
            &self,
            user_id: Uuid,
        ) -> Result<Option<TeamMembershipRecord>, StoreError> {
            Ok(self
                .memberships
                .lock()
                .expect("memberships")
                .get(&user_id)
                .map(|team_id| {
                    let now = OffsetDateTime::now_utc();
                    TeamMembershipRecord {
                        team_id: *team_id,
                        user_id,
                        role: MembershipRole::Member,
                        created_at: now,
                        updated_at: now,
                    }
                }))
        }

        async fn list_allowed_model_keys_for_user(
            &self,
            _user_id: Uuid,
        ) -> Result<Vec<String>, StoreError> {
            unimplemented!()
        }

        async fn list_allowed_model_keys_for_team(
            &self,
            _team_id: Uuid,
        ) -> Result<Vec<String>, StoreError> {
            unimplemented!()
        }
    }

    fn binding(
        server: &ExternalMcpServerRecord,
        owner_scope_key: &str,
        secret_ref: &str,
    ) -> McpUpstreamCredentialBindingRecord {
        let now = OffsetDateTime::now_utc();
        McpUpstreamCredentialBindingRecord {
            credential_binding_id: Uuid::new_v4(),
            mcp_server_id: server.mcp_server_id,
            owner_scope_kind: McpUpstreamCredentialOwnerScopeKind::Team,
            owner_scope_key: owner_scope_key.to_string(),
            owner_user_id: None,
            owner_team_id: Some(Uuid::new_v4()),
            owner_service_account_id: None,
            material_kind: McpUpstreamCredentialMaterialKind::BearerToken,
            header_name: None,
            storage_kind: McpUpstreamSecretStorageKind::SecretRef,
            secret_ciphertext: None,
            secret_nonce: None,
            secret_key_id: None,
            secret_ref: Some(secret_ref.to_string()),
            expires_at: None,
            metadata: Map::new(),
            created_at: now,
            updated_at: now,
            last_used_at: None,
            revoked_at: None,
        }
    }

    fn server_record() -> ExternalMcpServerRecord {
        let now = OffsetDateTime::now_utc();
        ExternalMcpServerRecord {
            mcp_server_id: Uuid::new_v4(),
            server_key: "github".to_string(),
            display_name: "GitHub".to_string(),
            description: None,
            transport: ExternalMcpTransport::StreamableHttp,
            server_url: "https://example.test/mcp".to_string(),
            auth_mode: ExternalMcpAuthMode::UserPassthrough,
            auth_config: Map::new(),
            timeout_ms: 30_000,
            status: ExternalMcpServerStatus::Active,
            last_discovery_status: Some(ExternalMcpDiscoveryStatus::Success),
            last_discovery_at: Some(now),
            last_successful_discovery_at: Some(now),
            last_error_summary: None,
            last_tool_count: Some(1),
            created_at: now,
            updated_at: now,
            disabled_at: None,
        }
    }
}
