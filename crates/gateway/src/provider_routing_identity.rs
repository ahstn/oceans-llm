//! Opaque identities for the configured upstream account and request defaults.
//!
//! These hashes describe startup configuration, not refreshed credentials. File-backed and
//! ambient credentials require an operator account scope to identify their stable principal.

use std::{collections::BTreeMap, path::Path};

use gateway_providers::{
    AnthropicCompatAuthKind, AnthropicCompatConfig, BearerAuthHeader, BedrockAuthConfig,
    BedrockProviderConfig, CloudRunOpenAiCompatAuth, CopilotAuthConfig, CopilotProviderConfig,
    OpenAiCompatConfig, TypeSafeConfig, VertexAuthConfig, VertexProviderConfig,
};
use sha2::{Digest, Sha256};

/// Bind dynamic credentials to an operator-declared principal without hashing refreshed keys.
pub fn with_account_scope(identity: &str, scope: Option<&str>) -> String {
    let mut scoped = RoutingIdentity::new("account_scope");
    scoped.add("provider_identity", identity);
    scoped.optional("account_scope", scope);
    scoped.finish()
}

/// Hash the initialized request configuration, including resolved static secrets.
pub fn openai_compat(config: &OpenAiCompatConfig) -> anyhow::Result<String> {
    let mut identity = RoutingIdentity::new("openai_compat");
    identity.add("provider_type", &config.provider_type);
    identity.add("base_url", &config.base_url);
    identity.optional("bearer_token", config.bearer_token.as_deref());
    identity.add(
        "bearer_auth_header",
        match config.bearer_auth_header {
            BearerAuthHeader::Authorization => "authorization",
            BearerAuthHeader::XServerlessAuthorization => "x-serverless-authorization",
        },
    );
    if config.identity_token_source.is_some() && config.routing_auth_identity.is_none() {
        anyhow::bail!("routing identity is required for an OpenAI identity token source");
    }
    identity.add(
        "has_identity_token_source",
        [u8::from(config.identity_token_source.is_some())],
    );
    identity.optional("identity_auth", config.routing_auth_identity.as_deref());
    identity.optional("decisions_url", config.decisions_url.as_deref());
    identity.headers(&config.default_headers);
    Ok(identity.finish())
}

/// Capture Cloud Run auth before its token source hides the configured auth details.
pub fn cloud_run_auth(auth: &CloudRunOpenAiCompatAuth) -> String {
    let mut identity = RoutingIdentity::new("cloud_run_auth");
    match auth {
        CloudRunOpenAiCompatAuth::Adc { audience } => {
            identity.add("auth_kind", "adc");
            identity.add("audience", audience);
        }
        CloudRunOpenAiCompatAuth::ServiceAccount {
            credentials_path,
            audience,
        } => {
            identity.add("auth_kind", "service_account");
            identity.service_account_file(credentials_path);
            identity.add("audience", audience);
        }
        CloudRunOpenAiCompatAuth::Bearer { token } => {
            identity.add("auth_kind", "bearer");
            identity.add("token", token);
        }
    }
    identity.finish()
}

pub fn anthropic_compat(config: &AnthropicCompatConfig) -> String {
    let mut identity = RoutingIdentity::new("anthropic_compat");
    identity.add("provider_type", &config.provider_type);
    identity.add("base_url", &config.base_url);
    match &config.auth {
        Some(auth) => {
            identity.add(
                "auth_kind",
                match auth.kind {
                    AnthropicCompatAuthKind::XApiKey => "x_api_key",
                    AnthropicCompatAuthKind::Bearer => "bearer",
                },
            );
            identity.add("token", &auth.token);
        }
        None => identity.add("auth_kind", "none"),
    }
    identity.headers(&config.default_headers);
    identity.finish()
}

pub fn typesafe(config: &TypeSafeConfig) -> String {
    let mut identity = RoutingIdentity::new("typesafe");
    identity.add("base_url", &config.base_url);
    identity.optional("bearer_token", config.bearer_token.as_deref());
    identity.headers(&config.default_headers);
    identity.finish()
}

pub fn vertex(config: &VertexProviderConfig) -> String {
    let mut identity = RoutingIdentity::new("vertex");
    identity.add("project_id", &config.project_id);
    identity.add("location", &config.location);
    identity.add("api_host", &config.api_host);
    match &config.auth {
        VertexAuthConfig::Adc => identity.add("auth_kind", "adc"),
        VertexAuthConfig::ServiceAccount { credentials_path } => {
            identity.add("auth_kind", "service_account");
            identity.service_account_file(credentials_path);
        }
        VertexAuthConfig::Bearer { token } => {
            identity.add("auth_kind", "bearer");
            identity.add("token", token);
        }
    }
    identity.headers(&config.default_headers);
    identity.finish()
}

pub fn bedrock(config: &BedrockProviderConfig) -> String {
    let mut identity = RoutingIdentity::new("bedrock");
    identity.add("region", &config.region);
    identity.add("endpoint_kind", config.endpoint_kind.as_config_value());
    identity.add("endpoint_url", &config.endpoint_url);
    match &config.auth {
        BedrockAuthConfig::DefaultChain => identity.add("auth_kind", "default_chain"),
        BedrockAuthConfig::Bearer { token } => {
            identity.add("auth_kind", "bearer");
            identity.add("token", token);
        }
        BedrockAuthConfig::StaticCredentials {
            access_key_id,
            secret_access_key,
            session_token,
        } => {
            identity.add("auth_kind", "static_credentials");
            identity.add("access_key_id", access_key_id);
            identity.add("secret_access_key", secret_access_key);
            identity.optional("session_token", session_token.as_deref());
        }
    }
    identity.headers(&config.default_headers);
    identity.finish()
}

pub fn copilot(config: &CopilotProviderConfig) -> String {
    let mut identity = RoutingIdentity::new("copilot");
    identity.add("base_url", &config.base_url);
    identity.optional("github_api_url", config.github_api_url.as_deref());
    identity.add("editor_version", &config.editor_version);
    identity.add("integration_id", &config.integration_id);
    match &config.auth {
        CopilotAuthConfig::GitHubApp {
            app_id,
            installation_id,
            repository_id,
            ..
        }
        | CopilotAuthConfig::GitHubAppKeyFile {
            app_id,
            installation_id,
            repository_id,
            ..
        } => {
            // App keys rotate without changing the installation or account identity.
            identity.add("auth_kind", "github_app");
            identity.add("app_id", app_id.to_be_bytes());
            identity.add("installation_id", installation_id.to_be_bytes());
            identity.add("repository_id", repository_id.to_be_bytes());
        }
        CopilotAuthConfig::GitHubUser => identity.add("auth_kind", "github_user"),
        CopilotAuthConfig::Bearer { token } => {
            identity.add("auth_kind", "bearer");
            identity.add("token", token);
        }
    }
    identity.headers(&config.default_headers);
    identity.finish()
}

struct RoutingIdentity(Sha256);

impl RoutingIdentity {
    fn new(provider_kind: &str) -> Self {
        let mut identity = Self(Sha256::new());
        identity.add("format", "oceans-provider-routing-v1");
        identity.add("provider_kind", provider_kind);
        identity
    }

    fn add(&mut self, name: &str, value: impl AsRef<[u8]>) {
        let value = value.as_ref();
        self.0.update((name.len() as u64).to_be_bytes());
        self.0.update(name.as_bytes());
        self.0.update((value.len() as u64).to_be_bytes());
        self.0.update(value);
    }

    fn optional(&mut self, name: &str, value: Option<&str>) {
        self.add(name, [u8::from(value.is_some())]);
        if let Some(value) = value {
            self.add(name, value);
        }
    }

    fn headers(&mut self, headers: &BTreeMap<String, String>) {
        for (name, value) in headers {
            self.add("header_name", name);
            self.add("header_value", value);
        }
    }

    fn service_account_file(&mut self, path: &Path) {
        // Token sources reread this file on refresh. Key rotation preserves the account;
        // a principal change requires a new operator scope and a gateway restart.
        self.add("service_account_path", path.as_os_str().as_encoded_bytes());
    }

    fn finish(self) -> String {
        format!("{:x}", self.0.finalize())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use gateway_core::ProviderRegistry;
    use gateway_providers::{
        AnthropicCompatAuth, BedrockEndpointKind, OpenAiBatchConfig, OpenAiBatchDialect,
        OpenAiCompatProvider, VertexBatchConfig,
    };

    #[test]
    fn openai_identity_tracks_resolved_request_settings() {
        let mut config = OpenAiCompatConfig::new("provider".into(), "https://first.test".into());
        config.bearer_token = Some("resolved-first-token".into());
        let original = openai_compat(&config).unwrap();

        let mut changed = config.clone();
        changed.base_url = "https://second.test".into();
        assert_ne!(original, openai_compat(&changed).unwrap());
        changed = config.clone();
        changed.bearer_token = Some("resolved-second-token".into());
        assert_ne!(original, openai_compat(&changed).unwrap());
        changed = config.clone();
        changed
            .default_headers
            .insert("x-account".into(), "other".into());
        assert_ne!(original, openai_compat(&changed).unwrap());
        changed = config.clone();
        changed.bearer_auth_header = BearerAuthHeader::XServerlessAuthorization;
        assert_ne!(original, openai_compat(&changed).unwrap());
        changed = config.clone();
        changed.decisions_url = Some("https://decisions.test".into());
        assert_ne!(original, openai_compat(&changed).unwrap());
        assert_eq!(original.len(), 64);
        assert!(original.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }

    #[test]
    fn openai_identity_ignores_labels_timeout_and_batch_settings() {
        let mut config = OpenAiCompatConfig::new("provider".into(), "https://test.test".into());
        let original = openai_compat(&config).unwrap();
        config.provider_key = "renamed-provider".into();
        config.request_timeout_ms += 1;
        config.batch = OpenAiBatchConfig {
            dialect: OpenAiBatchDialect::OpenRouter,
            base_url: Some("https://batch.test".into()),
        };
        assert_eq!(original, openai_compat(&config).unwrap());
    }

    #[test]
    fn anthropic_identity_distinguishes_auth_modes_and_tokens() {
        let mut config = AnthropicCompatConfig::new("provider".into(), "https://test.test".into());
        let unauthenticated = anthropic_compat(&config);
        config.auth = Some(AnthropicCompatAuth {
            kind: AnthropicCompatAuthKind::XApiKey,
            token: "first-token".into(),
        });
        let api_key = anthropic_compat(&config);
        assert_ne!(unauthenticated, api_key);
        config.auth.as_mut().unwrap().kind = AnthropicCompatAuthKind::Bearer;
        assert_ne!(api_key, anthropic_compat(&config));
        config.auth.as_mut().unwrap().kind = AnthropicCompatAuthKind::XApiKey;
        config.auth.as_mut().unwrap().token = "second-token".into();
        assert_ne!(api_key, anthropic_compat(&config));
    }

    #[test]
    fn typesafe_identity_tracks_token_and_headers_but_ignores_timeout() {
        let mut config = TypeSafeConfig::new("provider".into(), "https://test.test".into());
        let original = typesafe(&config);
        config.request_timeout_ms += 1;
        assert_eq!(original, typesafe(&config));
        config.bearer_token = Some("resolved-token".into());
        let authenticated = typesafe(&config);
        assert_ne!(original, authenticated);
        config
            .default_headers
            .insert("x-account".into(), "other".into());
        assert_ne!(authenticated, typesafe(&config));
    }

    fn vertex_config(auth: VertexAuthConfig) -> VertexProviderConfig {
        VertexProviderConfig {
            provider_key: "vertex".into(),
            project_id: "project".into(),
            location: "us-central1".into(),
            api_host: "us-central1-aiplatform.googleapis.com".into(),
            auth,
            default_headers: BTreeMap::new(),
            request_timeout_ms: 300_000,
            batch: None,
        }
    }

    #[test]
    fn service_account_identity_preserves_key_rotation_within_account_scope() {
        let directory = tempfile::tempdir().unwrap();
        let first = directory.path().join("first.json");
        std::fs::write(&first, "first-account-credentials").unwrap();
        let config = vertex_config(VertexAuthConfig::ServiceAccount {
            credentials_path: first.clone(),
        });
        let original = vertex(&config);
        std::fs::write(&first, "rotated-account-credentials").unwrap();
        assert_eq!(original, vertex(&config));
        assert_ne!(
            with_account_scope(&original, Some("first-principal")),
            with_account_scope(&original, Some("second-principal"))
        );
        assert_ne!(
            with_account_scope(&original, Some("first-principal")),
            with_account_scope(&original, None)
        );
    }

    #[test]
    fn vertex_identity_tracks_account_and_endpoint_but_ignores_batch() {
        let mut config = vertex_config(VertexAuthConfig::Adc);
        let original = vertex(&config);
        config.batch = Some(VertexBatchConfig {
            bigquery_project_id: "batch-project".into(),
            dataset: "batch-dataset".into(),
        });
        config.request_timeout_ms += 1;
        assert_eq!(original, vertex(&config));
        config.project_id = "other-project".into();
        assert_ne!(original, vertex(&config));
        config.project_id = "project".into();
        config.api_host = "private-endpoint.test".into();
        assert_ne!(original, vertex(&config));
        config.api_host = "us-central1-aiplatform.googleapis.com".into();
        config.auth = VertexAuthConfig::Bearer {
            token: "resolved-token".into(),
        };
        assert_ne!(original, vertex(&config));
    }

    fn bedrock_config(auth: BedrockAuthConfig) -> BedrockProviderConfig {
        BedrockProviderConfig {
            provider_key: "bedrock".into(),
            region: "us-east-1".into(),
            endpoint_kind: BedrockEndpointKind::BedrockRuntime,
            endpoint_url: "https://bedrock-runtime.us-east-1.amazonaws.com".into(),
            auth,
            default_headers: BTreeMap::new(),
            request_timeout_ms: 300_000,
        }
    }

    #[test]
    fn bedrock_identity_tracks_all_static_credentials() {
        let config = bedrock_config(BedrockAuthConfig::StaticCredentials {
            access_key_id: "access-key".into(),
            secret_access_key: "secret-key".into(),
            session_token: None,
        });
        let original = bedrock(&config);
        for auth in [
            BedrockAuthConfig::DefaultChain,
            BedrockAuthConfig::Bearer {
                token: "token".into(),
            },
            BedrockAuthConfig::StaticCredentials {
                access_key_id: "other-access-key".into(),
                secret_access_key: "secret-key".into(),
                session_token: None,
            },
            BedrockAuthConfig::StaticCredentials {
                access_key_id: "access-key".into(),
                secret_access_key: "other-secret-key".into(),
                session_token: None,
            },
            BedrockAuthConfig::StaticCredentials {
                access_key_id: "access-key".into(),
                secret_access_key: "secret-key".into(),
                session_token: Some("session-token".into()),
            },
        ] {
            assert_ne!(original, bedrock(&bedrock_config(auth)));
        }
    }

    #[test]
    fn copilot_app_identity_tracks_installation_but_ignores_private_key_rotation() {
        let mut config = CopilotProviderConfig::new(
            "copilot".into(),
            CopilotAuthConfig::GitHubApp {
                app_id: 1,
                private_key_pem: "first-private-key".into(),
                installation_id: 2,
                repository_id: 3,
            },
        );
        let original = copilot(&config);
        config.auth = CopilotAuthConfig::GitHubAppKeyFile {
            app_id: 1,
            private_key_path: "unused-key-path".into(),
            installation_id: 2,
            repository_id: 3,
        };
        assert_eq!(original, copilot(&config));
        for (app_id, installation_id, repository_id) in [(9, 2, 3), (1, 9, 3), (1, 2, 9)] {
            config.auth = CopilotAuthConfig::GitHubApp {
                app_id,
                private_key_pem: "rotated-private-key".into(),
                installation_id,
                repository_id,
            };
            assert_ne!(original, copilot(&config));
        }
        config.auth = CopilotAuthConfig::GitHubUser;
        assert_ne!(original, copilot(&config));
        config.auth = CopilotAuthConfig::Bearer {
            token: "static-token".into(),
        };
        assert_ne!(original, copilot(&config));
    }

    #[test]
    fn cloud_run_identity_tracks_audience_and_auth_kind() {
        let original = cloud_run_auth(&CloudRunOpenAiCompatAuth::Adc {
            audience: "https://first.test".into(),
        });
        assert_ne!(
            original,
            cloud_run_auth(&CloudRunOpenAiCompatAuth::Adc {
                audience: "https://second.test".into(),
            })
        );
        assert_ne!(
            original,
            cloud_run_auth(&CloudRunOpenAiCompatAuth::Bearer {
                token: "static-token".into(),
            })
        );
        let directory = tempfile::tempdir().unwrap();
        let credentials_path = directory.path().join("credentials.json");
        std::fs::write(&credentials_path, "service-account-credentials").unwrap();
        assert_ne!(
            original,
            cloud_run_auth(&CloudRunOpenAiCompatAuth::ServiceAccount {
                credentials_path,
                audience: "https://first.test".into(),
            })
        );
    }

    #[test]
    fn cloud_run_identity_requires_and_includes_preserved_auth_details() {
        let auth = CloudRunOpenAiCompatAuth::Adc {
            audience: "https://first.test".into(),
        };
        let mut config = OpenAiCompatConfig::new_cloud_run(
            "cloud-run".into(),
            "https://first.test".into(),
            BearerAuthHeader::Authorization,
            auth.clone(),
        )
        .unwrap();
        assert!(openai_compat(&config).is_err());
        config.routing_auth_identity = Some(cloud_run_auth(&auth));
        let original = openai_compat(&config).unwrap();
        config.routing_auth_identity = Some(cloud_run_auth(&CloudRunOpenAiCompatAuth::Adc {
            audience: "https://second.test".into(),
        }));
        assert_ne!(original, openai_compat(&config).unwrap());
    }

    #[test]
    fn registry_retains_identity_on_clone_and_clears_it_on_plain_registration() {
        let config = OpenAiCompatConfig::new("provider".into(), "https://test.test".into());
        let identity = openai_compat(&config).unwrap();
        let provider = Arc::new(OpenAiCompatProvider::new(config).unwrap());
        let mut registry = ProviderRegistry::new();
        registry.register_with_routing_identity(provider.clone(), identity.clone());
        assert_eq!(
            registry.routing_identity("provider"),
            Some(identity.as_str())
        );
        let cloned = registry.clone();
        assert_eq!(cloned.routing_identity("provider"), Some(identity.as_str()));
        registry.register(provider);
        assert_eq!(registry.routing_identity("provider"), None);
        assert_eq!(cloned.routing_identity("provider"), Some(identity.as_str()));
    }
}
