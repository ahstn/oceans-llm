//! OpenRouter Claude routes share its Anthropic endpoint for both public chat APIs.
//! Keep OpenAI-only compatibility transforms away from native Messages bodies.
use super::*;
use crate::anthropic::{AnthropicRequestOptions, map_anthropic_request};

impl OpenAiCompatProvider {
    pub(super) fn uses_native_messages(&self, context: &ProviderRequestContext) -> bool {
        gateway_core::route_capabilities::openrouter_uses_native_messages(
            &context.upstream_model,
            context.compatibility.openrouter.is_some(),
            Some(&self.config.base_url),
        )
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
                allow_remote_images: true,
                ..Default::default()
            },
        )?;
        if context.chat_protocol == gateway_core::ChatRequestProtocol::ChatCompletions {
            translate_openrouter_response_format(&mut body)?;
        }
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

fn translate_openrouter_response_format(body: &mut Value) -> Result<(), ProviderError> {
    if body["response_format"]["type"] != "json_schema" {
        return Ok(());
    }
    let schema = body["response_format"]["json_schema"]["schema"]
        .as_object()
        .ok_or_else(|| {
            ProviderError::InvalidRequest(
                "`response_format.json_schema.schema` must be an object".into(),
            )
        })?;
    let format = json!({"type": "json_schema", "schema": schema});
    let output = body
        .as_object_mut()
        .expect("mapped Messages body is an object")
        .entry("output_config")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| ProviderError::InvalidRequest("`output_config` must be an object".into()))?;
    if let Some(native) = output.get("format")
        && !native.is_null()
        && native != &format
    {
        return Err(ProviderError::InvalidRequest(
            "`response_format` conflicts with `output_config.format`".into(),
        ));
    }
    output.insert("format".into(), format);
    body.as_object_mut().unwrap().remove("response_format");
    Ok(())
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
    fn openrouter_chat_json_schema_uses_native_output_format() {
        let provider = OpenAiCompatProvider::new(OpenAiCompatConfig::new(
            "openrouter".into(),
            "https://openrouter.ai/api/v1".into(),
        ))
        .unwrap();
        let mut context = context("anthropic/claude-sonnet-4.6");
        context.chat_protocol = gateway_core::ChatRequestProtocol::ChatCompletions;
        let schema = json!({
            "type": "object",
            "properties": {"answer": {"type": "string"}},
            "required": ["answer"],
            "additionalProperties": false
        });
        let format = json!({"type": "json_schema", "schema": schema});
        for output in [
            json!({"extension": true}),
            json!({"extension": true, "format": null}),
            json!({"extension": true, "format": format}),
        ] {
            let request = serde_json::from_value(json!({
                "model": "claude",
                "messages": [{"role": "user", "content": "answer"}],
                "response_format": {
                    "type": "json_schema",
                    "json_schema": {"name": "answer", "strict": true, "schema": schema}
                },
                "output_config": output
            }))
            .unwrap();
            for streaming in [false, true] {
                let wire = if streaming {
                    provider.build_chat_stream_request(&request, &context)
                } else {
                    provider.build_chat_request(&request, &context)
                }
                .unwrap();
                let body: Value =
                    serde_json::from_slice(wire.body().unwrap().as_bytes().unwrap()).unwrap();
                assert_eq!(wire.url().path(), "/api/v1/messages");
                assert_eq!(body["output_config"]["format"], format);
                assert_eq!(body["output_config"]["extension"], true);
                assert!(body.get("response_format").is_none());
            }
        }
    }

    #[test]
    fn openrouter_chat_json_schema_rejects_conflicting_native_format() {
        let provider = OpenAiCompatProvider::new(OpenAiCompatConfig::new(
            "openrouter".into(),
            "https://openrouter.ai/api/v1".into(),
        ))
        .unwrap();
        let mut context = context("anthropic/claude-sonnet-4.6");
        context.chat_protocol = gateway_core::ChatRequestProtocol::ChatCompletions;
        let request = serde_json::from_value(json!({
            "model": "claude", "messages": [{"role": "user", "content": "answer"}],
            "response_format": {"type": "json_schema", "json_schema": {"schema": {"type": "object"}}},
            "output_config": {"format": {"type": "json_schema", "schema": {"type": "string"}}}
        }))
        .unwrap();
        for streaming in [false, true] {
            let error = if streaming {
                provider.build_chat_stream_request(&request, &context)
            } else {
                provider.build_chat_request(&request, &context)
            }
            .unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("conflicts with `output_config.format`")
            );
        }
    }

    #[test]
    fn openrouter_native_messages_preserve_output_config() {
        let provider = OpenAiCompatProvider::new(OpenAiCompatConfig::new(
            "openrouter".into(),
            "https://openrouter.ai/api/v1".into(),
        ))
        .unwrap();
        let context = context("~anthropic/claude-opus-latest");
        let output = json!({
            "format": {"type": "json_schema", "schema": {"type": "object"}},
            "extension": {"future": true}
        });
        let request = serde_json::from_value(json!({
            "model": "claude", "messages": [{"role": "user", "content": "answer"}],
            "output_config": output
        }))
        .unwrap();
        for streaming in [false, true] {
            let wire = provider
                .build_native_messages_request(&request, &context, streaming, None)
                .unwrap();
            let body: Value =
                serde_json::from_slice(wire.body().unwrap().as_bytes().unwrap()).unwrap();
            assert_eq!(body["output_config"], output);
            assert!(body.get("response_format").is_none());
        }
    }

    #[test]
    fn openrouter_native_route_classification_preserves_endpoint_rules() {
        for (base_url, compat, model, native) in [
            (
                "https://openrouter.ai/api/v1",
                false,
                "~anthropic/claude-opus-latest",
                true,
            ),
            (
                "https://openrouter.ai/api/v1/",
                false,
                "anthropic/claude-opus-5.5",
                true,
            ),
            (
                "https://openrouter.ai/api/v1///",
                false,
                "anthropic/claude-sonnet-latest",
                true,
            ),
            (
                "https://openrouter.ai/api",
                false,
                "anthropic/claude-opus-5.5",
                false,
            ),
            (
                "https://example.test/v1",
                false,
                "anthropic/claude-opus-5.5",
                false,
            ),
            (
                "https://example.test/v1",
                true,
                "anthropic/claude-opus-5.5",
                true,
            ),
            ("https://openrouter.ai/api/v1", true, "openai/gpt-4o", false),
            (
                "https://openrouter.ai/api/v1",
                false,
                "claude-opus-latest",
                false,
            ),
        ] {
            let provider =
                OpenAiCompatProvider::new(OpenAiCompatConfig::new("test".into(), base_url.into()))
                    .unwrap();
            let mut context = context(model);
            if compat {
                context.compatibility.openrouter = Some(serde_json::from_value(json!({})).unwrap());
            }
            assert_eq!(
                provider.uses_native_messages(&context),
                native,
                "{base_url}: {model}"
            );
        }
    }

    #[test]
    fn openrouter_chat_maps_remote_images_to_native_url_sources() {
        let provider = OpenAiCompatProvider::new(OpenAiCompatConfig::new(
            "openrouter".into(),
            "https://openrouter.ai/api/v1".into(),
        ))
        .unwrap();
        let mut context = context("anthropic/claude-sonnet-4.6");
        context.chat_protocol = gateway_core::ChatRequestProtocol::ChatCompletions;
        for url in [
            "https://example.test/image.png",
            "http://example.test/image.png",
        ] {
            for image_url in [json!({"url": url, "detail": "high"}), json!(url)] {
                let request = serde_json::from_value(json!({
                    "model": "claude",
                    "messages": [{"role": "user", "content": [
                        {"type": "text", "text": "Describe this"},
                        {"type": "image_url", "image_url": image_url}
                    ]}]
                }))
                .unwrap();
                for stream in [false, true] {
                    let wire = if stream {
                        provider.build_chat_stream_request(&request, &context)
                    } else {
                        provider.build_chat_request(&request, &context)
                    }
                    .unwrap();
                    assert_eq!(wire.url().as_str(), "https://openrouter.ai/api/v1/messages");
                    let body: Value =
                        serde_json::from_slice(wire.body().unwrap().as_bytes().unwrap()).unwrap();
                    assert_eq!(body["stream"], stream);
                    assert_eq!(
                        body["messages"],
                        json!([{"role": "user", "content": [
                            {"type": "text", "text": "Describe this"},
                            {"type": "image", "source": {"type": "url", "url": url}}
                        ]}])
                    );
                }
            }
        }
    }

    #[test]
    fn openrouter_native_history_remains_opaque_with_remote_image_support() {
        let provider = OpenAiCompatProvider::new(OpenAiCompatConfig::new(
            "openrouter".into(),
            "https://openrouter.ai/api/v1".into(),
        ))
        .unwrap();
        let messages = json!([
            {"role": "system", "content": [], "output_config": {"effort": "low"}},
            {"role": "user", "content": [
                {"type": "image", "source": {"type": "url", "url": "https://example.test/image.png"},
                    "cache_control": {"type": "ephemeral", "ttl": "1h"}},
                {"type": "future_content", "payload": {"preserve": true}}
            ], "future_message_option": true}
        ]);
        let request = serde_json::from_value(json!({
            "model": "claude", "max_tokens": 128, "messages": messages
        }))
        .unwrap();
        let wire = provider
            .build_chat_request(&request, &context("anthropic/claude-sonnet-4.6"))
            .unwrap();
        let body: Value = serde_json::from_slice(wire.body().unwrap().as_bytes().unwrap()).unwrap();
        assert_eq!(body["messages"], messages);
    }

    #[test]
    fn openrouter_route_token_alias_is_the_only_wire_limit() {
        let provider = OpenAiCompatProvider::new(OpenAiCompatConfig::new(
            "openrouter".into(),
            "https://openrouter.ai/api/v1".into(),
        ))
        .unwrap();
        let mut context = context("anthropic/claude-sonnet-4.6");
        context.chat_protocol = gateway_core::ChatRequestProtocol::ChatCompletions;
        context
            .extra_body
            .insert("max_completion_tokens".into(), json!(128));
        for caller_limits in [
            json!({}),
            json!({"max_tokens": 8192}),
            json!({"max_completion_tokens": 8192}),
        ] {
            let mut payload = json!({
                "model": "claude", "messages": [{"role": "user", "content": "hello"}]
            });
            payload
                .as_object_mut()
                .unwrap()
                .extend(caller_limits.as_object().unwrap().clone());
            let request = serde_json::from_value(payload).unwrap();
            let wire = provider.build_chat_request(&request, &context).unwrap();
            let body: Value =
                serde_json::from_slice(wire.body().unwrap().as_bytes().unwrap()).unwrap();
            assert_eq!(body["max_tokens"], 128);
            assert!(body.get("max_completion_tokens").is_none());
        }
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
