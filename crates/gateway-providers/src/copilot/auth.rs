use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use gateway_core::ProviderError;
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::http::map_reqwest_error;
use crate::token::{AccessToken, AccessTokenSource};

const DEFAULT_GITHUB_API_URL: &str = "https://api.github.com";

#[derive(Debug, Serialize)]
struct GitHubAppJwtClaims {
    iat: i64,
    exp: i64,
    iss: u64,
}

#[derive(Debug, Deserialize)]
struct GitHubInstallationTokenResponse {
    token: String,
    #[serde(with = "time::serde::rfc3339")]
    expires_at: OffsetDateTime,
}

#[derive(Clone)]
pub struct GitHubAppInstallationTokenSource {
    client: reqwest::Client,
    app_id: u64,
    private_key: EncodingKey,
    installation_id: u64,
    repository_id: Option<u64>,
    api_url: String,
}

impl GitHubAppInstallationTokenSource {
    pub fn new(
        app_id: u64,
        private_key_pem: &str,
        installation_id: u64,
        repository_id: Option<u64>,
    ) -> Result<Self, ProviderError> {
        let key = EncodingKey::from_rsa_pem(private_key_pem.as_bytes()).map_err(|error| {
            ProviderError::InvalidRequest(format!(
                "invalid GitHub App RSA private key PEM: {error}"
            ))
        })?;

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(map_reqwest_error)?;

        Ok(Self {
            client,
            app_id,
            private_key: key,
            installation_id,
            repository_id,
            api_url: DEFAULT_GITHUB_API_URL.to_string(),
        })
    }

    pub fn from_key_path(
        app_id: u64,
        private_key_path: &Path,
        installation_id: u64,
        repository_id: Option<u64>,
    ) -> Result<Self, ProviderError> {
        let pem = std::fs::read_to_string(private_key_path).map_err(|error| {
            ProviderError::InvalidRequest(format!(
                "failed to read GitHub App private key `{}`: {error}",
                private_key_path.display()
            ))
        })?;
        Self::new(app_id, &pem, installation_id, repository_id)
    }

    #[must_use]
    pub fn with_api_url(mut self, api_url: String) -> Self {
        self.api_url = api_url.trim_end_matches('/').to_string();
        self
    }

    fn generate_jwt(&self) -> Result<String, ProviderError> {
        let now = OffsetDateTime::now_utc();
        // GitHub recommends 60s clock drift tolerance (iat 60s in past) and max 10m validity
        let claims = GitHubAppJwtClaims {
            iat: (now - time::Duration::seconds(60)).unix_timestamp(),
            exp: (now + time::Duration::minutes(9)).unix_timestamp(),
            iss: self.app_id,
        };

        let header = Header::new(Algorithm::RS256);
        jsonwebtoken::encode(&header, &claims, &self.private_key).map_err(|error| {
            ProviderError::Transport(format!("failed to sign GitHub App JWT: {error}"))
        })
    }
}

#[async_trait]
impl AccessTokenSource for GitHubAppInstallationTokenSource {
    async fn fetch_token(&self) -> Result<AccessToken, ProviderError> {
        let jwt = self.generate_jwt()?;
        let url = format!(
            "{}/app/installations/{}/access_tokens",
            self.api_url, self.installation_id
        );

        let mut body = serde_json::json!({
            "permissions": {
                "copilot_requests": "write"
            }
        });

        if let Some(repo_id) = self.repository_id
            && let Some(map) = body.as_object_mut()
        {
            map.insert(
                "repository_ids".to_string(),
                serde_json::Value::Array(vec![serde_json::Value::Number(repo_id.into())]),
            );
        }

        let response = self
            .client
            .post(&url)
            .bearer_auth(jwt)
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "oceans-llm-gateway")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .json(&body)
            .send()
            .await
            .map_err(map_reqwest_error)?;

        let status = response.status();
        let text = response.text().await.map_err(map_reqwest_error)?;

        if !status.is_success() {
            return Err(ProviderError::UpstreamHttp {
                status: status.as_u16(),
                body: text,
            });
        }

        let parsed: GitHubInstallationTokenResponse =
            serde_json::from_str(&text).map_err(|error| {
                ProviderError::Transport(format!(
                    "invalid GitHub App installation token response: {error}"
                ))
            })?;

        Ok(AccessToken {
            token: parsed.token,
            expires_at: parsed.expires_at,
        })
    }
}

#[derive(Debug, Clone)]
pub enum CopilotAuthConfig {
    GitHubApp {
        app_id: u64,
        private_key_pem: String,
        installation_id: u64,
        repository_id: Option<u64>,
    },
    GitHubAppKeyFile {
        app_id: u64,
        private_key_path: PathBuf,
        installation_id: u64,
        repository_id: Option<u64>,
    },
    Bearer {
        token: String,
    },
}

impl CopilotAuthConfig {
    pub fn build_source(
        &self,
        api_url_override: Option<&str>,
    ) -> Result<Arc<dyn AccessTokenSource>, ProviderError> {
        match self {
            Self::GitHubApp {
                app_id,
                private_key_pem,
                installation_id,
                repository_id,
            } => {
                let mut source = GitHubAppInstallationTokenSource::new(
                    *app_id,
                    private_key_pem,
                    *installation_id,
                    *repository_id,
                )?;
                if let Some(url) = api_url_override {
                    source = source.with_api_url(url.to_string());
                }
                Ok(Arc::new(source))
            }
            Self::GitHubAppKeyFile {
                app_id,
                private_key_path,
                installation_id,
                repository_id,
            } => {
                let mut source = GitHubAppInstallationTokenSource::from_key_path(
                    *app_id,
                    private_key_path,
                    *installation_id,
                    *repository_id,
                )?;
                if let Some(url) = api_url_override {
                    source = source.with_api_url(url.to_string());
                }
                Ok(Arc::new(source))
            }
            Self::Bearer { token } => Ok(Arc::new(crate::token::StaticBearerTokenSource::new(
                token.clone(),
            ))),
        }
    }
}
