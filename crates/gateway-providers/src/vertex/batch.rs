use std::collections::BTreeSet;

use gateway_core::{
    BatchCapabilities, BatchEndpoint, BatchStatus, ChatCompletionsRequest, ProviderBatchRequest,
    ProviderBatchResult, ProviderBatchState, ProviderBatchSubmission, ProviderError,
    ProviderRequestContext, openai_chat_request_to_core,
};
use serde_json::{Value, json};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use super::{VertexProvider, map_google_request, normalize_google_response, parse_upstream_model};
use crate::http::map_reqwest_error;

impl VertexProvider {
    pub(super) fn batch_capabilities_impl(&self) -> BatchCapabilities {
        if self.config.batch.is_none() {
            BatchCapabilities::NONE
        } else {
            BatchCapabilities {
                chat_completions: true,
                responses: false,
                embeddings: false,
                cancel: true,
            }
        }
    }

    pub(super) async fn submit_batch_impl(
        &self,
        request: &ProviderBatchRequest,
    ) -> ProviderBatchSubmission {
        let plan = match self.prepare_vertex_batch(request).await {
            Ok(plan) => plan,
            Err(error) => return ProviderBatchSubmission::NotSubmitted(error),
        };
        let result = self
            .vertex_batch_json(
                reqwest::Method::POST,
                &self.batch_jobs_url(),
                Some(plan.request_body.clone()),
                &request.context,
            )
            .await
            .and_then(|value| parse_vertex_state(&value));
        match result {
            Ok(state) => ProviderBatchSubmission::Submitted(state),
            Err(error) if submission_is_unknown(&error) => {
                match self
                    .reconcile_vertex_batch(request.batch_id, &request.context)
                    .await
                {
                    Some(state) => ProviderBatchSubmission::Submitted(state),
                    None => ProviderBatchSubmission::SubmissionUnknown(error),
                }
            }
            Err(error) => {
                let _ = self.delete_input_table(&plan, &request.context).await;
                ProviderBatchSubmission::NotSubmitted(error)
            }
        }
    }

    async fn prepare_vertex_batch(
        &self,
        request: &ProviderBatchRequest,
    ) -> Result<VertexBatchPlan, ProviderError> {
        if request.endpoint != BatchEndpoint::ChatCompletions {
            return Err(ProviderError::InvalidRequest(
                "Vertex batch mode currently supports chat completions only".to_string(),
            ));
        }
        let (_, publisher, model_id) = parse_upstream_model(&request.upstream_model)?;
        if publisher != "google" {
            return Err(ProviderError::InvalidRequest(
                "Vertex batch mode currently supports Google publisher models only".to_string(),
            ));
        }
        let config = self.batch_config()?;
        validate_bigquery_project(&config.bigquery_project_id)?;
        validate_bigquery_identifier(&config.dataset, "dataset")?;
        let project = config.bigquery_project_id.clone();
        let dataset = config.dataset.clone();
        let table = format!("oceans_batch_{}", request.batch_id.simple());
        let output_table = format!("{table}_output");
        let plan = VertexBatchPlan {
            project,
            dataset,
            input_table: table.clone(),
            request_body: json!({
                "displayName": format!("oceans-batch-{}", request.batch_id),
                "model": format!("publishers/google/models/{model_id}"),
                "inputConfig": {
                    "instancesFormat": "bigquery",
                    "bigquerySource": {
                        "inputUri": format!("bq://{}.{}.{}", config.bigquery_project_id, config.dataset, table)
                    }
                },
                "outputConfig": {
                    "predictionsFormat": "bigquery",
                    "bigqueryDestination": {
                        "outputUri": format!("bq://{}.{}.{}", config.bigquery_project_id, config.dataset, output_table)
                    }
                }
            }),
        };
        if let Err(error) = self.create_clean_input_table(&plan, &request.context).await {
            return Err(
                match self.delete_input_table(&plan, &request.context).await {
                    Ok(()) => error,
                    Err(cleanup_error) => cleanup_error,
                },
            );
        }
        if let Err(error) = self
            .insert_input_rows(&plan.project, &plan.dataset, &plan.input_table, request)
            .await
        {
            return Err(
                match self.delete_input_table(&plan, &request.context).await {
                    Ok(()) => error,
                    Err(cleanup_error) => cleanup_error,
                },
            );
        }
        Ok(plan)
    }

    async fn create_clean_input_table(
        &self,
        plan: &VertexBatchPlan,
        context: &ProviderRequestContext,
    ) -> Result<(), ProviderError> {
        let result = self
            .create_input_table(&plan.project, &plan.dataset, &plan.input_table, context)
            .await;
        if result.as_ref().is_err_and(table_already_exists) {
            self.delete_input_table(plan, context).await?;
            return self
                .create_input_table(&plan.project, &plan.dataset, &plan.input_table, context)
                .await;
        }
        result
    }

    async fn delete_input_table(
        &self,
        plan: &VertexBatchPlan,
        context: &ProviderRequestContext,
    ) -> Result<(), ProviderError> {
        match self
            .bigquery_json(
                reqwest::Method::DELETE,
                &format!(
                    "projects/{}/datasets/{}/tables/{}",
                    plan.project, plan.dataset, plan.input_table
                ),
                None,
                context,
            )
            .await
        {
            Ok(_) | Err(ProviderError::UpstreamHttp { status: 404, .. }) => Ok(()),
            Err(error) => Err(error),
        }
    }

    async fn reconcile_vertex_batch(
        &self,
        batch_id: uuid::Uuid,
        context: &ProviderRequestContext,
    ) -> Option<ProviderBatchState> {
        let display_name = format!("oceans-batch-{batch_id}");
        let mut page_token = None;
        let mut seen_tokens = BTreeSet::new();
        loop {
            let mut url = url::Url::parse(&self.batch_jobs_url()).ok()?;
            {
                let mut query = url.query_pairs_mut();
                query.append_pair("pageSize", "100");
                if let Some(token) = page_token.as_deref() {
                    query.append_pair("pageToken", token);
                }
            }
            let value = self
                .vertex_batch_json(reqwest::Method::GET, url.as_str(), None, context)
                .await
                .ok()?;
            if let Some(state) = value
                .get("batchPredictionJobs")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .find(|job| {
                    job.get("displayName").and_then(Value::as_str) == Some(display_name.as_str())
                })
                .and_then(|job| parse_vertex_state(job).ok())
            {
                return Some(state);
            }
            let token = value.get("nextPageToken").and_then(Value::as_str)?;
            if token.is_empty() || !seen_tokens.insert(token.to_string()) {
                return None;
            }
            page_token = Some(token.to_string());
        }
    }

    pub(super) async fn inspect_batch_impl(
        &self,
        provider_batch_id: &str,
        context: &ProviderRequestContext,
    ) -> Result<ProviderBatchState, ProviderError> {
        parse_vertex_state(&self.load_vertex_batch(provider_batch_id, context).await?)
    }

    pub(super) async fn cancel_batch_impl(
        &self,
        provider_batch_id: &str,
        context: &ProviderRequestContext,
    ) -> Result<ProviderBatchState, ProviderError> {
        let url = format!("{}:cancel", self.vertex_resource_url(provider_batch_id));
        self.vertex_batch_json(reqwest::Method::POST, &url, Some(json!({})), context)
            .await?;
        self.inspect_batch_impl(provider_batch_id, context).await
    }

    pub(super) async fn batch_results_impl(
        &self,
        state: &ProviderBatchState,
        context: &ProviderRequestContext,
    ) -> Result<Vec<ProviderBatchResult>, ProviderError> {
        let job = self
            .load_vertex_batch(&state.provider_batch_id, context)
            .await?;
        let output = output_table(&job).ok_or_else(|| {
            ProviderError::Transport(
                "Vertex batch job omitted its BigQuery output table".to_string(),
            )
        })?;
        let (project, dataset, table) = parse_bigquery_table(output)?;
        validate_bigquery_project(project)?;
        validate_bigquery_identifier(dataset, "dataset")?;
        validate_bigquery_identifier(table, "table")?;
        self.set_table_expiration(project, dataset, table, context)
            .await?;
        let sql = format!(
            "SELECT custom_id, response, status FROM `{project}.{dataset}.{table}` ORDER BY custom_id"
        );
        let query = self.query_bigquery_rows(project, &sql, context).await?;
        parse_bigquery_results(&query, context)
    }

    async fn query_bigquery_rows(
        &self,
        project: &str,
        sql: &str,
        context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        const PAGE_SIZE: u64 = 10_000;
        const MAX_REQUESTS: usize = 240;

        let mut page = self
            .bigquery_json(
                reqwest::Method::POST,
                &format!("projects/{project}/queries"),
                Some(json!({
                    "query": sql,
                    "useLegacySql": false,
                    "maxResults": PAGE_SIZE,
                    "timeoutMs": 10_000
                })),
                context,
            )
            .await?;
        let mut rows = Vec::new();
        let mut requests = 1_usize;
        loop {
            if let Some(page_rows) = page.get_mut("rows").and_then(Value::as_array_mut) {
                rows.append(page_rows);
            }
            let job = page.get("jobReference").ok_or_else(|| {
                ProviderError::Transport("BigQuery query response omitted jobReference".to_string())
            })?;
            let job_id = job.get("jobId").and_then(Value::as_str).ok_or_else(|| {
                ProviderError::Transport("BigQuery query response omitted jobId".to_string())
            })?;
            let query_project = job
                .get("projectId")
                .and_then(Value::as_str)
                .unwrap_or(project);
            let location = job.get("location").and_then(Value::as_str);
            let page_token = page.get("pageToken").and_then(Value::as_str);
            let complete = page
                .get("jobComplete")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if complete && page_token.is_none() {
                break;
            }
            if requests >= MAX_REQUESTS {
                return Err(ProviderError::Timeout);
            }
            requests += 1;
            let mut url = url::Url::parse(&format!(
                "https://bigquery.googleapis.com/bigquery/v2/projects/{query_project}/queries/{job_id}"
            ))
            .map_err(|error| ProviderError::Transport(error.to_string()))?;
            {
                let mut query = url.query_pairs_mut();
                query
                    .append_pair("maxResults", &PAGE_SIZE.to_string())
                    .append_pair("timeoutMs", "10000");
                if let Some(location) = location {
                    query.append_pair("location", location);
                }
                if let Some(page_token) = page_token {
                    query.append_pair("pageToken", page_token);
                }
            }
            page = self
                .authenticated_json(reqwest::Method::GET, url.as_str(), None, context)
                .await?;
        }
        Ok(json!({"rows": rows}))
    }

    fn batch_config(&self) -> Result<&super::VertexBatchConfig, ProviderError> {
        self.config.batch.as_ref().ok_or_else(|| {
            ProviderError::NotImplemented("batch mode is not enabled for this provider".to_string())
        })
    }

    async fn create_input_table(
        &self,
        project: &str,
        dataset: &str,
        table: &str,
        context: &ProviderRequestContext,
    ) -> Result<(), ProviderError> {
        self.bigquery_json(
            reqwest::Method::POST,
            &format!("projects/{project}/datasets/{dataset}/tables"),
            Some(json!({
                "tableReference": {"projectId": project, "datasetId": dataset, "tableId": table},
                "expirationTime": ((OffsetDateTime::now_utc() + time::Duration::days(30)).unix_timestamp() * 1_000).to_string(),
                "schema": {"fields": [
                    {"name": "custom_id", "type": "STRING", "mode": "REQUIRED"},
                    {"name": "request", "type": "JSON", "mode": "REQUIRED"}
                ]}
            })),
            context,
        )
        .await?;
        Ok(())
    }

    async fn set_table_expiration(
        &self,
        project: &str,
        dataset: &str,
        table: &str,
        context: &ProviderRequestContext,
    ) -> Result<(), ProviderError> {
        self.bigquery_json(
            reqwest::Method::PATCH,
            &format!("projects/{project}/datasets/{dataset}/tables/{table}"),
            Some(json!({
                "expirationTime": ((OffsetDateTime::now_utc() + time::Duration::days(30)).unix_timestamp() * 1_000).to_string()
            })),
            context,
        )
        .await?;
        Ok(())
    }

    async fn insert_input_rows(
        &self,
        project: &str,
        dataset: &str,
        table: &str,
        request: &ProviderBatchRequest,
    ) -> Result<(), ProviderError> {
        // BigQuery insertAll requests have a 10 MB request limit. Leave room for
        // the surrounding JSON envelope and for serialization differences.
        const MAX_CHUNK_BYTES: usize = 5 * 1024 * 1024;
        const MAX_CHUNK_ROWS: usize = 5_000;

        let mut rows = Vec::with_capacity(request.items.len().min(MAX_CHUNK_ROWS));
        let mut chunk_bytes: usize = 0;
        for item in &request.items {
            let mut body = item.body.clone();
            body.as_object_mut()
                .ok_or_else(|| {
                    ProviderError::InvalidRequest(format!(
                        "batch item `{}` body must be a JSON object",
                        item.custom_id
                    ))
                })?
                .insert(
                    "model".to_string(),
                    Value::String(request.upstream_model.clone()),
                );
            let openai: ChatCompletionsRequest = serde_json::from_value(body).map_err(|error| {
                ProviderError::InvalidRequest(format!(
                    "batch item `{}` is not a valid chat request: {error}",
                    item.custom_id
                ))
            })?;
            let mapped = map_google_request(
                &openai_chat_request_to_core(&openai),
                &request.context,
                false,
            )?;
            let row = json!({
                "insertId": item.custom_id,
                "json": {"custom_id": item.custom_id, "request": mapped}
            });
            let row_bytes = serde_json::to_vec(&row)
                .map_err(|error| ProviderError::InvalidRequest(error.to_string()))?
                .len();
            if row_bytes > MAX_CHUNK_BYTES {
                return Err(ProviderError::InvalidRequest(format!(
                    "batch item `{}` is too large for Vertex BigQuery ingestion",
                    item.custom_id
                )));
            }
            if !rows.is_empty()
                && (rows.len() >= MAX_CHUNK_ROWS
                    || chunk_bytes.saturating_add(row_bytes) > MAX_CHUNK_BYTES)
            {
                self.insert_input_chunk(project, dataset, table, &rows, &request.context)
                    .await?;
                rows.clear();
                chunk_bytes = 0;
            }
            chunk_bytes = chunk_bytes.saturating_add(row_bytes);
            rows.push(row);
        }
        if !rows.is_empty() {
            self.insert_input_chunk(project, dataset, table, &rows, &request.context)
                .await?;
        }
        Ok(())
    }

    async fn insert_input_chunk(
        &self,
        project: &str,
        dataset: &str,
        table: &str,
        rows: &[Value],
        context: &ProviderRequestContext,
    ) -> Result<(), ProviderError> {
        let response = self
            .bigquery_json(
                reqwest::Method::POST,
                &format!("projects/{project}/datasets/{dataset}/tables/{table}/insertAll"),
                Some(json!({"rows": rows})),
                context,
            )
            .await?;
        if response
            .get("insertErrors")
            .and_then(Value::as_array)
            .is_some_and(|errors| !errors.is_empty())
        {
            return Err(ProviderError::InvalidRequest(format!(
                "BigQuery rejected Vertex batch input rows: {response}"
            )));
        }
        Ok(())
    }

    async fn load_vertex_batch(
        &self,
        provider_batch_id: &str,
        context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        self.vertex_batch_json(
            reqwest::Method::GET,
            &self.vertex_resource_url(provider_batch_id),
            None,
            context,
        )
        .await
    }

    async fn vertex_batch_json(
        &self,
        method: reqwest::Method,
        url: &str,
        body: Option<Value>,
        context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        self.authenticated_json(method, url, body, context).await
    }

    async fn bigquery_json(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
        context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        self.authenticated_json(
            method,
            &format!("https://bigquery.googleapis.com/bigquery/v2/{path}"),
            body,
            context,
        )
        .await
    }

    async fn authenticated_json(
        &self,
        method: reqwest::Method,
        url: &str,
        body: Option<Value>,
        context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        let token = self.access_token_source.token().await?;
        let mut builder = self
            .apply_request_headers(self.client.request(method, url).bearer_auth(token), context);
        if let Some(body) = body {
            builder = builder.json(&body);
        }
        let response = builder.send().await.map_err(map_reqwest_error)?;
        let status = response.status();
        let text = response.text().await.map_err(map_reqwest_error)?;
        if !status.is_success() {
            return Err(ProviderError::UpstreamHttp {
                status: status.as_u16(),
                body: text,
            });
        }
        if text.is_empty() {
            return Ok(json!({}));
        }
        serde_json::from_str(&text).map_err(|error| {
            ProviderError::Transport(format!("invalid Google batch JSON: {error}"))
        })
    }

    fn batch_jobs_url(&self) -> String {
        format!(
            "{}/v1/projects/{}/locations/{}/batchPredictionJobs",
            api_base(&self.config.api_host),
            self.config.project_id,
            self.config.location
        )
    }

    fn vertex_resource_url(&self, provider_batch_id: &str) -> String {
        if provider_batch_id.starts_with("http://") || provider_batch_id.starts_with("https://") {
            provider_batch_id.to_string()
        } else {
            format!("{}/v1/{provider_batch_id}", api_base(&self.config.api_host))
        }
    }
}

struct VertexBatchPlan {
    project: String,
    dataset: String,
    input_table: String,
    request_body: Value,
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

fn table_already_exists(error: &ProviderError) -> bool {
    matches!(error, ProviderError::UpstreamHttp { status: 409, .. })
}

fn api_base(host: &str) -> String {
    let host = host.trim_end_matches('/');
    if host.starts_with("http://") || host.starts_with("https://") {
        host.to_string()
    } else {
        format!("https://{host}")
    }
}

fn parse_vertex_state(value: &Value) -> Result<ProviderBatchState, ProviderError> {
    let name = value.get("name").and_then(Value::as_str).ok_or_else(|| {
        ProviderError::Transport("Vertex batch response omitted name".to_string())
    })?;
    let status = match value
        .get("state")
        .and_then(Value::as_str)
        .unwrap_or("JOB_STATE_PENDING")
    {
        "JOB_STATE_PENDING" | "JOB_STATE_QUEUED" => BatchStatus::Validating,
        "JOB_STATE_RUNNING" | "JOB_STATE_UPDATING" => BatchStatus::InProgress,
        "JOB_STATE_SUCCEEDED" => BatchStatus::Completed,
        "JOB_STATE_FAILED" => BatchStatus::Failed,
        "JOB_STATE_EXPIRED" => BatchStatus::Expired,
        "JOB_STATE_CANCELLING" => BatchStatus::Cancelling,
        "JOB_STATE_CANCELLED" => BatchStatus::Cancelled,
        other => {
            return Err(ProviderError::Transport(format!(
                "unknown Vertex batch state `{other}`"
            )));
        }
    };
    let completed = value
        .pointer("/completionStats/successfulCount")
        .and_then(integer)
        .unwrap_or(0);
    let failed = value
        .pointer("/completionStats/failedCount")
        .and_then(integer)
        .unwrap_or(0);
    Ok(ProviderBatchState {
        provider_batch_id: name.to_string(),
        status,
        request_count: completed.saturating_add(failed),
        completed_count: completed,
        failed_count: failed,
        provider_usage: value.get("completionStats").cloned(),
        provider_cost_usd: None,
        error: value.get("error").filter(|error| !error.is_null()).cloned(),
        submitted_at: rfc3339(value.get("createTime")),
        completed_at: status
            .is_terminal()
            .then(|| rfc3339(value.get("updateTime")))
            .flatten(),
    })
}

fn integer(value: &Value) -> Option<i64> {
    value.as_i64().or_else(|| value.as_str()?.parse().ok())
}

fn rfc3339(value: Option<&Value>) -> Option<OffsetDateTime> {
    OffsetDateTime::parse(value?.as_str()?, &Rfc3339).ok()
}

fn output_table(job: &Value) -> Option<&str> {
    job.pointer("/outputInfo/bigqueryOutputTable")
        .or_else(|| job.pointer("/outputInfo/bigQueryOutputTable"))
        .or_else(|| job.pointer("/outputConfig/bigqueryDestination/outputUri"))
        .and_then(Value::as_str)
}

fn parse_bigquery_table(raw: &str) -> Result<(&str, &str, &str), ProviderError> {
    let normalized = raw.trim_start_matches("bq://");
    let parts = normalized.split('.').collect::<Vec<_>>();
    if let [project, dataset, table] = parts.as_slice() {
        return Ok((project, dataset, table));
    }
    let path = normalized.trim_start_matches("projects/");
    let parts = path.split('/').collect::<Vec<_>>();
    if let [project, "datasets", dataset, "tables", table] = parts.as_slice() {
        return Ok((project, dataset, table));
    }
    Err(ProviderError::Transport(format!(
        "invalid Vertex BigQuery output table `{raw}`"
    )))
}

fn validate_bigquery_identifier(value: &str, label: &str) -> Result<(), ProviderError> {
    if value.is_empty()
        || !value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
    {
        return Err(ProviderError::InvalidRequest(format!(
            "Vertex batch {label} must contain only letters, numbers, and underscores"
        )));
    }
    Ok(())
}

fn validate_bigquery_project(value: &str) -> Result<(), ProviderError> {
    if value.is_empty()
        || !value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | ':')
        })
    {
        return Err(ProviderError::InvalidRequest(
            "Vertex batch project contains invalid characters".to_string(),
        ));
    }
    Ok(())
}

fn parse_bigquery_results(
    value: &Value,
    context: &ProviderRequestContext,
) -> Result<Vec<ProviderBatchResult>, ProviderError> {
    value
        .get("rows")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|row| {
            let fields = row.get("f").and_then(Value::as_array).ok_or_else(|| {
                ProviderError::Transport("BigQuery batch row omitted fields".to_string())
            })?;
            let custom_id = field(fields, 0).and_then(Value::as_str).ok_or_else(|| {
                ProviderError::Transport("BigQuery batch row omitted custom_id".to_string())
            })?;
            let response = parse_json_field(field(fields, 1))?;
            let status = parse_json_field(field(fields, 2))?;
            let error = status
                .as_ref()
                .filter(|status| {
                    !status.is_null()
                        && status.get("code").and_then(Value::as_i64).unwrap_or(0) != 0
                })
                .cloned();
            let normalized = response
                .as_ref()
                .map(|response| normalize_google_response(response, context));
            Ok(ProviderBatchResult {
                custom_id: custom_id.to_string(),
                provider_usage: normalized
                    .as_ref()
                    .and_then(|response| response.get("usage"))
                    .cloned(),
                response_body: normalized,
                error,
                provider_request_id: None,
                completed_at: None,
                cost_usd: None,
            })
        })
        .collect()
}

fn field(fields: &[Value], index: usize) -> Option<&Value> {
    fields.get(index)?.get("v")
}

fn parse_json_field(value: Option<&Value>) -> Result<Option<Value>, ProviderError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(raw)) => serde_json::from_str(raw).map(Some).map_err(|error| {
            ProviderError::Transport(format!("invalid BigQuery JSON field: {error}"))
        }),
        Some(value) => Ok(Some(value.clone())),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use gateway_core::{BatchStatus, ProviderError, ProviderRequestContext, RouteCompatibility};
    use serde_json::{Map, json};

    use super::{
        parse_bigquery_results, parse_bigquery_table, parse_vertex_state, submission_is_unknown,
        table_already_exists, validate_bigquery_identifier, validate_bigquery_project,
    };

    #[test]
    fn vertex_submission_certainty_changes_at_the_job_create_boundary() {
        assert!(submission_is_unknown(&ProviderError::Transport(
            "connection closed".to_string(),
        )));
        assert!(!submission_is_unknown(&ProviderError::InvalidRequest(
            "unsupported model".to_string(),
        )));
    }

    #[test]
    fn vertex_staging_table_conflict_requires_replacement() {
        assert!(table_already_exists(&ProviderError::UpstreamHttp {
            status: 409,
            body: "table exists".to_string(),
        }));
    }

    #[test]
    fn vertex_state_maps_terminal_counts_and_resource_name() {
        let state = parse_vertex_state(&json!({
            "name": "projects/p/locations/us/batchPredictionJobs/123",
            "state": "JOB_STATE_SUCCEEDED",
            "createTime": "2026-08-16T10:00:00Z",
            "updateTime": "2026-08-16T11:00:00Z",
            "completionStats": {"successfulCount": "9", "failedCount": "1"}
        }))
        .expect("state");
        assert_eq!(state.status, BatchStatus::Completed);
        assert_eq!(state.request_count, 10);
        assert_eq!(state.completed_count, 9);
        assert!(state.completed_at.is_some());
    }

    #[test]
    fn vertex_bigquery_result_uses_custom_id_and_normalizes_response() {
        let context = ProviderRequestContext {
            request_id: "batch-1".to_string(),
            model_key: "analysis".to_string(),
            provider_key: "vertex".to_string(),
            upstream_model: "projects/p/locations/us/publishers/google/models/gemini-test"
                .to_string(),
            owner_user_id: None,
            extra_headers: Map::new(),
            extra_body: Map::new(),
            request_headers: BTreeMap::new(),
            compatibility: RouteCompatibility::default(),
        };
        let results = parse_bigquery_results(
            &json!({"rows": [{"f": [
                {"v": "row-1"},
                {"v": "{\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"hello\"}]},\"finishReason\":\"STOP\"}],\"usageMetadata\":{\"promptTokenCount\":4,\"candidatesTokenCount\":1,\"totalTokenCount\":5}}"},
                {"v": null}
            ]}]}),
            &context,
        )
        .expect("results");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].custom_id, "row-1");
        assert!(results[0].response_body.is_some());
    }

    #[test]
    fn vertex_bigquery_table_parser_accepts_uri_and_resource_forms() {
        assert_eq!(
            parse_bigquery_table("bq://project.dataset.table").expect("uri"),
            ("project", "dataset", "table")
        );
        assert_eq!(
            parse_bigquery_table("projects/project/datasets/dataset/tables/table")
                .expect("resource"),
            ("project", "dataset", "table")
        );
        assert!(validate_bigquery_project("my-project:billing").is_ok());
        assert!(validate_bigquery_project("project` UNION SELECT 1").is_err());
        assert!(validate_bigquery_identifier("safe_table", "table").is_ok());
        assert!(validate_bigquery_identifier("unsafe`table", "table").is_err());
    }
}
