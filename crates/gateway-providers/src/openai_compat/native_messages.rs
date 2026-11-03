//! OpenRouter Claude routes share its Anthropic endpoint for both public chat APIs.
//! Keep OpenAI-only compatibility transforms away from native Messages bodies.
use super::*;
use crate::anthropic::{AnthropicRequestOptions, map_anthropic_request};

impl OpenAiCompatProvider {
    pub(super) fn uses_native_messages(&self, context: &ProviderRequestContext) -> bool {
        let claude =
            gateway_core::claude_models::is_openrouter_claude_model(&context.upstream_model);
        let openrouter = context.compatibility.openrouter.is_some()
            || url::Url::parse(&self.config.base_url).is_ok_and(|url| {
                url.host_str() == Some("openrouter.ai")
                    && url.path().trim_end_matches('/') == "/api/v1"
            });
        claude && openrouter
    }

    pub(super) fn build_native_messages_request(
        &self,
        request: &CoreChatRequest,
        context: &ProviderRequestContext,
        stream: bool,
        token: Option<&str>,
    ) -> Result<reqwest::Request, ProviderError> {
        let mut body = map_anthropic_request(
            request,
            context,
            stream,
            &AnthropicRequestOptions {
                default_headers: Some(&self.config.default_headers),
                ..Default::default()
            },
        )?;
        let betas = crate::anthropic::beta::take_request_betas(
            &mut body,
            context,
            &self.config.default_headers,
        )?;
        apply_openrouter_routing_policy(&mut body, context)?;
        let mut builder = self
            .client
            .post(join_base_url(&self.config.base_url, "messages")?)
            .json(&body);
        for (name, value) in &self.config.default_headers {
            if !name.eq_ignore_ascii_case("anthropic-beta") {
                builder = builder.header(name, value);
            }
        }
        builder = crate::anthropic::native::request_headers(builder, context);
        for (name, value) in &context.extra_headers {
            if !name.eq_ignore_ascii_case("anthropic-beta")
                && let Some(value) = value.as_str()
            {
                builder = builder.header(name, value);
            }
        }
        if !betas.is_empty() {
            builder = builder.header("anthropic-beta", betas.join(","));
        }
        if let Some(token) = token {
            builder = self.config.bearer_auth_header.apply(builder, token);
        }
        builder
            .header("x-request-id", &context.request_id)
            .build()
            .map_err(map_reqwest_error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(model: &str) -> ProviderRequestContext {
        ProviderRequestContext {
            request_id: "native-request".into(),
            model_key: "claude".into(),
            provider_key: "openrouter".into(),
            upstream_model: model.into(),
            owner_user_id: None,
            expected_provider_credential_id: None,
            chat_protocol: gateway_core::ChatRequestProtocol::AnthropicMessages,
            extra_headers: Default::default(),
            extra_body: Default::default(),
            request_headers: Default::default(),
            compatibility: Default::default(),
        }
    }

    #[test]
    fn openrouter_prefers_messages_for_claude_on_both_public_apis() {
        let provider = OpenAiCompatProvider::new(OpenAiCompatConfig::new(
            "openrouter".into(),
            "https://openrouter.ai/api/v1".into(),
        ))
        .unwrap();
        for model in ["~anthropic/claude-opus-latest", "anthropic/claude-opus-5.5"] {
            for protocol in [
                gateway_core::ChatRequestProtocol::AnthropicMessages,
                gateway_core::ChatRequestProtocol::ChatCompletions,
            ] {
                let mut context = context(model);
                context.chat_protocol = protocol;
                context.compatibility.openrouter = Some(
                    serde_json::from_value(json!({"provider":{"zdr":true,"only":["anthropic"]}}))
                        .unwrap(),
                );
                context.extra_body = serde_json::from_value(json!({"model":"must-not-override","stream":false,"provider":{"only":["must-not-override"]}})).unwrap();
                let request = serde_json::from_value(json!({"model":"claude","max_tokens":1024,"messages":[{"role":"user","content":"hello"}],"reasoning_effort":"high"})).unwrap();
                let wire = provider
                    .build_chat_stream_request(&request, &context)
                    .unwrap();
                assert_eq!(wire.url().as_str(), "https://openrouter.ai/api/v1/messages");
                let body: Value =
                    serde_json::from_slice(wire.body().unwrap().as_bytes().unwrap()).unwrap();
                assert_eq!(body["model"], model);
                assert_eq!(body["stream"], true);
                assert_eq!(body["thinking"], json!({"type":"adaptive"}));
                assert_eq!(body["output_config"]["effort"], "high");
                assert_eq!(body["provider"], json!({"zdr":true,"only":["anthropic"]}));
                assert!(body.get("stream_options").is_none());
            }
        }
        assert!(!provider.uses_native_messages(&context("openai/gpt-4o-mini")));
        let generic = OpenAiCompatProvider::new(OpenAiCompatConfig::new(
            "generic".into(),
            "https://example.test/v1".into(),
        ))
        .unwrap();
        assert!(!generic.uses_native_messages(&context("anthropic/claude-opus-5.5")));
    }

    #[test]
    fn native_openrouter_costs_survive_chat_completions_translation() {
        let upstream = json!({"content":[{"type":"text","text":"ok"}],"usage":{
            "input_tokens":2,"output_tokens":1,"cost":"0.001","is_byok":true,
            "cost_details":{"upstream_inference_cost":"0.002"}
        }});
        let translated = crate::anthropic::normalize_anthropic_response(
            &upstream,
            &context("anthropic/claude-opus-5.5"),
            "anthropic_compat",
            "openrouter",
        );
        for field in ["cost", "is_byok", "cost_details"] {
            assert_eq!(translated["usage"][field], upstream["usage"][field]);
        }
    }
}
