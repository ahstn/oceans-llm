use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, SystemTime},
};

use async_stream::stream;
use async_trait::async_trait;
use aws_config::{Region, default_provider::credentials::DefaultCredentialsChain};
use aws_credential_types::{Credentials, provider::ProvideCredentials};
use aws_sigv4::{
    http_request::{SignableBody, SignableRequest, SigningSettings, sign},
    sign::v4,
};
use aws_smithy_runtime_api::client::identity::Identity;
use bytes::{Buf, Bytes};
use futures_util::StreamExt;
use gateway_core::{
    AwsBedrockApiStyle, AwsBedrockRouteCompatibility, CoreChatRequest, CoreEmbeddingsRequest,
    CoreResponsesRequest, ProviderCapabilities, ProviderClient, ProviderError,
    ProviderRequestContext, ProviderStream, SseEventParser, core_chat_request_to_openai,
    core_responses_request_to_openai,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use time::OffsetDateTime;
use tokio::sync::OnceCell;
use url::Url;
use uuid::Uuid;

use crate::http::{join_base_url, map_reqwest_error};
use crate::openai_compat::{
    normalize_openai_compat_responses_stream, normalize_openai_compat_stream,
};
use crate::streaming::{done_sse_chunk, openai_sse_error_chunk, render_sse_event_chunk};

#[derive(Debug, Clone)]
pub enum BedrockAuthConfig {
    DefaultChain,
    Bearer {
        token: String,
    },
    StaticCredentials {
        access_key_id: String,
        secret_access_key: String,
        session_token: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BedrockEndpointKind {
    BedrockRuntime,
    BedrockMantle,
}

impl BedrockEndpointKind {
    #[must_use]
    pub const fn signing_service_name(self) -> &'static str {
        match self {
            Self::BedrockRuntime => "bedrock",
            Self::BedrockMantle => "bedrock-mantle",
        }
    }

    #[must_use]
    pub const fn as_config_value(self) -> &'static str {
        match self {
            Self::BedrockRuntime => "bedrock_runtime",
            Self::BedrockMantle => "bedrock_mantle",
        }
    }
}

#[derive(Debug, Clone)]
pub struct BedrockProviderConfig {
    pub provider_key: String,
    pub region: String,
    pub endpoint_kind: BedrockEndpointKind,
    pub endpoint_url: String,
    pub auth: BedrockAuthConfig,
    pub default_headers: BTreeMap<String, String>,
    pub request_timeout_ms: u64,
}

impl BedrockProviderConfig {
    #[must_use]
    pub fn default_endpoint_url(endpoint_kind: BedrockEndpointKind, region: &str) -> String {
        match endpoint_kind {
            BedrockEndpointKind::BedrockRuntime => {
                format!("https://bedrock-runtime.{region}.amazonaws.com")
            }
            BedrockEndpointKind::BedrockMantle => {
                format!("https://bedrock-mantle.{region}.api.aws")
            }
        }
    }

    pub fn resolved_endpoint_url(
        endpoint_kind: BedrockEndpointKind,
        region: &str,
        endpoint_url: Option<&str>,
    ) -> Result<String, url::ParseError> {
        let url = match endpoint_url {
            Some(endpoint_url) => Url::parse(endpoint_url)?,
            None => Url::parse(&Self::default_endpoint_url(endpoint_kind, region))?,
        };

        Ok(url.to_string().trim_end_matches('/').to_string())
    }
}

#[derive(Clone)]
pub struct BedrockProvider {
    config: BedrockProviderConfig,
    client: reqwest::Client,
    default_credentials_chain: Arc<OnceCell<DefaultCredentialsChain>>,
}

impl BedrockProvider {
    pub fn new(config: BedrockProviderConfig) -> Result<Self, ProviderError> {
        let _ = Url::parse(&config.endpoint_url).map_err(|error| {
            ProviderError::InvalidRequest(format!(
                "aws_bedrock provider `{}` endpoint_url is invalid: {error}",
                config.provider_key
            ))
        })?;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(config.request_timeout_ms))
            .build()
            .map_err(map_reqwest_error)?;

        Ok(Self {
            config,
            client,
            default_credentials_chain: Arc::new(OnceCell::new()),
        })
    }

    fn unsupported(method: &str) -> ProviderError {
        ProviderError::NotImplemented(format!(
            "aws_bedrock {method} execution is not implemented yet"
        ))
    }

    fn route_compatibility<'a>(
        &self,
        context: &'a ProviderRequestContext,
    ) -> Result<&'a AwsBedrockRouteCompatibility, ProviderError> {
        let compatibility = context.compatibility.aws_bedrock.as_ref().ok_or_else(|| {
            ProviderError::InvalidRequest(
                "aws_bedrock routes require compatibility.aws_bedrock.api_style".to_string(),
            )
        })?;
        self.validate_endpoint_api_style(compatibility.api_style)?;
        Ok(compatibility)
    }

    fn api_style(
        &self,
        context: &ProviderRequestContext,
    ) -> Result<AwsBedrockApiStyle, ProviderError> {
        Ok(self.route_compatibility(context)?.api_style)
    }

    fn validate_endpoint_api_style(
        &self,
        api_style: AwsBedrockApiStyle,
    ) -> Result<(), ProviderError> {
        let compatible = match self.config.endpoint_kind {
            BedrockEndpointKind::BedrockRuntime => api_style.is_runtime(),
            BedrockEndpointKind::BedrockMantle => api_style.is_mantle(),
        };
        if compatible {
            return Ok(());
        }

        Err(ProviderError::InvalidRequest(format!(
            "aws_bedrock api_style `{:?}` is not compatible with endpoint_kind `{}`",
            api_style,
            self.config.endpoint_kind.as_config_value()
        )))
    }

    fn openai_endpoint(
        &self,
        context: &ProviderRequestContext,
        endpoint_suffix: &str,
    ) -> Result<String, ProviderError> {
        let compatibility = self.route_compatibility(context)?;
        let base_path = compatibility
            .openai_base_path
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                ProviderError::InvalidRequest(
                    "aws_bedrock OpenAI-shaped api_style routes require compatibility.aws_bedrock.openai_base_path"
                        .to_string(),
                )
            })?;
        let base_url = join_base_url(&self.config.endpoint_url, base_path.trim_start_matches('/'))?;
        join_base_url(&base_url, endpoint_suffix)
    }

    fn converse_endpoint(&self, upstream_model: &str) -> String {
        self.bedrock_model_endpoint(upstream_model, "converse")
    }

    fn converse_stream_endpoint(&self, upstream_model: &str) -> String {
        self.bedrock_model_endpoint(upstream_model, "converse-stream")
    }

    fn bedrock_model_endpoint(&self, upstream_model: &str, operation: &str) -> String {
        let encoded_model_id: String =
            url::form_urlencoded::byte_serialize(upstream_model.as_bytes()).collect();
        format!(
            "{}/model/{encoded_model_id}/{operation}",
            self.config.endpoint_url
        )
    }

    fn invoke_endpoint(&self, upstream_model: &str) -> String {
        let encoded_model_id: String =
            url::form_urlencoded::byte_serialize(upstream_model.as_bytes()).collect();
        format!(
            "{}/model/{encoded_model_id}/invoke",
            self.config.endpoint_url
        )
    }

    async fn build_chat_request(
        &self,
        request: &CoreChatRequest,
        context: &ProviderRequestContext,
    ) -> Result<reqwest::Request, ProviderError> {
        let api_style = self.api_style(context)?;
        let (body, url, extra_headers) = match api_style {
            AwsBedrockApiStyle::RuntimeConverse => (
                map_chat_request_to_converse(request, context)?,
                self.converse_endpoint(&context.upstream_model),
                Vec::new(),
            ),
            AwsBedrockApiStyle::RuntimeAnthropicInvoke => (
                map_chat_request_to_anthropic_messages(
                    request,
                    context,
                    AnthropicMessagesTarget::RuntimeInvoke,
                )?,
                self.invoke_endpoint(&context.upstream_model),
                Vec::new(),
            ),
            AwsBedrockApiStyle::RuntimeOpenaiChat | AwsBedrockApiStyle::MantleOpenaiChat => (
                map_openai_chat_request(request, context, false)?,
                self.openai_endpoint(context, "chat/completions")?,
                Vec::new(),
            ),
            AwsBedrockApiStyle::MantleAnthropicMessages => (
                map_chat_request_to_anthropic_messages(
                    request,
                    context,
                    AnthropicMessagesTarget::MantleMessages,
                )?,
                join_base_url(&self.config.endpoint_url, "anthropic/v1/messages")?,
                vec![("anthropic-version", "2023-06-01")],
            ),
            AwsBedrockApiStyle::MantleOpenaiResponses => {
                return Err(ProviderError::InvalidRequest(
                    "aws_bedrock api_style `mantle_openai_responses` does not support Chat Completions"
                        .to_string(),
                ));
            }
        };

        self.build_json_request(
            url,
            body,
            context,
            "application/json",
            extra_headers,
            api_style,
        )
        .await
    }

    async fn build_converse_stream_request(
        &self,
        request: &CoreChatRequest,
        context: &ProviderRequestContext,
    ) -> Result<reqwest::Request, ProviderError> {
        let api_style = self.api_style(context)?;
        if api_style != AwsBedrockApiStyle::RuntimeConverse {
            return Err(ProviderError::InvalidRequest(
                "aws_bedrock ConverseStream requires api_style `runtime_converse`".to_string(),
            ));
        }
        let mut stream_request = request.clone();
        stream_request.stream = true;
        let body = map_chat_request_to_converse(&stream_request, context)?;
        let url = self.converse_stream_endpoint(&context.upstream_model);

        self.build_json_request(
            url,
            body,
            context,
            "application/vnd.amazon.eventstream",
            Vec::new(),
            api_style,
        )
        .await
    }

    async fn build_chat_stream_request(
        &self,
        request: &CoreChatRequest,
        context: &ProviderRequestContext,
    ) -> Result<reqwest::Request, ProviderError> {
        let api_style = self.api_style(context)?;
        match api_style {
            AwsBedrockApiStyle::RuntimeConverse => {
                self.build_converse_stream_request(request, context).await
            }
            AwsBedrockApiStyle::RuntimeOpenaiChat | AwsBedrockApiStyle::MantleOpenaiChat => {
                self.build_json_request(
                    self.openai_endpoint(context, "chat/completions")?,
                    map_openai_chat_request(request, context, true)?,
                    context,
                    "text/event-stream",
                    Vec::new(),
                    api_style,
                )
                .await
            }
            AwsBedrockApiStyle::MantleAnthropicMessages => {
                let mut stream_request = request.clone();
                stream_request.stream = true;
                self.build_json_request(
                    join_base_url(&self.config.endpoint_url, "anthropic/v1/messages")?,
                    map_chat_request_to_anthropic_messages(
                        &stream_request,
                        context,
                        AnthropicMessagesTarget::MantleMessages,
                    )?,
                    context,
                    "text/event-stream",
                    vec![("anthropic-version", "2023-06-01")],
                    api_style,
                )
                .await
            }
            AwsBedrockApiStyle::RuntimeAnthropicInvoke => Err(ProviderError::InvalidRequest(
                "aws_bedrock api_style `runtime_anthropic_invoke` does not support streaming"
                    .to_string(),
            )),
            AwsBedrockApiStyle::MantleOpenaiResponses => Err(ProviderError::InvalidRequest(
                "aws_bedrock api_style `mantle_openai_responses` does not support Chat Completions streaming"
                    .to_string(),
            )),
        }
    }

    async fn build_responses_request(
        &self,
        request: &CoreResponsesRequest,
        context: &ProviderRequestContext,
        stream: bool,
    ) -> Result<reqwest::Request, ProviderError> {
        let api_style = self.api_style(context)?;
        if api_style != AwsBedrockApiStyle::MantleOpenaiResponses {
            return Err(ProviderError::InvalidRequest(format!(
                "aws_bedrock responses require api_style `mantle_openai_responses`, got `{:?}`",
                api_style
            )));
        }

        self.build_json_request(
            self.openai_endpoint(context, "responses")?,
            map_openai_responses_request(request, context, stream)?,
            context,
            if stream {
                "text/event-stream"
            } else {
                "application/json"
            },
            Vec::new(),
            api_style,
        )
        .await
    }

    async fn build_json_request(
        &self,
        url: String,
        body: Value,
        context: &ProviderRequestContext,
        accept: &'static str,
        extra_headers: Vec<(&'static str, &'static str)>,
        api_style: AwsBedrockApiStyle,
    ) -> Result<reqwest::Request, ProviderError> {
        let body = serde_json::to_vec(&body).map_err(|error| {
            ProviderError::InvalidRequest(format!(
                "failed to serialize aws_bedrock request: {error}"
            ))
        })?;
        let mut request = self.client.post(url).body(body);
        request = request.header("content-type", "application/json");
        request = request.header("accept", accept);

        for (name, value) in &self.config.default_headers {
            request = request.header(name, value);
        }

        for (name, value) in &context.extra_headers {
            if let Some(value) = value.as_str() {
                request = request.header(name, value);
            }
        }

        request = request.header("x-request-id", &context.request_id);

        for (name, value) in extra_headers {
            request = request.header(name, value);
        }

        self.apply_auth(request.build().map_err(map_reqwest_error)?, api_style)
            .await
    }

    async fn apply_auth(
        &self,
        mut request: reqwest::Request,
        api_style: AwsBedrockApiStyle,
    ) -> Result<reqwest::Request, ProviderError> {
        match &self.config.auth {
            BedrockAuthConfig::Bearer { token } => {
                request.headers_mut().remove(reqwest::header::AUTHORIZATION);
                request.headers_mut().remove("x-api-key");
                if api_style == AwsBedrockApiStyle::MantleAnthropicMessages {
                    let auth_value =
                        reqwest::header::HeaderValue::from_str(token).map_err(|error| {
                            ProviderError::InvalidRequest(format!(
                                "aws_bedrock api key cannot be used as a header: {error}"
                            ))
                        })?;
                    request.headers_mut().insert("x-api-key", auth_value);
                } else {
                    let auth_value =
                        reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))
                            .map_err(|error| {
                                ProviderError::InvalidRequest(format!(
                                    "aws_bedrock bearer token cannot be used as a header: {error}"
                                ))
                            })?;
                    request
                        .headers_mut()
                        .insert(reqwest::header::AUTHORIZATION, auth_value);
                }
                Ok(request)
            }
            BedrockAuthConfig::DefaultChain => {
                let provider = self.default_credentials_provider().await;
                let credentials = provider.provide_credentials().await.map_err(|error| {
                    ProviderError::Transport(format!(
                        "failed to resolve aws_bedrock default credentials: {error}"
                    ))
                })?;
                self.sign_request(request, credentials)
            }
            BedrockAuthConfig::StaticCredentials {
                access_key_id,
                secret_access_key,
                session_token,
            } => self.sign_request(
                request,
                Credentials::new(
                    access_key_id,
                    secret_access_key,
                    session_token.clone(),
                    None,
                    "oceans-llm-static-bedrock-credentials",
                ),
            ),
        }
    }

    async fn default_credentials_provider(&self) -> &DefaultCredentialsChain {
        let region = self.config.region.clone();
        self.default_credentials_chain
            .get_or_init(|| async move {
                DefaultCredentialsChain::builder()
                    .region(Region::new(region))
                    .build()
                    .await
            })
            .await
    }

    fn sign_request(
        &self,
        mut request: reqwest::Request,
        credentials: Credentials,
    ) -> Result<reqwest::Request, ProviderError> {
        request.headers_mut().remove(reqwest::header::AUTHORIZATION);
        request.headers_mut().remove("x-api-key");
        request.headers_mut().remove("x-amz-date");
        request.headers_mut().remove("x-amz-security-token");

        let method = request.method().as_str().to_string();
        let uri = request.url().as_str().to_string();
        let body = request
            .body()
            .and_then(reqwest::Body::as_bytes)
            .ok_or_else(|| {
                ProviderError::InvalidRequest(
                    "aws_bedrock SigV4 signing requires an in-memory request body".to_string(),
                )
            })?
            .to_vec();

        let headers = request
            .headers()
            .iter()
            .map(|(name, value)| {
                value
                    .to_str()
                    .map(|value| (name.as_str(), value))
                    .map_err(|error| {
                        ProviderError::InvalidRequest(format!(
                            "aws_bedrock request header `{name}` cannot be signed: {error}"
                        ))
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let identity: Identity = credentials.into();
        let signing_params = v4::SigningParams::builder()
            .identity(&identity)
            .region(&self.config.region)
            .name(self.config.endpoint_kind.signing_service_name())
            .time(SystemTime::now())
            .settings(SigningSettings::default())
            .build()
            .map_err(|error| {
                ProviderError::Transport(format!(
                    "failed to build aws_bedrock SigV4 signing parameters: {error}"
                ))
            })?
            .into();
        let signable_request = SignableRequest::new(
            method.as_str(),
            uri.as_str(),
            headers.iter().copied(),
            SignableBody::Bytes(&body),
        )
        .map_err(|error| {
            ProviderError::Transport(format!(
                "failed to construct aws_bedrock SigV4 canonical request: {error}"
            ))
        })?;

        let (signing_instructions, _signature) = sign(signable_request, &signing_params)
            .map_err(|error| {
                ProviderError::Transport(format!("failed to sign aws_bedrock request: {error}"))
            })?
            .into_parts();
        for header in signing_instructions.headers() {
            let value = reqwest::header::HeaderValue::from_str(header.1).map_err(|error| {
                ProviderError::Transport(format!(
                    "aws_bedrock SigV4 signer produced invalid header `{}`: {error}",
                    header.0
                ))
            })?;
            request.headers_mut().insert(
                reqwest::header::HeaderName::from_bytes(header.0.as_bytes()).map_err(|error| {
                    ProviderError::Transport(format!(
                        "aws_bedrock SigV4 signer produced invalid header name `{}`: {error}",
                        header.0
                    ))
                })?,
                value,
            );
        }

        Ok(request)
    }

    async fn execute_stream_request(
        &self,
        request: reqwest::Request,
    ) -> Result<reqwest::Response, ProviderError> {
        let response = self
            .client
            .execute(request)
            .await
            .map_err(map_reqwest_error)?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.map_err(map_reqwest_error)?;
            return Err(ProviderError::UpstreamHttp {
                status: status.as_u16(),
                body,
            });
        }

        Ok(response)
    }
}

#[async_trait]
impl ProviderClient for BedrockProvider {
    fn provider_key(&self) -> &str {
        &self.config.provider_key
    }

    fn provider_type(&self) -> &str {
        "aws_bedrock"
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            chat_completions: true,
            responses: true,
            stream: true,
            embeddings: false,
            tools: true,
            vision: true,
            json_schema: false,
            developer_role: true,
        }
    }

    async fn chat_completions(
        &self,
        request: &CoreChatRequest,
        context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        let api_style = self.api_style(context)?;
        let request = self.build_chat_request(request, context).await?;
        let response = self
            .client
            .execute(request)
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

        let value: Value = serde_json::from_str(&text).map_err(|error| {
            ProviderError::Transport(format!("invalid JSON from aws_bedrock chat: {error}"))
        })?;
        match api_style {
            AwsBedrockApiStyle::RuntimeConverse => Ok(normalize_converse_response(&value, context)),
            AwsBedrockApiStyle::RuntimeAnthropicInvoke
            | AwsBedrockApiStyle::MantleAnthropicMessages => {
                Ok(normalize_anthropic_messages_response(&value, context))
            }
            AwsBedrockApiStyle::RuntimeOpenaiChat | AwsBedrockApiStyle::MantleOpenaiChat => {
                Ok(value)
            }
            AwsBedrockApiStyle::MantleOpenaiResponses => Err(ProviderError::InvalidRequest(
                "aws_bedrock api_style `mantle_openai_responses` does not support Chat Completions"
                    .to_string(),
            )),
        }
    }

    async fn chat_completions_stream(
        &self,
        request: &CoreChatRequest,
        context: &ProviderRequestContext,
    ) -> Result<ProviderStream, ProviderError> {
        let api_style = self.api_style(context)?;
        let request = self.build_chat_stream_request(request, context).await?;
        let response = self.execute_stream_request(request).await?;

        match api_style {
            AwsBedrockApiStyle::RuntimeConverse => Ok(normalize_bedrock_converse_stream(
                response.bytes_stream(),
                context.clone(),
            )),
            AwsBedrockApiStyle::RuntimeOpenaiChat | AwsBedrockApiStyle::MantleOpenaiChat => {
                Ok(normalize_openai_compat_stream(response.bytes_stream()))
            }
            AwsBedrockApiStyle::MantleAnthropicMessages => Ok(normalize_anthropic_messages_stream(
                response.bytes_stream(),
                context.clone(),
            )),
            AwsBedrockApiStyle::RuntimeAnthropicInvoke
            | AwsBedrockApiStyle::MantleOpenaiResponses => {
                Err(ProviderError::InvalidRequest(format!(
                    "aws_bedrock api_style `{:?}` does not support Chat Completions streaming",
                    api_style
                )))
            }
        }
    }

    async fn embeddings(
        &self,
        _request: &CoreEmbeddingsRequest,
        _context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        Err(Self::unsupported("embeddings"))
    }

    async fn responses(
        &self,
        request: &CoreResponsesRequest,
        context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        let request = self
            .build_responses_request(request, context, false)
            .await?;
        let response = self
            .client
            .execute(request)
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

        serde_json::from_str(&text).map_err(|error| {
            ProviderError::Transport(format!("invalid JSON from aws_bedrock responses: {error}"))
        })
    }

    async fn responses_stream(
        &self,
        request: &CoreResponsesRequest,
        context: &ProviderRequestContext,
    ) -> Result<ProviderStream, ProviderError> {
        let request = self.build_responses_request(request, context, true).await?;
        let response = self.execute_stream_request(request).await?;

        Ok(normalize_openai_compat_responses_stream(
            response.bytes_stream(),
        ))
    }
}

mod eventstream;
mod request;
mod response;

use eventstream::*;
use request::*;
use response::*;

#[cfg(test)]
mod tests;
