use std::{sync::Arc, time::Duration};

#[cfg(any(test, debug_assertions))]
use std::{collections::BTreeMap, sync::Mutex};

use anyhow::Context;
use opentelemetry::{
    KeyValue, global,
    metrics::{Counter, Histogram},
    trace::TracerProvider as _,
};
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{
    Resource,
    metrics::{PeriodicReader, SdkMeterProvider},
    trace::SdkTracerProvider,
};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

use crate::config::ServerConfig;

#[derive(Clone)]
pub struct GatewayMetrics {
    requests: Counter<u64>,
    request_duration: Histogram<f64>,
    provider_attempts: Counter<u64>,
    fallbacks: Counter<u64>,
    tokens: Counter<u64>,
    cost_usd: Counter<f64>,
    usage_records: Counter<u64>,
    usage_record_failures: Counter<u64>,
    #[cfg(any(test, debug_assertions))]
    test_counters: Arc<TestMetricCounters>,
}

pub struct ObservabilityGuard {
    pub metrics: Arc<GatewayMetrics>,
    tracer_provider: Option<SdkTracerProvider>,
    meter_provider: Option<SdkMeterProvider>,
}

#[derive(Debug, Clone)]
pub struct ChatMetricLabels<'a> {
    pub requested_model: &'a str,
    pub resolved_model: &'a str,
    pub provider_key: &'a str,
    pub stream: bool,
}

#[derive(Debug, Clone)]
pub struct ChatRequestMetric<'a> {
    pub labels: ChatMetricLabels<'a>,
    pub status_code: i64,
    pub outcome: &'a str,
    pub fallback_used: bool,
    pub latency_seconds: f64,
}

#[cfg(any(test, debug_assertions))]
#[derive(Debug, Default)]
struct TestMetricCounters {
    requests: Mutex<u64>,
    provider_attempts: Mutex<u64>,
    request_outcomes: Mutex<BTreeMap<String, u64>>,
}

#[cfg(any(test, debug_assertions))]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TestMetricSnapshot {
    pub requests: u64,
    pub provider_attempts: u64,
    pub request_outcomes: BTreeMap<String, u64>,
}

impl Default for GatewayMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl GatewayMetrics {
    #[must_use]
    pub fn new() -> Self {
        let meter = global::meter("gateway");
        Self {
            requests: meter
                .u64_counter("gateway.chat.requests")
                .with_description("Total chat completion requests")
                .build(),
            request_duration: meter
                .f64_histogram("gateway.chat.request.duration")
                .with_unit("s")
                .with_description("Chat completion request duration in seconds")
                .build(),
            provider_attempts: meter
                .u64_counter("gateway.chat.provider.attempts")
                .with_description("Provider attempts for chat completions")
                .build(),
            fallbacks: meter
                .u64_counter("gateway.chat.fallbacks")
                .with_description("Successful chat requests that required fallback")
                .build(),
            tokens: meter
                .u64_counter("gateway.chat.tokens")
                .with_description("Chat token totals by token type")
                .build(),
            cost_usd: meter
                .f64_counter("gateway.chat.cost.usd")
                .with_description("Operational chat request cost totals in USD")
                .build(),
            usage_records: meter
                .u64_counter("gateway.chat.usage_records")
                .with_description("Chat usage records by pricing status")
                .build(),
            usage_record_failures: meter
                .u64_counter("gateway.chat.usage_record_failures")
                .with_description("Post-success chat usage record failures")
                .build(),
            #[cfg(any(test, debug_assertions))]
            test_counters: Arc::new(TestMetricCounters::default()),
        }
    }

    pub fn record_provider_attempt(&self, labels: &ChatMetricLabels<'_>) {
        self.provider_attempts.add(1, &base_attrs(labels));
        #[cfg(any(test, debug_assertions))]
        {
            let mut attempts = self
                .test_counters
                .provider_attempts
                .lock()
                .expect("provider attempts lock");
            *attempts += 1;
        }
    }

    pub fn record_chat_request(&self, metric: &ChatRequestMetric<'_>) {
        let mut attrs = base_attrs(&metric.labels);
        attrs.push(KeyValue::new("http.route", "/v1/chat/completions"));
        attrs.push(KeyValue::new("http.method", "POST"));
        attrs.push(KeyValue::new(
            "http.response.status_code",
            metric.status_code,
        ));
        attrs.push(KeyValue::new("outcome", metric.outcome.to_string()));
        attrs.push(KeyValue::new("fallback_used", metric.fallback_used));

        self.requests.add(1, &attrs);
        self.request_duration.record(metric.latency_seconds, &attrs);
        #[cfg(any(test, debug_assertions))]
        {
            let mut requests = self.test_counters.requests.lock().expect("requests lock");
            *requests += 1;
            let mut outcomes = self
                .test_counters
                .request_outcomes
                .lock()
                .expect("request outcomes lock");
            *outcomes.entry(metric.outcome.to_string()).or_default() += 1;
        }

        if metric.fallback_used && metric.outcome == "success" {
            self.fallbacks.add(1, &base_attrs(&metric.labels));
        }
    }

    pub fn record_usage(
        &self,
        labels: &ChatMetricLabels<'_>,
        pricing_status: &str,
        prompt_tokens: Option<i64>,
        completion_tokens: Option<i64>,
        total_tokens: Option<i64>,
        cost_usd: Option<f64>,
    ) {
        let mut attrs = base_attrs(labels);
        attrs.push(KeyValue::new("pricing_status", pricing_status.to_string()));
        self.usage_records.add(1, &attrs);

        for (token_type, value) in [
            ("prompt", prompt_tokens),
            ("completion", completion_tokens),
            ("total", total_tokens),
        ] {
            if let Some(value) = value.and_then(|value| u64::try_from(value).ok()) {
                let mut token_attrs = attrs.clone();
                token_attrs.push(KeyValue::new("token_type", token_type));
                self.tokens.add(value, &token_attrs);
            }
        }

        if let Some(cost_usd) = cost_usd
            && cost_usd > 0.0
        {
            self.cost_usd.add(cost_usd, &attrs);
        }
    }

    pub fn record_usage_record_failure(&self, labels: &ChatMetricLabels<'_>, operation: &str) {
        let mut attrs = base_attrs(labels);
        attrs.push(KeyValue::new("operation", operation.to_string()));
        self.usage_record_failures.add(1, &attrs);
    }

    #[cfg(any(test, debug_assertions))]
    pub fn test_snapshot(&self) -> TestMetricSnapshot {
        TestMetricSnapshot {
            requests: *self.test_counters.requests.lock().expect("requests lock"),
            provider_attempts: *self
                .test_counters
                .provider_attempts
                .lock()
                .expect("provider attempts lock"),
            request_outcomes: self
                .test_counters
                .request_outcomes
                .lock()
                .expect("request outcomes lock")
                .clone(),
        }
    }
}

fn base_attrs(labels: &ChatMetricLabels<'_>) -> Vec<KeyValue> {
    vec![
        KeyValue::new("requested_model", labels.requested_model.to_string()),
        KeyValue::new("resolved_model", labels.resolved_model.to_string()),
        KeyValue::new("provider", labels.provider_key.to_string()),
        KeyValue::new("stream", labels.stream),
    ]
}

pub fn init_observability(config: &ServerConfig) -> anyhow::Result<ObservabilityGuard> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        "gateway=info,gateway_core=info,gateway_service=info,gateway_store=info,gateway_providers=info"
            .parse()
            .expect("default tracing filter should parse")
    });

    let resource = Resource::builder_empty()
        .with_attributes([
            KeyValue::new("service.name", "oceans-llm-gateway"),
            KeyValue::new("service.namespace", "oceans-llm"),
            KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
        ])
        .build();

    let tracer_provider = if let Some(endpoint) = &config.otel_endpoint {
        let exporter = opentelemetry_otlp::SpanExporter::builder()
            .with_tonic()
            .with_endpoint(endpoint)
            .build()
            .context("failed constructing OTLP span exporter")?;

        let tracer_provider = SdkTracerProvider::builder()
            .with_resource(resource.clone())
            .with_batch_exporter(exporter)
            .build();
        let tracer = tracer_provider.tracer("gateway");
        global::set_tracer_provider(tracer_provider.clone());

        init_subscriber(
            filter,
            config.log_format.as_str(),
            Some(tracing_opentelemetry::layer().with_tracer(tracer)),
        );

        Some(tracer_provider)
    } else {
        init_subscriber(
            filter,
            config.log_format.as_str(),
            Option::<
                tracing_opentelemetry::OpenTelemetryLayer<
                    tracing_subscriber::Registry,
                    opentelemetry_sdk::trace::Tracer,
                >,
            >::None,
        );
        None
    };

    let meter_provider = if let Some(endpoint) = config
        .otel_metrics_endpoint
        .as_ref()
        .or(config.otel_endpoint.as_ref())
    {
        let exporter = opentelemetry_otlp::MetricExporter::builder()
            .with_tonic()
            .with_endpoint(endpoint)
            .build()
            .context("failed constructing OTLP metric exporter")?;
        let reader = PeriodicReader::builder(exporter)
            .with_interval(Duration::from_secs(config.otel_export_interval_secs))
            .build();
        let meter_provider = SdkMeterProvider::builder()
            .with_resource(resource)
            .with_reader(reader)
            .build();
        global::set_meter_provider(meter_provider.clone());
        Some(meter_provider)
    } else {
        None
    };

    Ok(ObservabilityGuard {
        metrics: Arc::new(GatewayMetrics::new()),
        tracer_provider,
        meter_provider,
    })
}

impl ObservabilityGuard {
    pub fn shutdown(self) -> anyhow::Result<()> {
        if let Some(meter_provider) = self.meter_provider {
            meter_provider
                .shutdown()
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        }
        if let Some(tracer_provider) = self.tracer_provider {
            tracer_provider
                .shutdown()
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        }
        Ok(())
    }
}

fn init_subscriber(
    filter: EnvFilter,
    log_format: &str,
    telemetry_layer: Option<
        tracing_opentelemetry::OpenTelemetryLayer<
            tracing_subscriber::Registry,
            opentelemetry_sdk::trace::Tracer,
        >,
    >,
) {
    match (log_format, telemetry_layer) {
        ("json", Some(telemetry_layer)) => tracing_subscriber::registry()
            .with(telemetry_layer)
            .with(filter)
            .with(tracing_subscriber::fmt::layer().json())
            .init(),
        ("json", None) => tracing_subscriber::registry()
            .with(filter)
            .with(tracing_subscriber::fmt::layer().json())
            .init(),
        (_, Some(telemetry_layer)) => tracing_subscriber::registry()
            .with(telemetry_layer)
            .with(filter)
            .with(tracing_subscriber::fmt::layer())
            .init(),
        (_, None) => tracing_subscriber::registry()
            .with(filter)
            .with(tracing_subscriber::fmt::layer())
            .init(),
    }
}
