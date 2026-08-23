use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::{
    ContentTransformation, EvaluationError, EvaluationInput, GuardPhase, ManagedDecisionMetadata,
    ManagedEvaluator, ManagedOutcome, ManagedService, ReasonCode, managed::input_text,
};

#[async_trait]
pub trait BearerTokenProvider: Send + Sync {
    async fn bearer_token(&self) -> Result<String, EvaluationError>;
}

pub struct StaticBearerTokenProvider {
    token: String,
}

impl StaticBearerTokenProvider {
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            token: token.into(),
        }
    }
}

#[async_trait]
impl BearerTokenProvider for StaticBearerTokenProvider {
    async fn bearer_token(&self) -> Result<String, EvaluationError> {
        if self.token.is_empty() {
            Err(EvaluationError::AccessDenied)
        } else {
            Ok(self.token.clone())
        }
    }
}

#[derive(Debug, Clone)]
pub struct ModelArmorConfig {
    pub evaluator_id: String,
    pub project: String,
    pub location: String,
    pub prompt_template: Option<String>,
    pub response_template: Option<String>,
    pub endpoint_url: Option<String>,
}

impl ModelArmorConfig {
    pub fn validate(&self) -> Result<(), EvaluationError> {
        if self.evaluator_id.trim().is_empty() {
            return Err(EvaluationError::MalformedResponse(
                "Model Armor evaluator ID cannot be empty".into(),
            ));
        }
        if self.project.trim().is_empty() || self.location.trim().is_empty() {
            return Err(EvaluationError::MalformedResponse(
                "Model Armor project and location are required".into(),
            ));
        }
        for template in [
            self.prompt_template.as_deref(),
            self.response_template.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            let (project, location) = validate_template_resource_name(template)?;
            if project != self.project || location != self.location {
                return Err(EvaluationError::MalformedResponse(
                    "Model Armor template project and location must match adapter config".into(),
                ));
            }
        }
        if self.prompt_template.is_none() && self.response_template.is_none() {
            return Err(EvaluationError::MalformedResponse(
                "Model Armor requires a prompt or response template".into(),
            ));
        }
        Ok(())
    }

    fn endpoint(&self) -> &str {
        self.endpoint_url
            .as_deref()
            .unwrap_or("https://modelarmor.googleapis.com")
    }
}

pub fn validate_template_resource_name(value: &str) -> Result<(String, String), EvaluationError> {
    let segments = value.split('/').collect::<Vec<_>>();
    if segments.len() != 6
        || segments[0] != "projects"
        || segments[1].is_empty()
        || segments[2] != "locations"
        || segments[3].is_empty()
        || segments[4] != "templates"
        || segments[5].is_empty()
    {
        return Err(EvaluationError::MalformedResponse(
            "invalid Model Armor template resource name".into(),
        ));
    }
    Ok((segments[1].to_string(), segments[3].to_string()))
}

pub struct ModelArmor {
    config: ModelArmorConfig,
    client: reqwest::Client,
    token_provider: Arc<dyn BearerTokenProvider>,
}

impl ModelArmor {
    pub fn new(
        config: ModelArmorConfig,
        token_provider: Arc<dyn BearerTokenProvider>,
    ) -> Result<Self, EvaluationError> {
        config.validate()?;
        Ok(Self {
            config,
            client: reqwest::Client::new(),
            token_provider,
        })
    }

    pub fn with_client(
        config: ModelArmorConfig,
        token_provider: Arc<dyn BearerTokenProvider>,
        client: reqwest::Client,
    ) -> Result<Self, EvaluationError> {
        config.validate()?;
        Ok(Self {
            config,
            client,
            token_provider,
        })
    }

    async fn sanitize(&self, input: &EvaluationInput) -> Result<ManagedOutcome, EvaluationError> {
        let text = input_text(input);
        let (template, method, payload) = match input.phase {
            GuardPhase::Prompt | GuardPhase::McpCall | GuardPhase::HarnessPreTool => (
                self.config.prompt_template.as_deref(),
                "sanitizeUserPrompt",
                json!({"userPromptData": {"text": text}}),
            ),
            GuardPhase::ModelResponse | GuardPhase::GeneratedToolCall | GuardPhase::McpResult => (
                self.config.response_template.as_deref(),
                "sanitizeModelResponse",
                json!({
                    "modelResponseData": {"text": text},
                    "userPrompt": input.associated_prompt.as_deref().unwrap_or_default()
                }),
            ),
        };
        let template = template.ok_or_else(|| {
            EvaluationError::MalformedResponse(format!(
                "Model Armor has no template for {}",
                method
            ))
        })?;
        let token = self.token_provider.bearer_token().await?;
        let endpoint = format!(
            "{}/v1/{}:{}",
            self.config.endpoint().trim_end_matches('/'),
            template,
            method
        );
        let response = self
            .client
            .post(endpoint)
            .bearer_auth(token)
            .json(&payload)
            .send()
            .await
            .map_err(map_reqwest_error)?;
        normalize_response(response, &text).await
    }
}

#[async_trait]
impl ManagedEvaluator for ModelArmor {
    fn id(&self) -> &str {
        &self.config.evaluator_id
    }

    fn service(&self) -> ManagedService {
        ManagedService::GoogleModelArmor
    }

    async fn evaluate(&self, input: &EvaluationInput) -> Result<ManagedOutcome, EvaluationError> {
        self.sanitize(input).await
    }
}

async fn normalize_response(
    response: reqwest::Response,
    original: &str,
) -> Result<ManagedOutcome, EvaluationError> {
    if response.status() == reqwest::StatusCode::FORBIDDEN {
        return Err(EvaluationError::AccessDenied);
    }
    if !response.status().is_success() {
        return Err(EvaluationError::Unavailable(format!(
            "Model Armor returned HTTP {}",
            response.status().as_u16()
        )));
    }
    let body: Value = response
        .json()
        .await
        .map_err(|error| EvaluationError::MalformedResponse(error.to_string()))?;
    let result = body
        .get("sanitizationResult")
        .ok_or_else(|| EvaluationError::MalformedResponse("missing sanitizationResult".into()))?;
    match result.get("invocationResult").and_then(Value::as_str) {
        Some("SUCCESS") => {}
        Some("PARTIAL" | "FAILURE") => {
            return Err(EvaluationError::Unavailable(
                "Model Armor filter execution was incomplete".into(),
            ));
        }
        other => {
            return Err(EvaluationError::MalformedResponse(format!(
                "invalid Model Armor invocationResult {other:?}"
            )));
        }
    }

    let metadata = model_armor_metadata(result);

    let transformed = transformed_text(result);
    if let Some(transformed) = transformed
        && transformed != original
    {
        return Ok(ManagedOutcome::Transformed {
            transformation: ContentTransformation::new(transformed.to_string()),
            reason_code: reason("model_armor.sanitized"),
            metadata,
        });
    }
    match result.get("filterMatchState").and_then(Value::as_str) {
        Some("NO_MATCH_FOUND") => Ok(ManagedOutcome::Allow {
            reason_code: reason("model_armor.allow"),
            metadata,
        }),
        Some("MATCH_FOUND") => Ok(ManagedOutcome::Intervention {
            reason_code: reason("model_armor.match"),
            metadata,
        }),
        other => Err(EvaluationError::MalformedResponse(format!(
            "invalid Model Armor filterMatchState {other:?}"
        ))),
    }
}

fn model_armor_metadata(result: &Value) -> ManagedDecisionMetadata {
    let filters = result.get("filterResults").and_then(Value::as_object);
    ManagedDecisionMetadata {
        assessment_count: filters
            .map(|filters| filters.len().try_into().unwrap_or(u32::MAX))
            .unwrap_or_default(),
        matched_filters: filters
            .into_iter()
            .flat_map(|filters| filters.iter())
            .filter(|(_, result)| contains_match(result))
            .map(|(name, _)| name.clone())
            .collect(),
        usage_units: Default::default(),
    }
}

fn contains_match(value: &Value) -> bool {
    match value {
        Value::String(value) => value == "MATCH_FOUND",
        Value::Array(values) => values.iter().any(contains_match),
        Value::Object(values) => values.values().any(contains_match),
        _ => false,
    }
}

fn transformed_text(result: &Value) -> Option<&str> {
    let filters = result.get("filterResults")?.as_object()?;
    filters.values().find_map(|filter| {
        filter
            .get("sdpFilterResult")
            .and_then(|value| value.get("deidentifyResult"))
            .and_then(|value| value.get("data"))
            .and_then(|value| value.get("text"))
            .and_then(Value::as_str)
    })
}

fn map_reqwest_error(error: reqwest::Error) -> EvaluationError {
    if error.is_timeout() {
        EvaluationError::Timeout
    } else {
        EvaluationError::Unavailable(error.to_string())
    }
}

fn reason(value: &str) -> ReasonCode {
    ReasonCode::new(value).expect("static reason code is valid")
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::{Arc, mpsc},
        thread,
        time::Duration,
    };

    use super::*;
    use crate::{EvaluationPayload, ManagedEvaluator};

    #[tokio::test]
    async fn sends_model_response_with_associated_prompt_and_maps_sanitization() {
        let response = r#"{
            "sanitizationResult": {
                "invocationResult": "SUCCESS",
                "filterMatchState": "MATCH_FOUND",
                "filterResults": {
                    "sdp": {
                        "sdpFilterResult": {
                            "deidentifyResult": {"data": {"text": "masked"}}
                        }
                    }
                }
            }
        }"#;
        let (endpoint, request_rx) = fake_server(response);
        let evaluator = ModelArmor::new(
            ModelArmorConfig {
                evaluator_id: "gcp-primary".into(),
                project: "project-one".into(),
                location: "us-central1".into(),
                prompt_template: None,
                response_template: Some(
                    "projects/project-one/locations/us-central1/templates/response".into(),
                ),
                endpoint_url: Some(endpoint),
            },
            Arc::new(StaticBearerTokenProvider::new("token")),
        )
        .unwrap();
        let outcome = evaluator
            .evaluate(
                &EvaluationInput::new(
                    GuardPhase::ModelResponse,
                    EvaluationPayload::Text {
                        text: "sensitive".into(),
                    },
                )
                .with_associated_prompt("original prompt"),
            )
            .await
            .unwrap();
        assert!(matches!(
            outcome,
            ManagedOutcome::Transformed { transformation, .. }
                if transformation.content == "masked"
        ));
        let request = request_rx.recv().unwrap();
        assert!(request.starts_with(
            "POST /v1/projects/project-one/locations/us-central1/templates/response:sanitizeModelResponse HTTP/1.1"
        ));
        assert!(
            request
                .to_ascii_lowercase()
                .contains("authorization: bearer token")
        );
        assert!(request.contains(r#""userPrompt":"original prompt""#));
        assert!(request.contains(r#""text":"sensitive""#));
    }

    #[tokio::test]
    async fn normalizes_filter_metadata_and_service_errors_without_content() {
        let response_body = r#"{
            "sanitizationResult": {
                "invocationResult": "SUCCESS",
                "filterMatchState": "MATCH_FOUND",
                "filterResults": {
                    "rai": {"raiFilterResult":{"matchState":"MATCH_FOUND"}},
                    "maliciousUri": {"maliciousUriFilterResult":{"matchState":"NO_MATCH_FOUND"}}
                }
            }
        }"#;
        let (endpoint, _) = fake_server(response_body);
        let response = reqwest::get(endpoint).await.unwrap();
        let outcome = normalize_response(response, "private input").await.unwrap();
        let ManagedOutcome::Intervention { metadata, .. } = outcome else {
            panic!("expected an intervention outcome");
        };
        assert_eq!(metadata.assessment_count, 2);
        assert_eq!(metadata.matched_filters, ["rai"]);
        assert!(
            !serde_json::to_string(&metadata)
                .unwrap()
                .contains("private input")
        );

        for (status, expected) in [
            ("403 Forbidden", EvaluationError::AccessDenied),
            (
                "500 Internal Server Error",
                EvaluationError::Unavailable("Model Armor returned HTTP 500".into()),
            ),
        ] {
            let (endpoint, _) = fake_server_response(status, "{}");
            let response = reqwest::get(endpoint).await.unwrap();
            assert_eq!(normalize_response(response, "input").await, Err(expected));
        }

        for body in [
            r#"{"sanitizationResult":{"invocationResult":"SUCCESS"}}"#,
            r#"{"sanitizationResult":{"invocationResult":"UNKNOWN"}}"#,
        ] {
            let (endpoint, _) = fake_server(body);
            let response = reqwest::get(endpoint).await.unwrap();
            assert!(matches!(
                normalize_response(response, "input").await,
                Err(EvaluationError::MalformedResponse(_))
            ));
        }
    }

    struct FailingTokenProvider;

    #[async_trait]
    impl BearerTokenProvider for FailingTokenProvider {
        async fn bearer_token(&self) -> Result<String, EvaluationError> {
            Err(EvaluationError::AccessDenied)
        }
    }

    #[tokio::test]
    async fn maps_token_and_request_timeout_failures() {
        let evaluator = ModelArmor::new(
            ModelArmorConfig {
                evaluator_id: "gcp-auth".into(),
                project: "project-one".into(),
                location: "us-central1".into(),
                prompt_template: Some(
                    "projects/project-one/locations/us-central1/templates/prompt".into(),
                ),
                response_template: None,
                endpoint_url: Some("http://127.0.0.1:1".into()),
            },
            Arc::new(FailingTokenProvider),
        )
        .unwrap();
        assert_eq!(
            evaluator
                .evaluate(&EvaluationInput::new(
                    GuardPhase::Prompt,
                    EvaluationPayload::Text {
                        text: "inspect me".into(),
                    },
                ))
                .await,
            Err(EvaluationError::AccessDenied)
        );

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        thread::spawn(move || {
            let _stream = listener.accept().unwrap();
            thread::sleep(Duration::from_millis(50));
        });
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(1))
            .build()
            .unwrap();
        let error = client
            .get(format!("http://{address}"))
            .send()
            .await
            .unwrap_err();
        assert_eq!(map_reqwest_error(error), EvaluationError::Timeout);
    }

    fn fake_server(body: &'static str) -> (String, mpsc::Receiver<String>) {
        fake_server_response("200 OK", body)
    }

    fn fake_server_response(
        status: &'static str,
        body: &'static str,
    ) -> (String, mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut buffer = [0_u8; 4096];
            loop {
                let count = stream.read(&mut buffer).unwrap();
                request.extend_from_slice(&buffer[..count]);
                let text = String::from_utf8_lossy(&request);
                let Some(header_end) = text.find("\r\n\r\n") else {
                    continue;
                };
                let content_length = text[..header_end]
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .and_then(|value| value.trim().parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                if request.len() >= header_end + 4 + content_length {
                    break;
                }
            }
            let _ = sender.send(String::from_utf8(request).unwrap());
            write!(
                stream,
                "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        });
        (format!("http://{address}"), receiver)
    }
}
