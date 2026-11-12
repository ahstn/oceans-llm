use std::{
    collections::{BTreeMap, HashMap},
    pin::Pin,
    sync::Arc,
};

use async_trait::async_trait;
use bytes::Bytes;
use futures_core::Stream;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    batch::{
        BatchCapabilities, ProviderBatchRequest, ProviderBatchResult, ProviderBatchState,
        ProviderBatchSubmission,
    },
    domain::{ModelRoute, RouteCompatibility},
    protocol::{
        core::{ChatRequest, EmbeddingsRequest, ImagesRequest, ResponsesRequest},
        decisions::{DecisionsRequest, ProviderDecisionsResponse},
    },
    service_tier::ServiceTier,
};

/// Public API contract used when dispatching through the common chat execution path.
/// This is trusted endpoint metadata, never inferred from a caller's JSON fields.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ChatRequestProtocol {
    #[default]
    ChatCompletions,
    AnthropicMessages,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderRequestContext {
    pub request_id: String,
    pub model_key: String,
    pub provider_key: String,
    pub upstream_model: String,
    /// Trusted user identity derived from the authenticated gateway API key.
    #[serde(default)]
    pub owner_user_id: Option<Uuid>,
    /// Credential generation selected during routing, if the provider uses user credentials.
    #[serde(default)]
    pub expected_provider_credential_id: Option<Uuid>,
    /// Only relevant to chat execution; skipped so callers cannot override the endpoint contract.
    #[serde(skip)]
    pub chat_protocol: ChatRequestProtocol,
    #[serde(default)]
    pub extra_headers: Map<String, Value>,
    #[serde(default)]
    pub extra_body: Map<String, Value>,
    #[serde(default)]
    pub request_headers: BTreeMap<String, String>,
    #[serde(default)]
    pub compatibility: RouteCompatibility,
    /// Effective tier for this request: the route's forced tier, else the caller's.
    /// Request bodies never carry tier fields; adapters write this in their wire form.
    #[serde(default)]
    pub service_tier: Option<ServiceTier>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderCapabilities {
    #[serde(default = "default_true")]
    pub chat_completions: bool,
    #[serde(default = "default_true")]
    pub responses: bool,
    #[serde(default = "default_true")]
    pub stream: bool,
    #[serde(default = "default_true")]
    pub embeddings: bool,
    /// Images API generation (`POST /v1/images`). Opt-in per route: only
    /// image models serve it.
    #[serde(default)]
    pub images: bool,
    #[serde(default)]
    pub decisions: bool,
    #[serde(default = "default_true")]
    pub tools: bool,
    #[serde(default = "default_true")]
    pub vision: bool,
    #[serde(default = "default_true")]
    pub json_schema: bool,
    #[serde(default = "default_true")]
    pub developer_role: bool,
}

impl ProviderCapabilities {
    #[must_use]
    pub const fn none() -> Self {
        Self {
            chat_completions: false,
            responses: false,
            stream: false,
            embeddings: false,
            images: false,
            decisions: false,
            tools: false,
            vision: false,
            json_schema: false,
            developer_role: false,
        }
    }

    #[must_use]
    pub const fn new(chat_completions: bool, stream: bool, embeddings: bool) -> Self {
        Self::with_dimensions(
            chat_completions,
            stream,
            embeddings,
            false,
            false,
            false,
            false,
        )
    }

    #[must_use]
    pub const fn with_dimensions(
        chat_completions: bool,
        stream: bool,
        embeddings: bool,
        tools: bool,
        vision: bool,
        json_schema: bool,
        developer_role: bool,
    ) -> Self {
        Self {
            chat_completions,
            responses: false,
            stream,
            embeddings,
            images: false,
            decisions: false,
            tools,
            vision,
            json_schema,
            developer_role,
        }
    }

    #[must_use]
    pub const fn chat_only_streaming() -> Self {
        Self::with_dimensions(true, true, false, false, true, false, true)
    }

    #[must_use]
    pub const fn openai_compat_baseline() -> Self {
        Self {
            chat_completions: true,
            responses: true,
            stream: true,
            embeddings: true,
            images: true,
            decisions: false,
            tools: true,
            vision: true,
            json_schema: true,
            developer_role: true,
        }
    }

    #[must_use]
    pub const fn anthropic_compat_baseline() -> Self {
        Self {
            chat_completions: true,
            responses: false,
            stream: true,
            embeddings: false,
            images: false,
            decisions: false,
            tools: true,
            vision: true,
            json_schema: false,
            developer_role: false,
        }
    }

    #[must_use]
    pub const fn aws_bedrock_baseline() -> Self {
        Self {
            chat_completions: true,
            responses: true,
            stream: true,
            embeddings: false,
            images: false,
            decisions: false,
            tools: true,
            vision: true,
            json_schema: true,
            developer_role: true,
        }
    }

    #[must_use]
    pub const fn gcp_vertex_baseline() -> Self {
        Self::with_dimensions(true, true, false, true, true, true, true)
    }

    #[must_use]
    pub const fn decisions_only() -> Self {
        Self {
            decisions: true,
            ..Self::none()
        }
    }

    /// Capabilities a provider type can serve before route-specific narrowing.
    #[must_use]
    pub fn for_provider_type(provider_type: &str) -> Self {
        match provider_type {
            "openai_compat" | "gcp_cloud_run_openai_compat" => Self::openai_compat_baseline(),
            "anthropic_compat" => Self::anthropic_compat_baseline(),
            "gcp_vertex" => Self::gcp_vertex_baseline(),
            "aws_bedrock" => Self::aws_bedrock_baseline(),
            "github_copilot" => Self::none(),
            "typesafe" => Self::decisions_only(),
            // Every config provider type has an entry above; a gateway config test fails when
            // a new type is missing, because this fallback would grant every capability.
            _ => Self::all_enabled(),
        }
    }

    #[must_use]
    pub const fn all_enabled() -> Self {
        Self {
            chat_completions: true,
            responses: true,
            stream: true,
            embeddings: true,
            images: true,
            decisions: true,
            tools: true,
            vision: true,
            json_schema: true,
            developer_role: true,
        }
    }

    #[must_use]
    pub const fn intersect(self, other: Self) -> Self {
        Self {
            chat_completions: self.chat_completions && other.chat_completions,
            responses: self.responses && other.responses,
            stream: self.stream && other.stream,
            embeddings: self.embeddings && other.embeddings,
            images: self.images && other.images,
            decisions: self.decisions && other.decisions,
            tools: self.tools && other.tools,
            vision: self.vision && other.vision,
            json_schema: self.json_schema && other.json_schema,
            developer_role: self.developer_role && other.developer_role,
        }
    }
}

impl Default for ProviderCapabilities {
    fn default() -> Self {
        Self::all_enabled()
    }
}

const fn default_true() -> bool {
    true
}

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("invalid request for provider: {0}")]
    InvalidRequest(String),
    #[error("upstream provider timed out")]
    Timeout,
    #[error("upstream provider transport failure: {0}")]
    Transport(String),
    #[error("upstream provider returned {status}: {body}")]
    UpstreamHttp { status: u16, body: String },
    #[error("upstream provider returned {status}: {body}")]
    NativeMessagesHttp {
        status: u16,
        body: String,
        headers: std::collections::BTreeMap<String, String>,
    },
    #[error("provider call failed after billable upstream usage: {source}")]
    PartialUsage {
        source: Box<ProviderError>,
        provider_usage: Option<Value>,
    },
    #[error("provider is not implemented: {0}")]
    NotImplemented(String),
}

impl ProviderError {
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::PartialUsage { source, .. } => source.is_retryable(),
            Self::NativeMessagesHttp {
                status, headers, ..
            } => match headers.get("x-should-retry").map(String::as_str) {
                Some("true") => true,
                Some("false") => false,
                _ => matches!(status, 408 | 429 | 500..=599),
            },
            _ => matches!(
                self,
                Self::Timeout
                    | Self::Transport(_)
                    | Self::UpstreamHttp {
                        status: 408 | 429 | 500..=599,
                        ..
                    }
            ),
        }
    }
}

#[async_trait]
pub trait ProviderUserTokenResolver: Send + Sync {
    async fn resolve_provider_user_token(
        &self,
        provider_key: &str,
        user_id: Uuid,
        expected_credential_id: Option<Uuid>,
    ) -> Result<String, ProviderError>;
}

pub type ProviderStream = Pin<Box<dyn Stream<Item = Result<Bytes, ProviderError>> + Send>>;

pub struct NativeMessagesResponse<T> {
    pub body: T,
    pub headers: std::collections::BTreeMap<String, String>,
}

#[async_trait]
pub trait ProviderClient: Send + Sync {
    fn provider_key(&self) -> &str;
    fn provider_type(&self) -> &str;
    fn capabilities(&self) -> ProviderCapabilities;
    /// Tiers this adapter can send for `route`. Routing skips a route that cannot serve
    /// the effective tier, and adapters reject one they are handed anyway.
    fn service_tiers(&self, _route: &ModelRoute) -> &'static [ServiceTier] {
        &[ServiceTier::Default]
    }
    /// Whether this route can carry Anthropic Messages without translating the response.
    fn supports_native_messages(&self, _context: &ProviderRequestContext) -> bool {
        false
    }

    /// A native Message, including content order, usage and provider extensions.
    async fn messages(
        &self,
        _request: &ChatRequest,
        _context: &ProviderRequestContext,
    ) -> Result<NativeMessagesResponse<Value>, ProviderError> {
        Err(ProviderError::NotImplemented(format!(
            "{} does not support native Messages",
            self.provider_type()
        )))
    }

    /// Anthropic SSE bytes. Unknown events and content blocks must survive this boundary.
    async fn messages_stream(
        &self,
        _request: &ChatRequest,
        _context: &ProviderRequestContext,
    ) -> Result<NativeMessagesResponse<ProviderStream>, ProviderError> {
        Err(ProviderError::NotImplemented(format!(
            "{} does not support native Messages streaming",
            self.provider_type()
        )))
    }
    fn batch_capabilities(&self) -> BatchCapabilities {
        BatchCapabilities::NONE
    }

    async fn submit_batch(&self, _request: &ProviderBatchRequest) -> ProviderBatchSubmission {
        ProviderBatchSubmission::NotSubmitted(ProviderError::NotImplemented(format!(
            "{} does not support batch submission",
            self.provider_type()
        )))
    }

    async fn inspect_batch(
        &self,
        _provider_batch_id: &str,
        _context: &ProviderRequestContext,
    ) -> Result<ProviderBatchState, ProviderError> {
        Err(ProviderError::NotImplemented(format!(
            "{} does not support batch inspection",
            self.provider_type()
        )))
    }

    async fn cancel_batch(
        &self,
        _provider_batch_id: &str,
        _context: &ProviderRequestContext,
    ) -> Result<ProviderBatchState, ProviderError> {
        Err(ProviderError::NotImplemented(format!(
            "{} does not support batch cancellation",
            self.provider_type()
        )))
    }

    async fn batch_results(
        &self,
        _state: &ProviderBatchState,
        _context: &ProviderRequestContext,
    ) -> Result<Vec<ProviderBatchResult>, ProviderError> {
        Err(ProviderError::NotImplemented(format!(
            "{} does not support batch result retrieval",
            self.provider_type()
        )))
    }

    async fn chat_completions(
        &self,
        request: &ChatRequest,
        context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError>;

    async fn chat_completions_stream(
        &self,
        request: &ChatRequest,
        context: &ProviderRequestContext,
    ) -> Result<ProviderStream, ProviderError>;

    async fn embeddings(
        &self,
        request: &EmbeddingsRequest,
        context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError>;

    async fn responses(
        &self,
        request: &ResponsesRequest,
        context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError>;

    async fn responses_stream(
        &self,
        request: &ResponsesRequest,
        context: &ProviderRequestContext,
    ) -> Result<ProviderStream, ProviderError>;

    /// Count the input tokens of a Messages-shaped request without generating output.
    ///
    /// Returns `{"input_tokens": N}`. Providers without an upstream token-count API
    /// keep this default, so callers get a typed error instead of an estimate.
    async fn count_tokens(
        &self,
        request: &ChatRequest,
        context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        let _ = (request, context);
        Err(ProviderError::NotImplemented(format!(
            "{} does not support token counting",
            self.provider_type()
        )))
    }

    /// Generate images through an Images API upstream. Returns the upstream
    /// `{created, data: [{b64_json, media_type}], usage}` body as JSON.
    async fn images(
        &self,
        request: &ImagesRequest,
        context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        let _ = (request, context);
        Err(ProviderError::NotImplemented(format!(
            "{} does not support image generation",
            self.provider_type()
        )))
    }

    async fn decisions(
        &self,
        request: &DecisionsRequest,
        context: &ProviderRequestContext,
    ) -> Result<ProviderDecisionsResponse, ProviderError> {
        let _ = (request, context);
        Err(ProviderError::NotImplemented(format!(
            "{} does not support decisions",
            self.provider_type()
        )))
    }
}

#[derive(Default, Clone)]
pub struct ProviderRegistry {
    providers: HashMap<String, RegisteredProvider>,
}

#[derive(Clone)]
struct RegisteredProvider {
    client: Arc<dyn ProviderClient>,
    routing_identity: Option<String>,
}

impl ProviderRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            providers: HashMap::new(),
        }
    }

    pub fn register(&mut self, provider: Arc<dyn ProviderClient>) {
        self.providers.insert(
            provider.provider_key().to_string(),
            RegisteredProvider {
                client: provider,
                routing_identity: None,
            },
        );
    }

    /// Registers the initialized adapter with an opaque account and endpoint identity.
    /// The caller must hash sensitive credential material before registration.
    pub fn register_with_routing_identity(
        &mut self,
        provider: Arc<dyn ProviderClient>,
        routing_identity: String,
    ) {
        self.providers.insert(
            provider.provider_key().to_string(),
            RegisteredProvider {
                client: provider,
                routing_identity: Some(routing_identity),
            },
        );
    }

    #[must_use]
    pub fn routing_identity(&self, provider_key: &str) -> Option<&str> {
        self.providers
            .get(provider_key)
            .and_then(|provider| provider.routing_identity.as_deref())
    }

    #[must_use]
    pub fn get(&self, provider_key: &str) -> Option<Arc<dyn ProviderClient>> {
        self.providers
            .get(provider_key)
            .map(|provider| provider.client.clone())
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.providers.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }
}
