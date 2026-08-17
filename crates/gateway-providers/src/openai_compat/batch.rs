use std::collections::BTreeSet;

use bytes::{Bytes, BytesMut};
use futures_util::StreamExt;
use gateway_core::{
    BatchCapabilities, BatchStatus, Money4, ProviderBatchRequest, ProviderBatchResult,
    ProviderBatchState, ProviderBatchSubmission, ProviderError,
};
use reqwest::multipart::{Form, Part};
use serde::Serialize;
use serde_json::{Value, json};
use time::OffsetDateTime;

use super::{
    OpenAiBatchDialect, OpenAiCompatProvider, normalize_anthropic_messages_tools_for_openai,
};
use crate::http::{join_base_url, map_reqwest_error};

#[derive(Serialize)]
struct OpenRouterCreate<'a> {
    endpoint: &'a str,
    model: &'a str,
    requests: Vec<OpenRouterItem<'a>>,
}

#[derive(Serialize)]
struct OpenRouterItem<'a> {
    custom_id: &'a str,
    body: Value,
}

impl OpenAiCompatProvider {
    pub(super) fn batch_capabilities_impl(&self) -> BatchCapabilities {
        match self.config.batch.dialect {
            OpenAiBatchDialect::Disabled => BatchCapabilities::NONE,
            OpenAiBatchDialect::OpenAi => BatchCapabilities {
                chat_completions: true,
                responses: true,
                embeddings: true,
                cancel: true,
            },
            OpenAiBatchDialect::OpenRouter => BatchCapabilities {
                chat_completions: true,
                responses: true,
                embeddings: false,
                cancel: false,
            },
        }
    }

    pub(super) async fn submit_batch_impl(
        &self,
        request: &ProviderBatchRequest,
    ) -> ProviderBatchSubmission {
        match self.config.batch.dialect {
            OpenAiBatchDialect::Disabled => ProviderBatchSubmission::NotSubmitted(batch_disabled()),
            OpenAiBatchDialect::OpenAi => self.submit_openai_batch(request).await,
            OpenAiBatchDialect::OpenRouter => self.submit_openrouter_batch(request).await,
        }
    }

    pub(super) async fn inspect_batch_impl(
        &self,
        provider_batch_id: &str,
        context: &gateway_core::ProviderRequestContext,
    ) -> Result<ProviderBatchState, ProviderError> {
        if self.config.batch.dialect == OpenAiBatchDialect::Disabled {
            return Err(batch_disabled());
        }
        let value = self
            .batch_json(
                reqwest::Method::GET,
                &format!("batches/{provider_batch_id}"),
                None,
                Some(context),
            )
            .await?;
        parse_state(&value)
    }

    pub(super) async fn cancel_batch_impl(
        &self,
        provider_batch_id: &str,
        context: &gateway_core::ProviderRequestContext,
    ) -> Result<ProviderBatchState, ProviderError> {
        if self.config.batch.dialect != OpenAiBatchDialect::OpenAi {
            return Err(ProviderError::NotImplemented(
                "OpenRouter does not document a batch cancellation endpoint".to_string(),
            ));
        }
        let value = self
            .batch_json(
                reqwest::Method::POST,
                &format!("batches/{provider_batch_id}/cancel"),
                Some(json!({})),
                Some(context),
            )
            .await?;
        parse_state(&value)
    }

    pub(super) async fn batch_results_impl(
        &self,
        state: &ProviderBatchState,
        context: &gateway_core::ProviderRequestContext,
    ) -> Result<Vec<ProviderBatchResult>, ProviderError> {
        if self.config.batch.dialect == OpenAiBatchDialect::Disabled {
            return Err(batch_disabled());
        }
        let value = self
            .batch_json(
                reqwest::Method::GET,
                &format!("batches/{}", state.provider_batch_id),
                None,
                Some(context),
            )
            .await?;
        match self.config.batch.dialect {
            OpenAiBatchDialect::OpenAi => self.openai_results(&value, context).await,
            OpenAiBatchDialect::OpenRouter => Ok(parse_openrouter_results(&value)),
            OpenAiBatchDialect::Disabled => unreachable!("disabled dialect returned above"),
        }
    }

    async fn submit_openai_batch(&self, request: &ProviderBatchRequest) -> ProviderBatchSubmission {
        let file_id = match self.upload_openai_batch_file(request).await {
            Ok(file_id) => file_id,
            Err(error) => return ProviderBatchSubmission::NotSubmitted(error),
        };
        let result = self
            .batch_json(
                reqwest::Method::POST,
                "batches",
                Some(json!({
                    "input_file_id": &file_id,
                    "endpoint": request.endpoint.provider_path(),
                    "completion_window": "24h",
                    "metadata": {"oceans_batch_id": request.batch_id.to_string()},
                })),
                Some(&request.context),
            )
            .await
            .and_then(|value| parse_state(&value));
        match result {
            Ok(state) => ProviderBatchSubmission::Submitted(state),
            Err(error) if submission_is_unknown(&error) => {
                match self
                    .reconcile_openai_batch(request.batch_id, &request.context)
                    .await
                {
                    Some(state) => ProviderBatchSubmission::Submitted(state),
                    None => ProviderBatchSubmission::SubmissionUnknown(error),
                }
            }
            Err(error) => {
                self.delete_openai_batch_file(&file_id, &request.context)
                    .await;
                ProviderBatchSubmission::NotSubmitted(error)
            }
        }
    }

    async fn submit_openrouter_batch(
        &self,
        request: &ProviderBatchRequest,
    ) -> ProviderBatchSubmission {
        let requests = match request
            .items
            .iter()
            .map(|item| {
                Ok(OpenRouterItem {
                    custom_id: &item.custom_id,
                    body: self.prepare_batch_item_body(request, item)?,
                })
            })
            .collect::<Result<Vec<_>, ProviderError>>()
        {
            Ok(requests) => requests,
            Err(error) => return ProviderBatchSubmission::NotSubmitted(error),
        };
        let payload = match serde_json::to_value(OpenRouterCreate {
            endpoint: request.endpoint.provider_path(),
            model: &request.upstream_model,
            requests,
        })
        .map_err(|error| ProviderError::Transport(error.to_string()))
        {
            Ok(payload) => payload,
            Err(error) => return ProviderBatchSubmission::NotSubmitted(error),
        };
        match self
            .batch_json(
                reqwest::Method::POST,
                "batches",
                Some(payload),
                Some(&request.context),
            )
            .await
            .and_then(|value| parse_state(&value))
        {
            Ok(state) => ProviderBatchSubmission::Submitted(state),
            Err(error) if submission_is_unknown(&error) => {
                ProviderBatchSubmission::SubmissionUnknown(error)
            }
            Err(error) => ProviderBatchSubmission::NotSubmitted(error),
        }
    }

    fn prepare_batch_item_body(
        &self,
        request: &ProviderBatchRequest,
        item: &gateway_core::ProviderBatchRequestItem,
    ) -> Result<Value, ProviderError> {
        let mut body = item.body.clone();
        let object = body.as_object_mut().ok_or_else(|| {
            ProviderError::InvalidRequest(format!(
                "batch item `{}` body must be a JSON object",
                item.custom_id
            ))
        })?;
        object.insert(
            "model".to_string(),
            Value::String(request.upstream_model.clone()),
        );
        if request.endpoint == gateway_core::BatchEndpoint::ChatCompletions {
            normalize_anthropic_messages_tools_for_openai(object);
        }
        let endpoint_suffix = match request.endpoint {
            gateway_core::BatchEndpoint::ChatCompletions => "chat/completions",
            gateway_core::BatchEndpoint::Responses => "responses",
            gateway_core::BatchEndpoint::Embeddings => "embeddings",
        };
        let mut body = self.prepare_request_body(
            endpoint_suffix,
            body,
            &request.context,
            false,
            request.endpoint == gateway_core::BatchEndpoint::ChatCompletions,
        )?;
        if let Some(object) = body.as_object_mut() {
            object.remove("stream");
        }
        Ok(body)
    }

    async fn upload_openai_batch_file(
        &self,
        request: &ProviderBatchRequest,
    ) -> Result<String, ProviderError> {
        for item in &request.items {
            self.encode_openai_batch_item(request, item)?;
        }
        let url = join_base_url(&self.batch_base_url(), "files")?;
        let provider = self.clone();
        let stream_request = request.clone();
        let jsonl = async_stream::stream! {
            for item in &stream_request.items {
                let encoded = provider.encode_openai_batch_item(&stream_request, item);
                let failed = encoded.is_err();
                yield encoded;
                if failed {
                    break;
                }
            }
        };
        let part = Part::stream(reqwest::Body::wrap_stream(jsonl))
            .file_name("batch.jsonl")
            .mime_str("application/jsonl")
            .map_err(|error| ProviderError::Transport(error.to_string()))?;
        let builder = self
            .apply_batch_auth(
                self.client
                    .post(url)
                    .multipart(Form::new().text("purpose", "batch").part("file", part)),
                Some(&request.context),
            )
            .await?;
        let value = response_json(builder.send().await.map_err(map_reqwest_error)?).await?;
        value
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| ProviderError::Transport("OpenAI file upload omitted id".to_string()))
    }

    fn encode_openai_batch_item(
        &self,
        request: &ProviderBatchRequest,
        item: &gateway_core::ProviderBatchRequestItem,
    ) -> Result<Bytes, ProviderError> {
        let body = self.prepare_batch_item_body(request, item)?;
        let mut encoded = serde_json::to_vec(&json!({
            "custom_id": item.custom_id,
            "method": "POST",
            "url": request.endpoint.provider_path(),
            "body": body,
        }))
        .map_err(|error| {
            ProviderError::Transport(format!("failed encoding OpenAI batch JSONL: {error}"))
        })?;
        encoded.push(b'\n');
        Ok(Bytes::from(encoded))
    }

    async fn delete_openai_batch_file(
        &self,
        file_id: &str,
        context: &gateway_core::ProviderRequestContext,
    ) {
        let _ = self
            .batch_json(
                reqwest::Method::DELETE,
                &format!("files/{file_id}"),
                None,
                Some(context),
            )
            .await;
    }

    async fn reconcile_openai_batch(
        &self,
        batch_id: uuid::Uuid,
        context: &gateway_core::ProviderRequestContext,
    ) -> Option<ProviderBatchState> {
        let batch_id = batch_id.to_string();
        let mut after = None;
        let mut seen_cursors = BTreeSet::new();
        loop {
            let path = match after.as_deref() {
                Some(cursor) => {
                    let query = url::form_urlencoded::Serializer::new(String::new())
                        .append_pair("limit", "100")
                        .append_pair("after", cursor)
                        .finish();
                    format!("batches?{query}")
                }
                None => "batches?limit=100".to_string(),
            };
            let value = self
                .batch_json(reqwest::Method::GET, &path, None, Some(context))
                .await
                .ok()?;
            let batches = value.get("data")?.as_array()?;
            if let Some(state) = batches
                .iter()
                .find(|batch| {
                    batch
                        .pointer("/metadata/oceans_batch_id")
                        .and_then(Value::as_str)
                        == Some(batch_id.as_str())
                })
                .and_then(|batch| parse_state(batch).ok())
            {
                return Some(state);
            }
            if value.get("has_more").and_then(Value::as_bool) != Some(true) {
                return None;
            }
            let cursor = batches.last()?.get("id")?.as_str()?.to_string();
            if !seen_cursors.insert(cursor.clone()) {
                return None;
            }
            after = Some(cursor);
        }
    }

    async fn openai_results(
        &self,
        batch: &Value,
        context: &gateway_core::ProviderRequestContext,
    ) -> Result<Vec<ProviderBatchResult>, ProviderError> {
        let mut results = Vec::new();
        for key in ["output_file_id", "error_file_id"] {
            let Some(file_id) = batch.get(key).and_then(Value::as_str) else {
                continue;
            };
            results.extend(self.openai_result_file(file_id, context).await?);
        }
        Ok(results)
    }

    async fn openai_result_file(
        &self,
        file_id: &str,
        context: &gateway_core::ProviderRequestContext,
    ) -> Result<Vec<ProviderBatchResult>, ProviderError> {
        let builder = self
            .apply_batch_auth(
                self.client.get(join_base_url(
                    &self.batch_base_url(),
                    &format!("files/{file_id}/content"),
                )?),
                Some(context),
            )
            .await?;
        let response = builder.send().await.map_err(map_reqwest_error)?;
        let status = response.status();
        if !status.is_success() {
            return Err(ProviderError::UpstreamHttp {
                status: status.as_u16(),
                body: response.text().await.map_err(map_reqwest_error)?,
            });
        }
        let mut chunks = response.bytes_stream();
        let mut pending = BytesMut::new();
        let mut results = Vec::new();
        while let Some(chunk) = chunks.next().await {
            pending.extend_from_slice(&chunk.map_err(map_reqwest_error)?);
            while let Some(newline) = pending.iter().position(|byte| *byte == b'\n') {
                let mut line = pending.split_to(newline + 1);
                line.truncate(newline);
                parse_openai_result_line(&line, &mut results)?;
            }
        }
        parse_openai_result_line(&pending, &mut results)?;
        Ok(results)
    }

    async fn batch_json(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
        context: Option<&gateway_core::ProviderRequestContext>,
    ) -> Result<Value, ProviderError> {
        let mut builder = self
            .client
            .request(method, join_base_url(&self.batch_base_url(), path)?);
        if let Some(body) = body {
            builder = builder.json(&body);
        }
        builder = self.apply_batch_auth(builder, context).await?;
        response_json(builder.send().await.map_err(map_reqwest_error)?).await
    }

    fn batch_base_url(&self) -> String {
        self.config
            .batch
            .base_url
            .clone()
            .unwrap_or_else(|| self.config.base_url.clone())
    }

    async fn apply_batch_auth(
        &self,
        mut builder: reqwest::RequestBuilder,
        context: Option<&gateway_core::ProviderRequestContext>,
    ) -> Result<reqwest::RequestBuilder, ProviderError> {
        for (name, value) in &self.config.default_headers {
            builder = builder.header(name, value);
        }
        if let Some(context) = context {
            for (name, value) in &context.extra_headers {
                if let Some(value) = value.as_str() {
                    builder = builder.header(name, value);
                }
            }
            builder = builder.header("x-request-id", &context.request_id);
        }
        if let Some(token) = self.auth_token().await? {
            builder = self.config.bearer_auth_header.apply(builder, &token);
        }
        Ok(builder)
    }
}

fn batch_disabled() -> ProviderError {
    ProviderError::NotImplemented("batch mode is not enabled for this provider".to_string())
}

fn submission_is_unknown(error: &ProviderError) -> bool {
    matches!(
        error,
        ProviderError::Timeout
            | ProviderError::Transport(_)
            | ProviderError::UpstreamHttp {
                status: 500..=599,
                ..
            }
    )
}

async fn response_text(response: reqwest::Response) -> Result<String, ProviderError> {
    let status = response.status();
    let text = response.text().await.map_err(map_reqwest_error)?;
    if status.is_success() {
        Ok(text)
    } else {
        Err(ProviderError::UpstreamHttp {
            status: status.as_u16(),
            body: text,
        })
    }
}

async fn response_json(response: reqwest::Response) -> Result<Value, ProviderError> {
    serde_json::from_str(&response_text(response).await?)
        .map_err(|error| ProviderError::Transport(format!("invalid batch provider JSON: {error}")))
}

fn parse_state(value: &Value) -> Result<ProviderBatchState, ProviderError> {
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| ProviderError::Transport("batch response omitted id".to_string()))?;
    let raw_status = value
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("in_progress");
    let counts = value.get("request_counts").unwrap_or(value);
    Ok(ProviderBatchState {
        provider_batch_id: id.to_string(),
        status: parse_status(raw_status)?,
        request_count: count(counts, &["total", "total_count"]),
        completed_count: count(counts, &["completed", "completed_count", "succeeded"]),
        failed_count: count(counts, &["failed", "failed_count"]),
        provider_usage: value.get("usage").cloned(),
        provider_cost_usd: value.pointer("/usage/cost").and_then(parse_money),
        error: value
            .get("errors")
            .cloned()
            .or_else(|| value.get("error").cloned()),
        submitted_at: timestamp(value, &["in_progress_at", "created_at"]),
        completed_at: timestamp(
            value,
            &["completed_at", "cancelled_at", "expired_at", "failed_at"],
        ),
    })
}

fn parse_status(raw: &str) -> Result<BatchStatus, ProviderError> {
    match raw {
        "validating" => Ok(BatchStatus::Validating),
        "in_progress" | "running" => Ok(BatchStatus::InProgress),
        "finalizing" => Ok(BatchStatus::Finalizing),
        "completed" => Ok(BatchStatus::Completed),
        "failed" => Ok(BatchStatus::Failed),
        "expired" => Ok(BatchStatus::Expired),
        "cancelling" => Ok(BatchStatus::Cancelling),
        "cancelled" => Ok(BatchStatus::Cancelled),
        other => Err(ProviderError::Transport(format!(
            "unknown batch status `{other}`"
        ))),
    }
}

fn count(value: &Value, keys: &[&str]) -> i64 {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_i64))
        .unwrap_or(0)
}

fn timestamp(value: &Value, keys: &[&str]) -> Option<OffsetDateTime> {
    keys.iter().find_map(|key| {
        value
            .get(*key)
            .and_then(Value::as_i64)
            .and_then(|ts| OffsetDateTime::from_unix_timestamp(ts).ok())
    })
}

fn parse_money(value: &Value) -> Option<Money4> {
    let raw = value
        .as_str()
        .map(str::to_string)
        .or_else(|| value.as_f64().map(|number| number.to_string()))?;
    Money4::from_decimal_str(&raw).ok()
}

fn parse_openai_result(value: &Value) -> Result<ProviderBatchResult, ProviderError> {
    let custom_id = value
        .get("custom_id")
        .and_then(Value::as_str)
        .ok_or_else(|| ProviderError::Transport("batch result omitted custom_id".to_string()))?;
    let response = value.get("response");
    let error = value
        .get("error")
        .filter(|error| !error.is_null())
        .cloned()
        .or_else(|| openai_response_error(response));
    Ok(ProviderBatchResult {
        custom_id: custom_id.to_string(),
        response_body: response.and_then(|item| item.get("body")).cloned(),
        error,
        provider_request_id: response
            .and_then(|item| item.get("request_id"))
            .and_then(Value::as_str)
            .map(str::to_string),
        provider_usage: response
            .and_then(|item| item.pointer("/body/usage"))
            .cloned(),
        completed_at: None,
        cost_usd: response
            .and_then(|item| item.pointer("/body/usage/cost"))
            .and_then(parse_money),
    })
}

fn openai_response_error(response: Option<&Value>) -> Option<Value> {
    let response = response?;
    let status_code = response.get("status_code")?.as_u64()?;
    if (200..300).contains(&status_code) {
        return None;
    }
    Some(json!({
        "status_code": status_code,
        "body": response.get("body").cloned().unwrap_or(Value::Null),
    }))
}

fn parse_openai_result_line(
    line: &[u8],
    results: &mut Vec<ProviderBatchResult>,
) -> Result<(), ProviderError> {
    if line.iter().all(u8::is_ascii_whitespace) {
        return Ok(());
    }
    let value: Value = serde_json::from_slice(line).map_err(|error| {
        ProviderError::Transport(format!("invalid OpenAI batch result JSONL: {error}"))
    })?;
    results.push(parse_openai_result(&value)?);
    Ok(())
}

fn parse_openrouter_results(value: &Value) -> Vec<ProviderBatchResult> {
    value
        .get("requests")
        .or_else(|| value.get("results"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let custom_id = item.get("custom_id")?.as_str()?;
            let response = item.get("response").or_else(|| item.get("result"));
            Some(ProviderBatchResult {
                custom_id: custom_id.to_string(),
                response_body: response
                    .and_then(|value| value.get("body"))
                    .cloned()
                    .or_else(|| response.cloned()),
                error: item.get("error").filter(|error| !error.is_null()).cloned(),
                provider_request_id: item.get("id").and_then(Value::as_str).map(str::to_string),
                provider_usage: item
                    .get("usage")
                    .cloned()
                    .or_else(|| response.and_then(|value| value.get("usage")).cloned())
                    .or_else(|| {
                        response
                            .and_then(|value| value.pointer("/body/usage"))
                            .cloned()
                    }),
                completed_at: None,
                cost_usd: item
                    .pointer("/usage/cost")
                    .or_else(|| response.and_then(|value| value.pointer("/usage/cost")))
                    .or_else(|| response.and_then(|value| value.pointer("/body/usage/cost")))
                    .and_then(parse_money),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use gateway_core::{
        BatchEndpoint, BatchStatus, Money4, OpenAiCompatEmptyTools, OpenAiCompatRouteCompatibility,
        ProviderBatchRequest, ProviderBatchRequestItem, ProviderBatchSubmission, ProviderError,
        ProviderRequestContext, RouteCompatibility,
    };
    use serde_json::{Map, json};
    use uuid::Uuid;

    use super::{
        parse_openai_result, parse_openai_result_line, parse_openrouter_results, parse_state,
        submission_is_unknown,
    };
    use crate::openai_compat::{
        OpenAiBatchConfig, OpenAiBatchDialect, OpenAiCompatConfig, OpenAiCompatProvider,
    };

    #[tokio::test]
    async fn invalid_openai_batch_item_is_rejected_before_upload() {
        let mut config = OpenAiCompatConfig::new(
            "openai-prod".to_string(),
            "http://127.0.0.1:1/v1".to_string(),
        );
        config.batch = OpenAiBatchConfig {
            dialect: OpenAiBatchDialect::OpenAi,
            base_url: None,
        };
        let provider = OpenAiCompatProvider::new(config).expect("provider");
        let request = ProviderBatchRequest {
            batch_id: Uuid::new_v4(),
            endpoint: BatchEndpoint::ChatCompletions,
            upstream_model: "gpt-5.6-sol".to_string(),
            items: vec![ProviderBatchRequestItem {
                custom_id: "invalid-tools".to_string(),
                body: json!({
                    "messages": [{"role": "user", "content": "hello"}],
                    "tools": [],
                    "tool_choice": "required"
                }),
            }],
            context: ProviderRequestContext {
                request_id: "request-1".to_string(),
                model_key: "fast".to_string(),
                provider_key: "openai-prod".to_string(),
                upstream_model: "gpt-5.6-sol".to_string(),
                extra_headers: Map::new(),
                extra_body: Map::new(),
                request_headers: BTreeMap::new(),
                compatibility: RouteCompatibility {
                    openai_compat: Some(OpenAiCompatRouteCompatibility {
                        empty_tools: OpenAiCompatEmptyTools::Omit,
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            },
        };

        match provider.submit_batch_impl(&request).await {
            ProviderBatchSubmission::NotSubmitted(ProviderError::InvalidRequest(message)) => {
                assert!(message.contains("when empty `tools` are omitted"));
            }
            _ => panic!("invalid batch item was not rejected before upload"),
        }
    }

    #[test]
    fn only_uncertain_create_failures_require_reconciliation() {
        assert!(submission_is_unknown(&ProviderError::Timeout));
        assert!(submission_is_unknown(&ProviderError::Transport(
            "reset".to_string()
        )));
        assert!(submission_is_unknown(&ProviderError::UpstreamHttp {
            status: 503,
            body: "unavailable".to_string(),
        }));
        assert!(!submission_is_unknown(&ProviderError::UpstreamHttp {
            status: 400,
            body: "invalid".to_string(),
        }));
    }

    #[test]
    fn openai_state_and_jsonl_result_preserve_counts_usage_and_cost() {
        let state = parse_state(&json!({
            "id": "batch_123",
            "status": "completed",
            "request_counts": {"total": 2, "completed": 1, "failed": 1},
            "completed_at": 1_700_000_000,
            "usage": {"cost": "0.0125"}
        }))
        .expect("state");
        assert_eq!(state.status, BatchStatus::Completed);
        assert_eq!(state.request_count, 2);
        assert_eq!(state.provider_cost_usd, Some(Money4::from_scaled(125)));

        let result = parse_openai_result(&json!({
            "custom_id": "row-1",
            "response": {
                "request_id": "request-1",
                "body": {"id": "response-1", "usage": {"input_tokens": 10}}
            },
            "error": null
        }))
        .expect("result");
        assert_eq!(result.custom_id, "row-1");
        assert_eq!(result.provider_request_id.as_deref(), Some("request-1"));
        assert_eq!(result.provider_usage, Some(json!({"input_tokens": 10})));

        let mut streamed = Vec::new();
        parse_openai_result_line(
            br#"{"custom_id":"row-2","response":{"body":{"id":"response-2"}},"error":null}"#,
            &mut streamed,
        )
        .expect("streamed result line");
        parse_openai_result_line(b"\r", &mut streamed).expect("blank line");
        assert_eq!(streamed.len(), 1);
        assert_eq!(streamed[0].custom_id, "row-2");
    }

    #[test]
    fn openai_non_success_response_is_a_failed_item() {
        let result = parse_openai_result(&json!({
            "custom_id": "row-rejected",
            "response": {
                "status_code": 429,
                "request_id": "request-rejected",
                "body": {
                    "error": {
                        "message": "Rate limit exceeded",
                        "type": "rate_limit_error"
                    }
                }
            },
            "error": null
        }))
        .expect("result");

        assert_eq!(
            result
                .error
                .as_ref()
                .and_then(|error| error["status_code"].as_u64()),
            Some(429)
        );
        assert_eq!(
            result
                .error
                .as_ref()
                .and_then(|error| error.pointer("/body/error/type"))
                .and_then(serde_json::Value::as_str),
            Some("rate_limit_error")
        );
    }

    #[test]
    fn openrouter_inline_results_keep_custom_ids_and_reported_costs() {
        let results = parse_openrouter_results(&json!({
            "requests": [{
                "custom_id": "row-1",
                "response": {"body": {"id": "response-1"}},
                "usage": {"cost": 0.0025}
            }]
        }));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].custom_id, "row-1");
        assert_eq!(results[0].cost_usd, Some(Money4::from_scaled(25)));
    }
}
