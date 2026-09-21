use std::{collections::HashMap, sync::Arc, time::Duration};

use async_trait::async_trait;
use gateway_core::{
    BenchmarkCatalogRepository, BenchmarkSyncState, GatewayError, ModelBenchmarkBinding,
    ModelBenchmarkScore,
};
use reqwest::{Client, StatusCode};
use serde::Deserialize;
use serde_json::Number;
use time::OffsetDateTime;

const ARTIFICIAL_ANALYSIS_SOURCE: &str = "artificial_analysis";
const ARTIFICIAL_ANALYSIS_FREE_MODELS_URL: &str =
    "https://artificialanalysis.ai/api/v2/language/models/free";
pub const DEFAULT_BENCHMARK_CATALOG_REFRESH_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
const INTELLIGENCE_INDEX_METRIC_KEY: &str = "artificial_analysis_intelligence_index";
const INTELLIGENCE_INDEX_LABEL: &str = "Artificial Analysis Intelligence Index";
const INTELLIGENCE_INDEX_UNIT: &str = "index_points";
const MAX_PAGES: u32 = 100;

#[async_trait]
pub trait BenchmarkCatalogRefresh: Send + Sync {
    async fn refresh_if_stale(&self) -> Result<(), GatewayError>;
    async fn refresh_now(&self) -> Result<(), GatewayError>;
}

#[derive(Clone)]
pub struct BenchmarkCatalog<R> {
    repo: Arc<R>,
    client: Client,
    api_key: String,
    source_url: String,
    refresh_interval: Duration,
}

impl<R> BenchmarkCatalog<R>
where
    R: BenchmarkCatalogRepository + Send + Sync + 'static,
{
    #[must_use]
    pub fn new(repo: Arc<R>, api_key: String) -> Self {
        Self::with_options(
            repo,
            api_key,
            ARTIFICIAL_ANALYSIS_FREE_MODELS_URL.to_string(),
            DEFAULT_BENCHMARK_CATALOG_REFRESH_INTERVAL,
        )
    }

    #[must_use]
    fn with_options(
        repo: Arc<R>,
        api_key: String,
        source_url: String,
        refresh_interval: Duration,
    ) -> Self {
        Self {
            repo,
            client: benchmark_http_client(),
            api_key,
            source_url,
            refresh_interval,
        }
    }

    pub async fn refresh_if_stale(&self) -> Result<(), GatewayError> {
        let now = OffsetDateTime::now_utc();
        if self
            .repo
            .get_benchmark_sync_state(ARTIFICIAL_ANALYSIS_SOURCE)
            .await?
            .is_some_and(|state| {
                now.unix_timestamp()
                    .saturating_sub(state.last_successful_refresh_at.unix_timestamp())
                    < self.refresh_interval.as_secs() as i64
            })
        {
            return Ok(());
        }
        self.refresh_at(now).await
    }

    pub async fn refresh_now(&self) -> Result<(), GatewayError> {
        self.refresh_at(OffsetDateTime::now_utc()).await
    }

    async fn refresh_at(&self, fetched_at: OffsetDateTime) -> Result<(), GatewayError> {
        let bindings = self
            .repo
            .list_model_benchmark_bindings(ARTIFICIAL_ANALYSIS_SOURCE)
            .await?;
        if bindings.is_empty() {
            return Ok(());
        }
        let snapshot = self.fetch_all_pages().await?;
        let benchmark_version = snapshot.intelligence_index_version.to_string();
        let scores = project_intelligence_scores(
            &bindings,
            snapshot.models,
            &benchmark_version,
            fetched_at,
        )?;
        let state = BenchmarkSyncState {
            source: ARTIFICIAL_ANALYSIS_SOURCE.to_string(),
            benchmark_version,
            last_successful_refresh_at: fetched_at,
            updated_at: fetched_at,
        };
        self.repo
            .replace_model_benchmark_scores(&scores, &state)
            .await?;
        Ok(())
    }

    async fn fetch_all_pages(&self) -> Result<ArtificialAnalysisSnapshot, GatewayError> {
        let mut page = 1;
        let mut version = None;
        let mut expected_total_pages = None;
        let mut models = HashMap::new();

        loop {
            if page > MAX_PAGES {
                return Err(GatewayError::Internal(format!(
                    "Artificial Analysis response exceeded {MAX_PAGES} pages"
                )));
            }
            let response = self
                .client
                .get(&self.source_url)
                .header("x-api-key", &self.api_key)
                .query(&[("page", page)])
                .send()
                .await
                .map_err(|error| {
                    GatewayError::Internal(format!(
                        "Artificial Analysis benchmark refresh request failed: {error}"
                    ))
                })?;
            if response.status() != StatusCode::OK {
                return Err(GatewayError::Internal(format!(
                    "Artificial Analysis benchmark refresh failed with HTTP {}",
                    response.status().as_u16()
                )));
            }
            let response: FreeModelsResponse = response.json().await.map_err(|error| {
                GatewayError::Internal(format!(
                    "Artificial Analysis benchmark response was invalid: {error}"
                ))
            })?;
            validate_page(&response, page, expected_total_pages)?;

            match &version {
                Some(current) if current != &response.intelligence_index_version => {
                    return Err(GatewayError::Internal(
                        "Artificial Analysis benchmark version changed between pages".to_string(),
                    ));
                }
                None => version = Some(response.intelligence_index_version.clone()),
                _ => {}
            }
            expected_total_pages = Some(response.pagination.total_pages);
            for model in response.data {
                let model_id = model.id.clone();
                if models.insert(model_id.clone(), model).is_some() {
                    return Err(GatewayError::Internal(format!(
                        "Artificial Analysis returned duplicate model id `{model_id}`"
                    )));
                }
            }

            if !response.pagination.has_more {
                break;
            }
            page += 1;
        }

        Ok(ArtificialAnalysisSnapshot {
            intelligence_index_version: version.ok_or_else(|| {
                GatewayError::Internal(
                    "Artificial Analysis response omitted the index version".to_string(),
                )
            })?,
            models,
        })
    }
}

#[async_trait]
impl<R> BenchmarkCatalogRefresh for BenchmarkCatalog<R>
where
    R: BenchmarkCatalogRepository + Send + Sync + 'static,
{
    async fn refresh_if_stale(&self) -> Result<(), GatewayError> {
        Self::refresh_if_stale(self).await
    }

    async fn refresh_now(&self) -> Result<(), GatewayError> {
        Self::refresh_now(self).await
    }
}

fn validate_page(
    response: &FreeModelsResponse,
    requested_page: u32,
    expected_total_pages: Option<u32>,
) -> Result<(), GatewayError> {
    let pagination = &response.pagination;
    let total_pages_is_valid = pagination.total_pages > 0 || response.data.is_empty();
    let final_page_is_valid = pagination.has_more || pagination.total_pages <= requested_page;
    if pagination.page != requested_page
        || pagination.page_size == 0
        || !total_pages_is_valid
        || !final_page_is_valid
        || pagination.has_more && requested_page >= pagination.total_pages
        || expected_total_pages.is_some_and(|expected| expected != pagination.total_pages)
    {
        return Err(GatewayError::Internal(format!(
            "Artificial Analysis returned invalid pagination for page {requested_page}"
        )));
    }
    Ok(())
}

fn project_intelligence_scores(
    bindings: &[ModelBenchmarkBinding],
    models: HashMap<String, FreeModel>,
    benchmark_version: &str,
    fetched_at: OffsetDateTime,
) -> Result<Vec<ModelBenchmarkScore>, GatewayError> {
    let mut scores = Vec::with_capacity(bindings.len());
    for binding in bindings {
        let Some(model) = models.get(&binding.source_model_id) else {
            continue;
        };
        let Some(value) = model.evaluations.artificial_analysis_intelligence_index else {
            continue;
        };
        if !value.is_finite() {
            return Err(GatewayError::Internal(format!(
                "Artificial Analysis returned a non-finite score for model `{}`",
                model.id
            )));
        }
        let source_url = format!("https://artificialanalysis.ai/models/{}", model.slug);
        url::Url::parse(&source_url).map_err(|error| {
            GatewayError::Internal(format!(
                "Artificial Analysis returned an invalid model slug `{}`: {error}",
                model.slug
            ))
        })?;
        scores.push(ModelBenchmarkScore {
            model_id: binding.model_id,
            metric_key: INTELLIGENCE_INDEX_METRIC_KEY.to_string(),
            label: INTELLIGENCE_INDEX_LABEL.to_string(),
            value,
            unit: INTELLIGENCE_INDEX_UNIT.to_string(),
            benchmark_version: benchmark_version.to_string(),
            source: ARTIFICIAL_ANALYSIS_SOURCE.to_string(),
            source_model_id: binding.source_model_id.clone(),
            source_url,
            fetched_at,
        });
    }
    Ok(scores)
}

fn benchmark_http_client() -> Client {
    Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(30))
        .build()
        .expect("benchmark catalog HTTP client configuration must be valid")
}

struct ArtificialAnalysisSnapshot {
    intelligence_index_version: Number,
    models: HashMap<String, FreeModel>,
}

#[derive(Debug, Deserialize)]
struct FreeModelsResponse {
    intelligence_index_version: Number,
    pagination: Pagination,
    data: Vec<FreeModel>,
}

#[derive(Debug, Deserialize)]
struct Pagination {
    page: u32,
    page_size: u32,
    total_pages: u32,
    has_more: bool,
}

#[derive(Debug, Deserialize)]
struct FreeModel {
    id: String,
    slug: String,
    evaluations: FreeEvaluations,
}

#[derive(Debug, Deserialize)]
struct FreeEvaluations {
    artificial_analysis_intelligence_index: Option<f64>,
}

#[cfg(test)]
mod tests;
