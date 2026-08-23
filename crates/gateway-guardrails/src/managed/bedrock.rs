use std::{collections::BTreeMap, sync::Arc, time::SystemTime};

use async_trait::async_trait;
use aws_config::{Region, default_provider::credentials::DefaultCredentialsChain};
use aws_credential_types::{Credentials, provider::ProvideCredentials};
use aws_sigv4::{
    http_request::{SignableBody, SignableRequest, SigningSettings, sign},
    sign::v4,
};
use aws_smithy_runtime_api::client::identity::Identity;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::OnceCell;

use crate::{
    ContentTransformation, EvaluationError, EvaluationInput, GuardPhase, ManagedDecisionMetadata,
    ManagedEvaluator, ManagedOutcome, ManagedService, ReasonCode, managed::input_text,
};

#[derive(Debug, Clone)]
pub enum BedrockAuth {
    DefaultChain,
    StaticCredentials {
        access_key_id: String,
        secret_access_key: String,
        session_token: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub struct BedrockApplyGuardrailConfig {
    pub evaluator_id: String,
    pub region: String,
    pub guardrail_identifier: String,
    pub guardrail_version: String,
    pub endpoint_url: Option<String>,
    pub auth: BedrockAuth,
    pub max_retries: u8,
}

impl BedrockApplyGuardrailConfig {
    pub fn validate(&self) -> Result<(), EvaluationError> {
        validate_guardrail_identifier(&self.guardrail_identifier)?;
        validate_guardrail_version(&self.guardrail_version)?;
        if self.region.is_empty()
            || !self
                .region
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            return Err(EvaluationError::MalformedResponse(
                "invalid Bedrock Guardrails region".to_string(),
            ));
        }
        if self.evaluator_id.trim().is_empty() {
            return Err(EvaluationError::MalformedResponse(
                "Bedrock Guardrails evaluator ID cannot be empty".to_string(),
            ));
        }
        Ok(())
    }

    fn endpoint(&self) -> String {
        self.endpoint_url
            .clone()
            .unwrap_or_else(|| format!("https://bedrock-runtime.{}.amazonaws.com", self.region))
    }
}

pub fn validate_guardrail_identifier(value: &str) -> Result<(), EvaluationError> {
    if value.is_empty()
        || value.len() > 2048
        || !value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':' | b'/' | b'.')
        })
    {
        return Err(EvaluationError::MalformedResponse(
            "invalid Bedrock guardrail identifier".to_string(),
        ));
    }
    Ok(())
}

pub fn validate_guardrail_version(value: &str) -> Result<(), EvaluationError> {
    if value.is_empty() || value.len() > 8 || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(EvaluationError::MalformedResponse(
            "Bedrock guardrail version must be a numbered production version".to_string(),
        ));
    }
    Ok(())
}

pub struct BedrockApplyGuardrail {
    config: BedrockApplyGuardrailConfig,
    client: reqwest::Client,
    default_credentials_chain: Arc<OnceCell<DefaultCredentialsChain>>,
}

impl BedrockApplyGuardrail {
    pub fn new(config: BedrockApplyGuardrailConfig) -> Result<Self, EvaluationError> {
        config.validate()?;
        Ok(Self {
            config,
            client: reqwest::Client::new(),
            default_credentials_chain: Arc::new(OnceCell::new()),
        })
    }

    pub fn with_client(
        config: BedrockApplyGuardrailConfig,
        client: reqwest::Client,
    ) -> Result<Self, EvaluationError> {
        config.validate()?;
        Ok(Self {
            config,
            client,
            default_credentials_chain: Arc::new(OnceCell::new()),
        })
    }

    async fn apply(&self, input: &EvaluationInput) -> Result<ManagedOutcome, EvaluationError> {
        let text = input_text(input);
        let source = match input.phase {
            GuardPhase::Prompt | GuardPhase::McpCall | GuardPhase::HarnessPreTool => "INPUT",
            GuardPhase::ModelResponse | GuardPhase::GeneratedToolCall | GuardPhase::McpResult => {
                "OUTPUT"
            }
        };
        let payload = json!({
            "source": source,
            "content": [{
                "text": {
                    "text": text,
                    "qualifiers": ["guard_content"]
                }
            }],
            "outputScope": "FULL"
        });

        let mut attempt = 0_u8;
        loop {
            let response = self.send(payload.clone()).await?;
            if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS
                && attempt < self.config.max_retries
            {
                attempt = attempt.saturating_add(1);
                tokio::time::sleep(std::time::Duration::from_millis(
                    25_u64.saturating_mul(u64::from(attempt)),
                ))
                .await;
                continue;
            }
            return normalize_response(response, &text).await;
        }
    }

    async fn send(&self, payload: Value) -> Result<reqwest::Response, EvaluationError> {
        let endpoint = format!(
            "{}/guardrail/{}/version/{}/apply",
            self.config.endpoint().trim_end_matches('/'),
            self.config.guardrail_identifier,
            self.config.guardrail_version
        );
        let request = self
            .client
            .post(endpoint)
            .json(&payload)
            .build()
            .map_err(|error| EvaluationError::Unavailable(error.to_string()))?;
        let request = self.sign_request(request).await?;
        self.client
            .execute(request)
            .await
            .map_err(map_reqwest_error)
    }

    async fn sign_request(
        &self,
        request: reqwest::Request,
    ) -> Result<reqwest::Request, EvaluationError> {
        let credentials = match &self.config.auth {
            BedrockAuth::DefaultChain => {
                let region = self.config.region.clone();
                let provider = self
                    .default_credentials_chain
                    .get_or_init(|| async move {
                        DefaultCredentialsChain::builder()
                            .region(Region::new(region))
                            .build()
                            .await
                    })
                    .await;
                provider
                    .provide_credentials()
                    .await
                    .map_err(|error| EvaluationError::Unavailable(error.to_string()))?
            }
            BedrockAuth::StaticCredentials {
                access_key_id,
                secret_access_key,
                session_token,
            } => Credentials::new(
                access_key_id,
                secret_access_key,
                session_token.clone(),
                None,
                "oceans-guardrails-static-credentials",
            ),
        };
        sign_request(request, credentials, &self.config.region)
    }
}

#[async_trait]
impl ManagedEvaluator for BedrockApplyGuardrail {
    fn id(&self) -> &str {
        &self.config.evaluator_id
    }

    fn service(&self) -> ManagedService {
        ManagedService::AmazonBedrock
    }

    async fn evaluate(&self, input: &EvaluationInput) -> Result<ManagedOutcome, EvaluationError> {
        self.apply(input).await
    }
}

fn sign_request(
    mut request: reqwest::Request,
    credentials: Credentials,
    region: &str,
) -> Result<reqwest::Request, EvaluationError> {
    request.headers_mut().remove(reqwest::header::AUTHORIZATION);
    request.headers_mut().remove("x-amz-date");
    request.headers_mut().remove("x-amz-security-token");

    let method = request.method().as_str().to_string();
    let uri = request.url().as_str().to_string();
    let body = request
        .body()
        .and_then(reqwest::Body::as_bytes)
        .ok_or_else(|| EvaluationError::MalformedResponse("SigV4 body is not buffered".into()))?
        .to_vec();
    let headers = request
        .headers()
        .iter()
        .map(|(name, value)| {
            value
                .to_str()
                .map(|value| (name.as_str(), value))
                .map_err(|error| EvaluationError::MalformedResponse(error.to_string()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let identity: Identity = credentials.into();
    let signing_params = v4::SigningParams::builder()
        .identity(&identity)
        .region(region)
        .name("bedrock")
        .time(SystemTime::now())
        .settings(SigningSettings::default())
        .build()
        .map_err(|error| EvaluationError::Unavailable(error.to_string()))?
        .into();
    let signable = SignableRequest::new(
        method.as_str(),
        uri.as_str(),
        headers.iter().copied(),
        SignableBody::Bytes(&body),
    )
    .map_err(|error| EvaluationError::Unavailable(error.to_string()))?;
    let (instructions, _) = sign(signable, &signing_params)
        .map_err(|error| EvaluationError::Unavailable(error.to_string()))?
        .into_parts();
    for header in instructions.headers() {
        let name = reqwest::header::HeaderName::from_bytes(header.0.as_bytes())
            .map_err(|error| EvaluationError::Unavailable(error.to_string()))?;
        let value = reqwest::header::HeaderValue::from_str(header.1)
            .map_err(|error| EvaluationError::Unavailable(error.to_string()))?;
        request.headers_mut().insert(name, value);
    }
    Ok(request)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApplyGuardrailResponse {
    action: String,
    #[serde(default)]
    outputs: Vec<GuardrailOutput>,
    #[serde(default)]
    assessments: Vec<Value>,
    #[serde(default)]
    usage: BTreeMap<String, u64>,
}

#[derive(Debug, Deserialize)]
struct GuardrailOutput {
    text: String,
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
            "Bedrock ApplyGuardrail returned HTTP {}",
            response.status().as_u16()
        )));
    }
    let response: ApplyGuardrailResponse = response
        .json()
        .await
        .map_err(|error| EvaluationError::MalformedResponse(error.to_string()))?;
    let metadata = ManagedDecisionMetadata {
        assessment_count: response.assessments.len().try_into().unwrap_or(u32::MAX),
        matched_filters: Vec::new(),
        usage_units: response.usage,
    };
    let output = response
        .outputs
        .into_iter()
        .map(|output| output.text)
        .collect::<String>();
    match response.action.as_str() {
        "NONE" if !output.is_empty() && output != original => Ok(ManagedOutcome::Transformed {
            transformation: ContentTransformation::new(output),
            reason_code: reason("bedrock.masked"),
            metadata: metadata.clone(),
        }),
        "NONE" => Ok(ManagedOutcome::Allow {
            reason_code: reason("bedrock.allow"),
            metadata: metadata.clone(),
        }),
        "GUARDRAIL_INTERVENED"
            if !output.is_empty() && has_anonymized_assessment(&response.assessments) =>
        {
            Ok(ManagedOutcome::Transformed {
                transformation: ContentTransformation::new(output),
                reason_code: reason("bedrock.anonymized"),
                metadata,
            })
        }
        "GUARDRAIL_INTERVENED" => Ok(ManagedOutcome::Intervention {
            reason_code: reason("bedrock.intervened"),
            metadata,
        }),
        other => Err(EvaluationError::MalformedResponse(format!(
            "unknown Bedrock guardrail action `{other}`"
        ))),
    }
}
fn has_anonymized_assessment(assessments: &[Value]) -> bool {
    assessments.iter().any(has_anonymized_action)
}

fn has_anonymized_action(value: &Value) -> bool {
    match value {
        Value::Object(object) => object.iter().any(|(key, value)| {
            (key == "action"
                && value
                    .as_str()
                    .is_some_and(|action| action.eq_ignore_ascii_case("ANONYMIZED")))
                || has_anonymized_action(value)
        }),
        Value::Array(values) => values.iter().any(has_anonymized_action),
        _ => false,
    }
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
        net::{TcpListener, TcpStream},
        sync::mpsc,
        thread,
        time::Duration,
    };

    use super::*;
    use crate::{EvaluationPayload, ManagedEvaluator};

    #[tokio::test]
    async fn sends_signed_apply_guardrail_request_and_maps_intervention() {
        let (endpoint, request_rx) =
            fake_server(r#"{"action":"GUARDRAIL_INTERVENED","outputs":[]}"#);
        let evaluator = BedrockApplyGuardrail::new(BedrockApplyGuardrailConfig {
            evaluator_id: "aws-primary".into(),
            region: "us-east-1".into(),
            guardrail_identifier: "guardrail-1".into(),
            guardrail_version: "1".into(),
            endpoint_url: Some(endpoint),
            auth: BedrockAuth::StaticCredentials {
                access_key_id: "access".into(),
                secret_access_key: "secret".into(),
                session_token: None,
            },
            max_retries: 0,
        })
        .unwrap();
        let outcome = evaluator
            .evaluate(&EvaluationInput::new(
                GuardPhase::Prompt,
                EvaluationPayload::Text {
                    text: "inspect me".into(),
                },
            ))
            .await
            .unwrap();
        assert!(matches!(outcome, ManagedOutcome::Intervention { .. }));
        let request = request_rx.recv().unwrap();
        assert!(request.starts_with("POST /guardrail/guardrail-1/version/1/apply HTTP/1.1"));
        assert!(
            request
                .to_ascii_lowercase()
                .contains("authorization: aws4-hmac-sha256")
        );
        assert!(request.contains(r#""source":"INPUT""#));
        assert!(request.contains(r#""text":"inspect me""#));
    }

    #[tokio::test]
    async fn maps_apply_guardrail_masking_as_a_transformation() {
        let (endpoint, request_rx) =
            fake_server(r#"{"action":"NONE","outputs":[{"text":"masked"}]}"#);
        let evaluator = BedrockApplyGuardrail::new(BedrockApplyGuardrailConfig {
            evaluator_id: "aws-masking".into(),
            region: "us-east-1".into(),
            guardrail_identifier: "guardrail-1".into(),
            guardrail_version: "1".into(),
            endpoint_url: Some(endpoint),
            auth: BedrockAuth::StaticCredentials {
                access_key_id: "access".into(),
                secret_access_key: "secret".into(),
                session_token: None,
            },
            max_retries: 0,
        })
        .unwrap();

        let outcome = evaluator
            .evaluate(&EvaluationInput::new(
                GuardPhase::ModelResponse,
                EvaluationPayload::Text {
                    text: "sensitive".into(),
                },
            ))
            .await
            .unwrap();

        assert!(matches!(
            outcome,
            ManagedOutcome::Transformed { transformation, .. }
                if transformation.content == "masked"
        ));
        assert!(request_rx.recv().unwrap().contains(r#""text":"sensitive""#));
    }

    #[tokio::test]
    async fn maps_intervened_anonymization_as_a_transformation() {
        let response_body = r#"{
            "action":"GUARDRAIL_INTERVENED",
            "outputs":[{"text":"Customer [NAME]"}],
            "assessments":[{
                "sensitiveInformationPolicy":{
                    "piiEntities":[{"type":"NAME","action":"ANONYMIZED"}]
                }
            }]
        }"#;
        let (endpoint, _) = fake_server(response_body);
        let response = reqwest::get(endpoint).await.unwrap();

        let outcome = normalize_response(response, "Customer Alice")
            .await
            .unwrap();

        assert!(matches!(
            outcome,
            ManagedOutcome::Transformed {
                transformation,
                reason_code,
                ..
            } if transformation.content == "Customer [NAME]"
                && reason_code.as_str() == "bedrock.anonymized"
        ));
    }

    #[tokio::test]
    async fn normalizes_assessment_usage_and_service_errors_without_content() {
        let response_body = r#"{
            "action":"NONE",
            "outputs":[],
            "assessments":[{"contentPolicy":{"filters":[]}}],
            "usage":{"contentPolicyUnits":2,"topicPolicyUnits":1}
        }"#;
        let (endpoint, _) = fake_server(response_body);
        let response = reqwest::get(endpoint).await.unwrap();
        let outcome = normalize_response(response, "private input").await.unwrap();
        let ManagedOutcome::Allow { metadata, .. } = outcome else {
            panic!("expected an allow outcome");
        };
        assert_eq!(metadata.assessment_count, 1);
        assert_eq!(metadata.usage_units["contentPolicyUnits"], 2);
        assert!(
            !serde_json::to_string(&metadata)
                .unwrap()
                .contains("private input")
        );

        for (status, expected) in [
            ("403 Forbidden", EvaluationError::AccessDenied),
            (
                "500 Internal Server Error",
                EvaluationError::Unavailable("Bedrock ApplyGuardrail returned HTTP 500".into()),
            ),
        ] {
            let (endpoint, _) = fake_server_response(status, "{}");
            let response = reqwest::get(endpoint).await.unwrap();
            assert_eq!(normalize_response(response, "input").await, Err(expected));
        }

        let (endpoint, _) = fake_server(r#"{"action":"UNKNOWN"}"#);
        let response = reqwest::get(endpoint).await.unwrap();
        assert!(matches!(
            normalize_response(response, "input").await,
            Err(EvaluationError::MalformedResponse(_))
        ));
    }

    #[tokio::test]
    async fn retries_throttling_once_and_maps_request_timeouts() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let mut requests = Vec::new();
            for (status, body) in [
                ("429 Too Many Requests", "{}"),
                ("200 OK", r#"{"action":"NONE","outputs":[]}"#),
            ] {
                let (mut stream, _) = listener.accept().unwrap();
                requests.push(read_request(&mut stream));
                write_response(&mut stream, status, body);
            }
            sender.send(requests).unwrap();
        });
        let evaluator = BedrockApplyGuardrail::new(BedrockApplyGuardrailConfig {
            evaluator_id: "aws-retry".into(),
            region: "us-east-1".into(),
            guardrail_identifier: "guardrail-1".into(),
            guardrail_version: "1".into(),
            endpoint_url: Some(format!("http://{address}")),
            auth: BedrockAuth::StaticCredentials {
                access_key_id: "access".into(),
                secret_access_key: "secret".into(),
                session_token: None,
            },
            max_retries: 1,
        })
        .unwrap();
        assert!(matches!(
            evaluator
                .evaluate(&EvaluationInput::new(
                    GuardPhase::Prompt,
                    EvaluationPayload::Text {
                        text: "inspect me".into(),
                    },
                ))
                .await
                .unwrap(),
            ManagedOutcome::Allow { .. }
        ));
        assert_eq!(receiver.recv().unwrap().len(), 2);

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
            let request = read_request(&mut stream);
            let _ = sender.send(request);
            write_response(&mut stream, status, body);
        });
        (format!("http://{address}"), receiver)
    }

    fn read_request(stream: &mut TcpStream) -> String {
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
                return String::from_utf8(request).unwrap();
            }
        }
    }

    fn write_response(stream: &mut TcpStream, status: &str, body: &str) {
        write!(
            stream,
            "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .unwrap();
    }
}
