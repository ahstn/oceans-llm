use std::net::SocketAddr;

use anyhow::{Context, bail};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_bind")]
    pub bind: String,
    #[serde(default = "default_log_format")]
    pub log_format: String,
    #[serde(default)]
    pub otel_endpoint: Option<String>,
    #[serde(default)]
    pub otel_metrics_endpoint: Option<String>,
    #[serde(default = "default_otel_trace_sample_ratio")]
    pub otel_trace_sample_ratio: f64,
    #[serde(default = "default_otel_export_interval_secs")]
    pub otel_export_interval_secs: u64,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind: default_bind(),
            log_format: default_log_format(),
            otel_endpoint: None,
            otel_metrics_endpoint: None,
            otel_trace_sample_ratio: default_otel_trace_sample_ratio(),
            otel_export_interval_secs: default_otel_export_interval_secs(),
        }
    }
}

impl ServerConfig {
    pub(super) fn validate(&self) -> anyhow::Result<()> {
        self.bind_address()?;
        validate_otel_endpoint("server.otel_endpoint", self.otel_endpoint.as_deref())?;
        validate_otel_endpoint(
            "server.otel_metrics_endpoint",
            self.otel_metrics_endpoint.as_deref(),
        )?;
        if !(0.0..=1.0).contains(&self.otel_trace_sample_ratio) {
            bail!("server.otel_trace_sample_ratio must be between 0.0 and 1.0 inclusive");
        }
        Ok(())
    }

    pub fn bind_address(&self) -> anyhow::Result<SocketAddr> {
        self.bind
            .parse()
            .with_context(|| format!("server.bind `{}` is not a valid socket address", self.bind))
    }
}

fn validate_otel_endpoint(field: &str, endpoint: Option<&str>) -> anyhow::Result<()> {
    let Some(endpoint) = endpoint else {
        return Ok(());
    };
    let uri: http::Uri = endpoint
        .parse()
        .with_context(|| format!("{field} `{endpoint}` is not a valid URI"))?;
    if !matches!(uri.scheme_str(), Some("http" | "https")) || uri.host().is_none() {
        bail!("{field} must be an absolute HTTP URI with a host");
    }
    Ok(())
}

fn default_bind() -> String {
    "0.0.0.0:8080".to_string()
}

fn default_log_format() -> String {
    "pretty".to_string()
}

const fn default_otel_export_interval_secs() -> u64 {
    30
}

const fn default_otel_trace_sample_ratio() -> f64 {
    1.0
}

#[cfg(test)]
mod tests {
    use super::ServerConfig;

    #[test]
    fn telemetry_endpoints_require_absolute_http_uris() {
        for metrics in [false, true] {
            for (endpoint, accepted) in [
                ("/v1/traces", false),
                ("collector:4317", false),
                ("ftp://collector:4317", false),
                ("http://127.0.0.1:4317", true),
                ("https://collector.example.com:4317", true),
            ] {
                let mut config = ServerConfig::default();
                let field = if metrics {
                    config.otel_metrics_endpoint = Some(endpoint.to_string());
                    "server.otel_metrics_endpoint"
                } else {
                    config.otel_endpoint = Some(endpoint.to_string());
                    "server.otel_endpoint"
                };
                if accepted {
                    config.validate().expect("valid telemetry endpoint");
                } else {
                    let error = config.validate().expect_err("invalid telemetry endpoint");
                    assert!(format!("{error:#}").contains(field), "{error:#}");
                }
            }
        }
    }
}
