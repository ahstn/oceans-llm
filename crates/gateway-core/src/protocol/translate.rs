use std::collections::BTreeMap;

use crate::protocol::{anthropic, core, openai};

#[must_use]
pub fn anthropic_messages_request_to_core(
    request: &anthropic::AnthropicMessagesRequest,
) -> core::ChatRequest {
    let mut messages = Vec::new();
    if let Some(system) = &request.system {
        messages.push(core::ChatMessage {
            role: "system".to_string(),
            content: system.clone(),
            name: None,
            extra: BTreeMap::new(),
        });
    }
    messages.extend(request.messages.iter().map(|message| core::ChatMessage {
        role: message.role.clone(),
        content: message.content.clone(),
        name: None,
        extra: message.extra.clone(),
    }));

    let mut extra = request.extra.clone();
    if let Some(max_tokens) = &request.max_tokens {
        extra.insert("max_tokens".to_string(), max_tokens.clone());
    }
    if let Some(tools) = &request.tools {
        extra.insert("tools".to_string(), tools.clone());
    }
    if let Some(tool_choice) = &request.tool_choice {
        extra.insert("tool_choice".to_string(), tool_choice.clone());
    }
    if let Some(thinking) = &request.thinking {
        extra.insert("thinking".to_string(), thinking.clone());
    }

    core::ChatRequest {
        model: request.model.clone(),
        messages,
        stream: request.stream,
        extra,
    }
}

#[must_use]
pub fn openai_chat_request_to_core(request: &openai::ChatCompletionsRequest) -> core::ChatRequest {
    core::ChatRequest {
        model: request.model.clone(),
        messages: request
            .messages
            .iter()
            .map(|message| core::ChatMessage {
                role: message.role.clone(),
                content: message.content.clone(),
                name: message.name.clone(),
                extra: message.extra.clone(),
            })
            .collect(),
        stream: request.stream,
        extra: request.extra.clone(),
    }
}

#[must_use]
pub fn core_chat_request_to_openai(request: &core::ChatRequest) -> openai::ChatCompletionsRequest {
    openai::ChatCompletionsRequest {
        model: request.model.clone(),
        messages: request
            .messages
            .iter()
            .map(|message| openai::ChatMessage {
                role: message.role.clone(),
                content: message.content.clone(),
                name: message.name.clone(),
                extra: message.extra.clone(),
            })
            .collect(),
        stream: request.stream,
        extra: request.extra.clone(),
    }
}

#[must_use]
pub fn openai_embeddings_request_to_core(
    request: &openai::EmbeddingsRequest,
) -> core::EmbeddingsRequest {
    core::EmbeddingsRequest {
        model: request.model.clone(),
        input: request.input.clone(),
        extra: request.extra.clone(),
    }
}

#[must_use]
pub fn core_embeddings_request_to_openai(
    request: &core::EmbeddingsRequest,
) -> openai::EmbeddingsRequest {
    openai::EmbeddingsRequest {
        model: request.model.clone(),
        input: request.input.clone(),
        extra: request.extra.clone(),
    }
}

#[must_use]
pub fn openai_responses_request_to_core(
    request: &openai::ResponsesRequest,
) -> core::ResponsesRequest {
    core::ResponsesRequest {
        model: request.model.clone(),
        input: request.input.clone(),
        stream: request.stream,
        instructions: request.instructions.clone(),
        tools: request.tools.clone(),
        tool_choice: request.tool_choice.clone(),
        reasoning: request.reasoning.clone(),
        text: request.text.clone(),
        extra: request.extra.clone(),
    }
}

#[must_use]
pub fn core_responses_request_to_openai(
    request: &core::ResponsesRequest,
) -> openai::ResponsesRequest {
    openai::ResponsesRequest {
        model: request.model.clone(),
        input: request.input.clone(),
        stream: request.stream,
        instructions: request.instructions.clone(),
        tools: request.tools.clone(),
        tool_choice: request.tool_choice.clone(),
        reasoning: request.reasoning.clone(),
        text: request.text.clone(),
        extra: request.extra.clone(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::{Value, json};

    use crate::protocol::{
        anthropic::{AnthropicMessage, AnthropicMessagesRequest},
        openai::{ChatCompletionsRequest, ChatMessage, EmbeddingsRequest, ResponsesRequest},
        translate::{
            anthropic_messages_request_to_core, core_chat_request_to_openai,
            core_embeddings_request_to_openai, core_responses_request_to_openai,
            openai_chat_request_to_core, openai_embeddings_request_to_core,
            openai_responses_request_to_core,
        },
    };

    #[test]
    fn chat_request_round_trips_between_openai_and_core() {
        let mut message_extra = BTreeMap::new();
        message_extra.insert("cache_control".to_string(), json!({"type":"ephemeral"}));

        let mut request_extra = BTreeMap::new();
        request_extra.insert("temperature".to_string(), json!(0.2));
        request_extra.insert("reasoning".to_string(), json!({"effort":"medium"}));

        let openai_request = ChatCompletionsRequest {
            model: "fast".to_string(),
            messages: vec![ChatMessage {
                role: "developer".to_string(),
                content: Value::String("you are concise".to_string()),
                name: Some("policy".to_string()),
                extra: message_extra,
            }],
            stream: true,
            extra: request_extra,
        };

        let core_request = openai_chat_request_to_core(&openai_request);
        assert_eq!(core_request.model, "fast");
        assert_eq!(core_request.messages.len(), 1);
        assert_eq!(core_request.messages[0].role, "developer");
        assert_eq!(core_request.messages[0].name.as_deref(), Some("policy"));
        assert_eq!(core_request.extra.get("temperature"), Some(&json!(0.2)));

        let translated_back = core_chat_request_to_openai(&core_request);
        assert_eq!(translated_back.model, openai_request.model);
        assert_eq!(translated_back.messages, openai_request.messages);
        assert_eq!(translated_back.stream, openai_request.stream);
        assert_eq!(translated_back.extra, openai_request.extra);
    }

    #[test]
    fn anthropic_messages_request_maps_to_tool_capable_core_chat() {
        let request = AnthropicMessagesRequest {
            model: "claude".to_string(),
            system: Some(json!("be terse")),
            messages: vec![AnthropicMessage {
                role: "user".to_string(),
                content: json!("hi"),
                extra: BTreeMap::new(),
            }],
            max_tokens: Some(json!(64)),
            stream: true,
            tools: Some(json!([{
                "name": "noop",
                "input_schema": {"type":"object","properties":{}}
            }])),
            tool_choice: None,
            thinking: None,
            extra: BTreeMap::new(),
        };

        let core = anthropic_messages_request_to_core(&request);
        let requirements = core.requirements();

        assert_eq!(core.messages[0].role, "system");
        assert_eq!(core.extra["max_tokens"], json!(64));
        assert!(requirements.chat_completions);
        assert!(requirements.stream);
        assert!(requirements.tools);
    }

    #[test]
    fn embeddings_request_round_trips_between_openai_and_core() {
        let mut request_extra = BTreeMap::new();
        request_extra.insert(
            "encoding_format".to_string(),
            Value::String("float".to_string()),
        );

        let openai_request = EmbeddingsRequest {
            model: "embed-fast".to_string(),
            input: json!(["hello", "world"]),
            extra: request_extra,
        };

        let core_request = openai_embeddings_request_to_core(&openai_request);
        assert_eq!(core_request.model, "embed-fast");
        assert_eq!(core_request.input, json!(["hello", "world"]));
        assert_eq!(
            core_request.extra.get("encoding_format"),
            Some(&Value::String("float".to_string()))
        );

        let translated_back = core_embeddings_request_to_openai(&core_request);
        assert_eq!(translated_back.model, openai_request.model);
        assert_eq!(translated_back.input, openai_request.input);
        assert_eq!(translated_back.extra, openai_request.extra);
    }

    #[test]
    fn responses_request_round_trips_between_openai_and_core() {
        let mut request_extra = BTreeMap::new();
        request_extra.insert("metadata".to_string(), json!({"tenant":"acme"}));

        let openai_request = ResponsesRequest {
            model: "reasoning".to_string(),
            input: json!([
                {"type":"message","role":"user","content":"hello"},
                {"type":"function_call_output","call_id":"call_1","output":"ok"}
            ]),
            stream: true,
            instructions: Some(json!("Answer with citations.")),
            tools: Some(json!([{"type":"function","name":"lookup"}])),
            tool_choice: Some(json!("auto")),
            reasoning: Some(json!({"effort":"medium"})),
            text: Some(json!({"format":{"type":"text"}})),
            extra: request_extra,
        };

        let core_request = openai_responses_request_to_core(&openai_request);
        assert_eq!(core_request.model, "reasoning");
        assert_eq!(core_request.input, openai_request.input);
        assert_eq!(core_request.tools, openai_request.tools);
        assert_eq!(core_request.reasoning, openai_request.reasoning);

        let translated_back = core_responses_request_to_openai(&core_request);
        assert_eq!(translated_back, openai_request);
    }
}
