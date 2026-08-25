use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use gateway_core::{
    ApiKeyModelGrantMode, ApiKeyOwnerKind, AuthenticatedApiKey, ChatCompletionsRequest,
    GatewayError, ProviderError, RequestAttemptStatus, RequestLogRepository,
    RequestLogRetentionWindow, RequestTag, RequestTags, ResponsesRequest,
};
use serde_json::{Value, json};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{RequestLogIconMetadata, redaction::RequestLogPayloadCaptureMode};

use super::super::{RequestLogging, StreamFailureSummary, StreamLogResultInput};
use super::{
    InMemoryRepo, policy, policy_with_redaction_paths, sample_attempt, sample_auth,
    sample_embeddings_request, sample_icon_metadata, sample_log, sample_request,
    sample_service_account_auth, user_record,
};

#[tokio::test]
async fn suppresses_logging_for_user_toggle_disabled() {
    let user_id = Uuid::new_v4();
    let repo = Arc::new(InMemoryRepo {
        users: Arc::new(Mutex::new(vec![user_record(user_id, false)])),
        logs: Arc::new(Mutex::new(Vec::new())),
        payloads: Arc::new(Mutex::new(Vec::new())),
        attempts: Arc::new(Mutex::new(Vec::new())),
    });
    let logging = RequestLogging::new(repo.clone());
    let auth = sample_auth(user_id);
    let context = logging.begin_chat_request(
        "req_1",
        "fast",
        "fast",
        &ChatCompletionsRequest {
            model: "fast".to_string(),
            messages: Vec::new(),
            stream: false,
            extra: BTreeMap::new(),
        },
        &BTreeMap::new(),
        RequestTags::default(),
    );

    let wrote = logging
        .log_non_stream_success(
            &auth,
            &context,
            "openai-prod",
            RequestLogIconMetadata {
                provider_icon_key: crate::ProviderIconKey::OpenAI,
                model_icon_key: Some(crate::ModelIconKey::OpenAI),
            },
            120,
            0,
            &json!({"usage": {"prompt_tokens": 1, "completion_tokens": 2}}),
            Vec::new(),
        )
        .await
        .expect("request logging should evaluate");

    assert!(!wrote.wrote);
    assert_eq!(repo.logs.lock().expect("logs lock").len(), 0);
}

#[tokio::test]
async fn purge_request_logs_uses_typed_retention_window() {
    let now = OffsetDateTime::now_utc();
    let repo = Arc::new(InMemoryRepo {
        users: Arc::new(Mutex::new(Vec::new())),
        logs: Arc::new(Mutex::new(vec![
            sample_log("old", now - time::Duration::days(2)),
            sample_log("young", now),
        ])),
        payloads: Arc::new(Mutex::new(Vec::new())),
        attempts: Arc::new(Mutex::new(Vec::new())),
    });
    let logging = RequestLogging::new(repo.clone());

    let dry_run = logging
        .purge_request_logs(RequestLogRetentionWindow::OneDay, true)
        .await
        .expect("dry run purge");
    assert!(dry_run.dry_run);
    assert_eq!(dry_run.matched_count, 1);
    assert_eq!(dry_run.deleted_count, 0);
    assert_eq!(repo.logs.lock().expect("logs lock").len(), 2);

    let purge = logging
        .purge_request_logs(RequestLogRetentionWindow::OneDay, false)
        .await
        .expect("purge logs");
    assert!(!purge.dry_run);
    assert_eq!(purge.matched_count, 1);
    assert_eq!(purge.deleted_count, 1);
    let logs = repo.logs.lock().expect("logs lock");
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].request_id, "young");
}

#[tokio::test]
async fn logs_service_account_requests_with_payload_and_redaction() {
    let team_id = Uuid::new_v4();
    let service_account_id = Uuid::new_v4();
    let repo = Arc::new(InMemoryRepo::default());
    let logging = RequestLogging::new(repo.clone());
    let auth = AuthenticatedApiKey {
        id: Uuid::new_v4(),
        public_id: "dev123".to_string(),
        name: "dev".to_string(),
        model_grant_mode: ApiKeyModelGrantMode::Explicit,
        owner_kind: ApiKeyOwnerKind::ServiceAccount,
        owner_user_id: None,
        owner_team_id: Some(team_id),
        owner_service_account_id: Some(service_account_id),
    };
    let mut headers = BTreeMap::new();
    headers.insert("authorization".to_string(), "secret".to_string());
    headers.insert("session_id".to_string(), "session-1".to_string());
    headers.insert("x-client-secret".to_string(), "client-secret".to_string());
    let context = logging.begin_chat_request(
        "req_1",
        "fast",
        "fast",
        &ChatCompletionsRequest {
            model: "fast".to_string(),
            messages: Vec::new(),
            stream: false,
            extra: BTreeMap::from([("token".to_string(), Value::String("secret".to_string()))]),
        },
        &headers,
        RequestTags {
            service: Some("checkout".to_string()),
            component: Some("pricing_api".to_string()),
            env: Some("prod".to_string()),
            bespoke: vec![RequestTag {
                key: "feature".to_string(),
                value: "guest_checkout".to_string(),
            }],
        },
    );

    let wrote = logging
        .log_non_stream_success(
            &auth,
            &context,
            "openai-prod",
            RequestLogIconMetadata {
                provider_icon_key: crate::ProviderIconKey::OpenAI,
                model_icon_key: Some(crate::ModelIconKey::OpenAI),
            },
            120,
            0,
            &json!({"usage": {"prompt_tokens": 10, "completion_tokens": 20, "total_tokens": 30}}),
            Vec::new(),
        )
        .await
        .expect("request logging should evaluate");

    let logs = repo.logs.lock().expect("logs lock");
    let payloads = repo.payloads.lock().expect("payloads lock");
    assert!(wrote.wrote);
    assert_eq!(logs.len(), 1);
    assert!(logs[0].user_id.is_none());
    assert_eq!(logs[0].team_id, Some(team_id));
    assert!(logs[0].has_payload);
    assert_eq!(
        payloads[0].request_json["headers"]["session_id"],
        "session-1"
    );
    assert!(
        payloads[0].request_json["headers"]
            .get("authorization")
            .is_none()
    );
    assert!(
        payloads[0].request_json["headers"]
            .get("x-client-secret")
            .is_none()
    );
    assert_eq!(payloads[0].request_json["body"]["token"], "[REDACTED]");
    assert_eq!(logs[0].request_tags.service.as_deref(), Some("checkout"));
    assert_eq!(logs[0].request_tags.bespoke[0].key, "feature");
    assert_eq!(
        logs[0].metadata["operation"],
        Value::String("chat_completions".to_string())
    );
    assert_eq!(logs[0].metadata["stream"], Value::Bool(false));
    assert!(logs[0].metadata.get("fallback_used").is_none());
    assert!(logs[0].metadata.get("attempt_count").is_none());
}

#[tokio::test]
async fn embeddings_success_log_records_operation_usage_payload_and_attempt() {
    let repo = Arc::new(InMemoryRepo::default());
    let logging = RequestLogging::new(repo.clone());
    let auth = sample_service_account_auth();
    let context = logging.begin_embeddings_request(
        "req_embeddings_success",
        "embeddings",
        "embeddings",
        &sample_embeddings_request(),
        &BTreeMap::new(),
        RequestTags::default(),
    );
    let attempt = sample_attempt(
        context.request_log_id,
        &context.request_id,
        RequestAttemptStatus::Success,
    );

    let wrote = logging
        .log_non_stream_success(
            &auth,
            &context,
            "vertex-prod",
            sample_icon_metadata(),
            75,
            0,
            &json!({
                "object": "list",
                "model": "embeddings",
                "data": [{"object": "embedding", "index": 0, "embedding": [0.1]}],
                "usage": {"prompt_tokens": 7, "total_tokens": 7}
            }),
            vec![attempt],
        )
        .await
        .expect("embeddings success log");

    assert!(wrote.wrote);
    let detail = repo
        .get_request_log_detail(wrote.request_log_id)
        .await
        .expect("request log detail");
    assert_eq!(detail.log.request_id, "req_embeddings_success");
    assert_eq!(detail.log.provider_key, "vertex-prod");
    assert_eq!(detail.log.status_code, Some(200));
    assert_eq!(detail.log.prompt_tokens, Some(7));
    assert_eq!(detail.log.total_tokens, Some(7));
    assert_eq!(
        detail.log.metadata["operation"],
        Value::String("embeddings".to_string())
    );
    assert_eq!(
        detail
            .payload
            .as_ref()
            .expect("captured payload")
            .request_json["body"]["input"],
        "hello"
    );
    assert_eq!(detail.attempts.len(), 1);
    assert_eq!(detail.attempts[0].status, RequestAttemptStatus::Success);
    assert_eq!(
        detail.attempts[0].upstream_model,
        "google/gemini-embedding-001"
    );
    assert!(detail.attempts[0].produced_final_response);
}

#[tokio::test]
async fn embeddings_provider_error_log_records_failure_payload_and_attempt() {
    let repo = Arc::new(InMemoryRepo::default());
    let logging = RequestLogging::new(repo.clone());
    let auth = sample_service_account_auth();
    let context = logging.begin_embeddings_request(
        "req_embeddings_failure",
        "embeddings",
        "embeddings",
        &sample_embeddings_request(),
        &BTreeMap::new(),
        RequestTags::default(),
    );
    let attempt = sample_attempt(
        context.request_log_id,
        &context.request_id,
        RequestAttemptStatus::ProviderError,
    );
    let gateway_error =
        GatewayError::Provider(ProviderError::Transport("connection reset".to_string()));

    let wrote = logging
        .log_non_stream_failure(
            &auth,
            &context,
            "vertex-prod",
            sample_icon_metadata(),
            90,
            &gateway_error,
            vec![attempt],
        )
        .await
        .expect("embeddings provider error log");

    assert!(wrote.wrote);
    let detail = repo
        .get_request_log_detail(wrote.request_log_id)
        .await
        .expect("request log detail");
    assert_eq!(detail.log.request_id, "req_embeddings_failure");
    assert_eq!(detail.log.status_code, Some(502));
    assert_eq!(detail.log.error_code.as_deref(), Some("upstream_transport"));
    assert_eq!(
        detail.log.metadata["operation"],
        Value::String("embeddings".to_string())
    );
    assert_eq!(
        detail
            .payload
            .as_ref()
            .expect("captured payload")
            .response_json["body"]["error"]["code"],
        "upstream_transport"
    );
    assert_eq!(detail.attempts.len(), 1);
    assert_eq!(
        detail.attempts[0].status,
        RequestAttemptStatus::ProviderError
    );
    assert_eq!(detail.attempts[0].status_code, Some(502));
    assert_eq!(
        detail.attempts[0].error_code.as_deref(),
        Some("upstream_transport")
    );
    assert!(detail.attempts[0].retryable);
    assert!(!detail.attempts[0].produced_final_response);
}

#[tokio::test]
async fn disabled_payload_policy_writes_no_request_log_rows() {
    let repo = Arc::new(InMemoryRepo::default());
    let logging = RequestLogging::new_with_payload_policy(
        repo.clone(),
        policy(RequestLogPayloadCaptureMode::Disabled, 1024, 1024, 4),
    );
    let auth = sample_service_account_auth();
    let context = logging.begin_chat_request(
        "req_1",
        "fast",
        "fast",
        &sample_request(false),
        &BTreeMap::new(),
        RequestTags::default(),
    );

    let wrote = logging
        .log_non_stream_success(
            &auth,
            &context,
            "openai-prod",
            sample_icon_metadata(),
            120,
            0,
            &json!({"usage": {"prompt_tokens": 1, "completion_tokens": 2}}),
            Vec::new(),
        )
        .await
        .expect("request logging should evaluate");

    assert!(!wrote.wrote);
    assert!(repo.logs.lock().expect("logs lock").is_empty());
    assert!(repo.payloads.lock().expect("payloads lock").is_empty());
}

#[tokio::test]
async fn summary_only_payload_policy_writes_summary_without_payload() {
    let repo = Arc::new(InMemoryRepo::default());
    let logging = RequestLogging::new_with_payload_policy(
        repo.clone(),
        policy(RequestLogPayloadCaptureMode::SummaryOnly, 1024, 1024, 4),
    );
    let auth = sample_service_account_auth();
    let context = logging.begin_chat_request(
        "req_1",
        "fast",
        "fast",
        &sample_request(false),
        &BTreeMap::new(),
        RequestTags::default(),
    );

    let wrote = logging
        .log_non_stream_success(
            &auth,
            &context,
            "openai-prod",
            sample_icon_metadata(),
            120,
            0,
            &json!({"usage": {"prompt_tokens": 1, "completion_tokens": 2, "total_tokens": 3}}),
            Vec::new(),
        )
        .await
        .expect("summary log");

    let logs = repo.logs.lock().expect("logs lock");
    assert!(wrote.wrote);
    assert_eq!(logs.len(), 1);
    assert!(!logs[0].has_payload);
    assert!(!logs[0].request_payload_truncated);
    assert!(!logs[0].response_payload_truncated);
    assert_eq!(
        logs[0].metadata["payload_policy"]["capture_mode"],
        "summary_only"
    );
    assert!(repo.payloads.lock().expect("payloads lock").is_empty());
}

#[tokio::test]
async fn separate_payload_limits_mark_only_affected_side_truncated() {
    let repo = Arc::new(InMemoryRepo::default());
    let logging = RequestLogging::new_with_payload_policy(
        repo.clone(),
        policy(RequestLogPayloadCaptureMode::RedactedPayloads, 4096, 80, 4),
    );
    let auth = sample_service_account_auth();
    let context = logging.begin_chat_request(
        "req_1",
        "fast",
        "fast",
        &sample_request(false),
        &BTreeMap::new(),
        RequestTags::default(),
    );

    logging
        .log_non_stream_success(
            &auth,
            &context,
            "openai-prod",
            sample_icon_metadata(),
            120,
            0,
            &json!({
                "choices": [{"message": {"content": "x".repeat(512)}}],
                "usage": {"prompt_tokens": 1, "completion_tokens": 2, "total_tokens": 3}
            }),
            Vec::new(),
        )
        .await
        .expect("truncated log");

    let logs = repo.logs.lock().expect("logs lock");
    let payloads = repo.payloads.lock().expect("payloads lock");
    assert!(logs[0].has_payload);
    assert!(!logs[0].request_payload_truncated);
    assert!(logs[0].response_payload_truncated);
    assert_eq!(payloads[0].response_json["truncated"], true);
}

#[tokio::test]
async fn under_cap_responses_request_redacts_media_urls_and_filters_headers() {
    let repo = Arc::new(InMemoryRepo::default());
    let logging = RequestLogging::new(repo.clone());
    let request = ResponsesRequest {
        model: "gpt-test".to_string(),
        input: json!([{
            "type": "message",
            "role": "user",
            "content": [
                {
                    "type": "input_image",
                    "image_url": "https://media.example.invalid/image.png?token=image-secret"
                },
                {
                    "type": "input_file",
                    "file_url": "https://media.example.invalid/file.pdf?signature=file-secret"
                }
            ]
        }]),
        stream: false,
        instructions: None,
        tools: None,
        tool_choice: None,
        reasoning: None,
        text: None,
        extra: BTreeMap::new(),
    };
    let headers = BTreeMap::from([
        ("user-agent".to_string(), "pi/0.80.2".to_string()),
        ("session_id".to_string(), "pi-session-small".to_string()),
        (
            "x-client-request-id".to_string(),
            "pi-session-small".to_string(),
        ),
        ("x-client-secret".to_string(), "header-secret".to_string()),
    ]);
    let context = logging.begin_responses_request(
        "req_pi_small",
        "gpt-test",
        "gpt-test",
        &request,
        &headers,
        RequestTags::default(),
    );

    assert!(!context.request_payload_truncated);
    assert_eq!(
        context.analysis_metadata.external_session_id.as_deref(),
        Some("pi-session-small")
    );
    logging
        .log_non_stream_success(
            &sample_service_account_auth(),
            &context,
            "openai-prod",
            sample_icon_metadata(),
            10,
            0,
            &json!({"id": "response-small", "status": "completed"}),
            Vec::new(),
        )
        .await
        .expect("log under-cap request");

    let payloads = repo.payloads.lock().expect("payloads lock");
    let stored = &payloads[0].request_json;
    assert_eq!(stored["headers"]["session_id"], "pi-session-small");
    assert!(stored["headers"].get("x-client-secret").is_none());
    assert_eq!(
        stored["body"]["input"][0]["content"][0]["image_url"],
        "https://media.example.invalid/image.png?<redacted>"
    );
    assert_eq!(
        stored["body"]["input"][0]["content"][1]["file_url"],
        "https://media.example.invalid/file.pdf?<redacted>"
    );
    let retained = stored.to_string();
    for secret in ["header-secret", "image-secret", "file-secret"] {
        assert!(!retained.contains(secret));
    }
}

#[tokio::test]
async fn oversized_pi_responses_request_keeps_analysis_and_structured_storage() {
    let repo = Arc::new(InMemoryRepo::default());
    let request_max_bytes = 8 * 1024;
    let logging = RequestLogging::new_with_payload_policy(
        repo.clone(),
        policy(
            RequestLogPayloadCaptureMode::RedactedPayloads,
            request_max_bytes,
            4096,
            4,
        ),
    );
    let input = json!([
        {
            "type": "message",
            "id": "message-1",
            "role": "user",
            "content": "first 🙂 prompt ".repeat(1200)
        },
        {
            "type": "future_input_item",
            "id": "future-1",
            "name": "preserved-item",
            "metadata": {"keep": true},
            "content": [
                {"type": "input_text", "text": "second é prompt ".repeat(900)},
                {"type": "input_text", "text": "short marker"},
                {
                    "type": "input_image",
                    "image_url": "https://media.example.invalid/oversized.png?token=oversized-secret"
                }
            ]
        }
    ]);
    let instructions = "system instructions 🙂 ".repeat(900);
    let tools = json!([{
        "type": "function",
        "name": "search",
        "description": "Search repository content",
        "parameters": {
            "type": "object",
            "properties": {"query": {"type": "string"}},
            "required": ["query"]
        }
    }]);
    let request = ResponsesRequest {
        model: "gpt-test".to_string(),
        input: input.clone(),
        stream: false,
        instructions: Some(json!(instructions.clone())),
        tools: Some(tools),
        tool_choice: Some(json!("auto")),
        reasoning: Some(json!({"effort": "high"})),
        text: None,
        extra: BTreeMap::from([
            (
                "include".to_string(),
                json!(["reasoning.encrypted_content"]),
            ),
            ("metadata".to_string(), json!({"trace": "kept"})),
            ("prompt_cache_key".to_string(), json!("cache-key-1")),
        ]),
    };
    let session_id = "pi-session-123";
    let headers = BTreeMap::from([
        ("user-agent".to_string(), "pi/0.80.2".to_string()),
        ("session_id".to_string(), session_id.to_string()),
        ("x-client-request-id".to_string(), session_id.to_string()),
        ("x-auth-token".to_string(), "header-secret".to_string()),
    ]);
    let original_prompt_bytes = serde_json::to_vec(&input).expect("serialize input").len()
        + serde_json::to_vec(&instructions)
            .expect("serialize instructions")
            .len();

    let context = logging.begin_responses_request(
        "req_pi_oversized",
        "gpt-test",
        "gpt-test",
        &request,
        &headers,
        RequestTags::default(),
    );

    assert!(context.request_payload_truncated);
    assert_eq!(
        context.analysis_metadata.external_session_id.as_deref(),
        Some(session_id)
    );
    assert_eq!(
        context.analysis_metadata.session_source.as_deref(),
        Some("header:session_id+header:x-client-request-id")
    );
    assert_eq!(
        context.analysis_metadata.prompt_bytes,
        u64::try_from(original_prompt_bytes).ok()
    );
    assert_eq!(context.analysis_metadata.supplied_tool_count, Some(1));
    assert_eq!(context.analysis_metadata.supplied_tools[0].name, "search");

    logging
        .log_non_stream_success(
            &sample_service_account_auth(),
            &context,
            "openai-prod",
            sample_icon_metadata(),
            120,
            0,
            &json!({"id": "response-1", "status": "completed"}),
            Vec::new(),
        )
        .await
        .expect("log oversized request");

    let payloads = repo.payloads.lock().expect("payloads lock");
    let stored = &payloads[0].request_json;
    assert!(serde_json::to_vec(stored).expect("serialize stored").len() <= request_max_bytes);
    assert_eq!(stored["headers"]["session_id"], session_id);
    assert!(stored["headers"].get("x-auth-token").is_none());
    assert_eq!(stored["body"]["model"], "gpt-test");
    assert_eq!(stored["body"]["reasoning"]["effort"], "high");
    assert_eq!(stored["body"]["tools"][0]["name"], "search");
    assert_eq!(stored["body"]["tool_choice"], "auto");
    assert_eq!(stored["body"]["include"][0], "reasoning.encrypted_content");
    assert_eq!(stored["body"]["prompt_cache_key"], "cache-key-1");
    assert_eq!(stored["body"]["metadata"]["trace"], "kept");
    assert!(
        stored["body"]["instructions"]
            .as_str()
            .expect("instructions")
            .contains("gateway truncated")
    );
    assert_eq!(stored["body"]["input"][0]["role"], "user");
    assert_eq!(stored["body"]["input"][1]["type"], "future_input_item");
    assert_eq!(stored["body"]["input"][1]["name"], "preserved-item");
    assert_eq!(stored["body"]["input"][1]["metadata"]["keep"], true);
    assert!(
        stored["body"]["input"][0]["content"]
            .as_str()
            .expect("string content")
            .contains("gateway truncated")
    );
    assert!(
        stored["body"]["input"][1]["content"][0]["text"]
            .as_str()
            .expect("content array text")
            .contains("gateway truncated")
    );
    assert_eq!(
        stored["body"]["input"][1]["content"][2]["image_url"],
        "https://media.example.invalid/oversized.png?<redacted>"
    );
    let retained = stored.to_string();
    assert!(!retained.contains("header-secret"));
    assert!(!retained.contains("oversized-secret"));
    assert_eq!(
        stored["body"]["input"][1]["content"][1]["text"],
        "short marker"
    );
    assert_eq!(
        stored["truncation"]["strategy_version"],
        "structured-request-v2"
    );
}

#[tokio::test]
async fn operator_redaction_paths_apply_to_wrapped_response_payloads() {
    let repo = Arc::new(InMemoryRepo::default());
    let logging = RequestLogging::new_with_payload_policy(
        repo.clone(),
        policy_with_redaction_paths(&["body.choices.*.message.content"]),
    );
    let auth = sample_service_account_auth();
    let context = logging.begin_chat_request(
        "req_1",
        "fast",
        "fast",
        &sample_request(false),
        &BTreeMap::new(),
        RequestTags::default(),
    );

    logging
        .log_non_stream_success(
            &auth,
            &context,
            "openai-prod",
            sample_icon_metadata(),
            120,
            0,
            &json!({
                "choices": [{"message": {"content": "operator-secret"}}],
                "usage": {"prompt_tokens": 1, "completion_tokens": 2, "total_tokens": 3}
            }),
            Vec::new(),
        )
        .await
        .expect("redacted response log");

    let payloads = repo.payloads.lock().expect("payloads lock");
    assert_eq!(
        payloads[0].response_json["body"]["choices"][0]["message"]["content"],
        "[REDACTED]"
    );
}

#[tokio::test]
async fn records_stream_failures_with_payload() {
    let repo = Arc::new(InMemoryRepo::default());
    let logging = RequestLogging::new(repo.clone());
    let auth = AuthenticatedApiKey {
        id: Uuid::new_v4(),
        public_id: "dev123".to_string(),
        name: "dev".to_string(),
        model_grant_mode: ApiKeyModelGrantMode::Explicit,
        owner_kind: ApiKeyOwnerKind::ServiceAccount,
        owner_user_id: None,
        owner_team_id: Some(Uuid::new_v4()),
        owner_service_account_id: Some(Uuid::new_v4()),
    };
    let context = logging.begin_chat_request(
        "req_1",
        "fast",
        "fast",
        &ChatCompletionsRequest {
            model: "fast".to_string(),
            messages: Vec::new(),
            stream: true,
            extra: BTreeMap::new(),
        },
        &BTreeMap::new(),
        RequestTags::default(),
    );
    let mut collector = logging.new_stream_response_collector();
    collector.observe_chunk(
        br#"data: {"delta":"hello"}

"#,
    );

    let wrote = logging
        .log_stream_result(
            &auth,
            &context,
            StreamLogResultInput {
                provider_key: "openai-prod".to_string(),
                icon_metadata: RequestLogIconMetadata {
                    provider_icon_key: crate::ProviderIconKey::OpenAI,
                    model_icon_key: Some(crate::ModelIconKey::OpenAI),
                },
                latency_ms: 120,
                collector,
                failure: Some(StreamFailureSummary {
                    status_code: 502,
                    error_code: "stream_error".to_string(),
                }),
                attempts: Vec::new(),
            },
        )
        .await
        .expect("stream failure log");

    assert!(wrote.wrote);
    let logs = repo.logs.lock().expect("logs lock");
    let payload = repo.payloads.lock().expect("payloads lock");
    assert_eq!(
        logs[0].metadata["operation"],
        Value::String("chat_completions".to_string())
    );
    assert_eq!(logs[0].metadata["stream"], Value::Bool(true));
    assert!(logs[0].metadata.get("fallback_used").is_none());
    assert!(logs[0].metadata.get("attempt_count").is_none());
    assert_eq!(payload[0].response_json["error"]["code"], "stream_error");
}

#[tokio::test]
async fn stream_event_storage_cap_does_not_stop_usage_parsing() {
    let repo = Arc::new(InMemoryRepo::default());
    let logging = RequestLogging::new_with_payload_policy(
        repo.clone(),
        policy(
            RequestLogPayloadCaptureMode::RedactedPayloads,
            4096,
            4096,
            1,
        ),
    );
    let auth = sample_service_account_auth();
    let context = logging.begin_chat_request(
        "req_1",
        "fast",
        "fast",
        &sample_request(true),
        &BTreeMap::new(),
        RequestTags::default(),
    );
    let mut collector = logging.new_stream_response_collector();
    collector.observe_chunk(
        br#"data: {"choices":[{"delta":{"content":"hello"}}]}

data: {"usage":{"prompt_tokens":4,"completion_tokens":5,"total_tokens":9}}

"#,
    );

    logging
        .log_stream_result(
            &auth,
            &context,
            StreamLogResultInput {
                provider_key: "openai-prod".to_string(),
                icon_metadata: sample_icon_metadata(),
                latency_ms: 120,
                collector,
                failure: None,
                attempts: Vec::new(),
            },
        )
        .await
        .expect("stream log");

    let logs = repo.logs.lock().expect("logs lock");
    let payload = repo.payloads.lock().expect("payloads lock");
    assert_eq!(logs[0].total_tokens, Some(9));
    assert!(logs[0].response_payload_truncated);
    assert_eq!(
        payload[0].response_json["events"].as_array().unwrap().len(),
        1
    );
}

#[tokio::test]
async fn operator_redaction_paths_apply_to_wrapped_stream_payloads() {
    let repo = Arc::new(InMemoryRepo::default());
    let logging = RequestLogging::new_with_payload_policy(
        repo.clone(),
        policy_with_redaction_paths(&["events.*.choices.*.delta.content"]),
    );
    let auth = sample_service_account_auth();
    let context = logging.begin_chat_request(
        "req_1",
        "fast",
        "fast",
        &sample_request(true),
        &BTreeMap::new(),
        RequestTags::default(),
    );
    let mut collector = logging.new_stream_response_collector();
    collector.observe_chunk(
        br#"data: {"choices":[{"delta":{"content":"operator-secret"}}]}

"#,
    );

    logging
        .log_stream_result(
            &auth,
            &context,
            StreamLogResultInput {
                provider_key: "openai-prod".to_string(),
                icon_metadata: sample_icon_metadata(),
                latency_ms: 120,
                collector,
                failure: None,
                attempts: Vec::new(),
            },
        )
        .await
        .expect("stream log");

    let payloads = repo.payloads.lock().expect("payloads lock");
    assert_eq!(
        payloads[0].response_json["events"][0]["choices"][0]["delta"]["content"],
        "[REDACTED]"
    );
}
