use anyhow::Context;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::WithExportConfig;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

use crate::config::ServerConfig;

pub fn init_tracing(config: &ServerConfig) -> anyhow::Result<()> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        "gateway=info,gateway_core=info,gateway_service=info,gateway_store=info,gateway_providers=info"
            .parse()
            .expect("default tracing filter should parse")
    });

    if let Some(endpoint) = &config.otel_endpoint {
        let exporter = opentelemetry_otlp::SpanExporter::builder()
            .with_tonic()
            .with_endpoint(endpoint)
            .build()
            .context("failed constructing OTLP span exporter")?;

        let tracer_provider = opentelemetry_sdk::trace::SdkTracerProvider::builder()
            .with_batch_exporter(exporter)
            .build();

        let tracer = tracer_provider.tracer("gateway");
        opentelemetry::global::set_tracer_provider(tracer_provider);

        tracing_subscriber::registry()
            .with(filter)
            .with(tracing_subscriber::fmt::layer().json())
            .with(tracing_opentelemetry::layer().with_tracer(tracer))
            .init();

        return Ok(());
    }

    match config.log_format.as_str() {
        "json" => {
            tracing_subscriber::registry()
                .with(filter)
                .with(tracing_subscriber::fmt::layer().json())
                .init();
        }
        _ => {
            tracing_subscriber::registry()
                .with(filter)
                .with(tracing_subscriber::fmt::layer())
                .init();
        }
    }

    Ok(())
}
