use anyhow::{Context, bail};
use gateway_core::RequestLogRetentionWindow;
use gateway_service::{
    PayloadPath, RequestLogPayloadCaptureMode, RequestLogPayloadPolicy, parse_payload_path,
};
use serde::Deserialize;

const fn default_request_log_request_max_bytes() -> usize {
    64 * 1024
}

const fn default_request_log_response_max_bytes() -> usize {
    64 * 1024
}

const fn default_request_log_stream_max_events() -> usize {
    128
}

const fn default_request_log_purge_retention() -> RequestLogRetentionWindow {
    RequestLogRetentionWindow::SevenDays
}

fn default_request_log_purge_schedule() -> String {
    "0 0 * * *".to_string()
}

fn validate_daily_cron_schedule(field_name: &str, schedule: &str) -> anyhow::Result<()> {
    let schedule = schedule.trim();
    let fields = schedule.split_whitespace().count();
    if fields != 5 {
        bail!("{field_name} must use standard 5-field cron syntax");
    }

    let parsed: cron::Schedule = format!("0 {schedule}")
        .parse()
        .with_context(|| format!("{field_name} `{schedule}` is invalid"))?;
    let mut upcoming = parsed.upcoming(chrono::Utc);
    let first = upcoming
        .next()
        .ok_or_else(|| anyhow::anyhow!("{field_name} `{schedule}` has no upcoming run"))?;
    let second = upcoming
        .next()
        .ok_or_else(|| anyhow::anyhow!("{field_name} `{schedule}` has fewer than two runs"))?;

    if second - first < chrono::Duration::days(1) {
        bail!("{field_name} must not run more frequently than once per day");
    }

    Ok(())
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct RequestLoggingConfig {
    #[serde(default)]
    pub payloads: RequestLogPayloadConfig,
    #[serde(default)]
    pub purge: RequestLogPurgeConfig,
}

impl RequestLoggingConfig {
    pub(super) fn validate(&self) -> anyhow::Result<()> {
        self.payloads.validate()?;
        self.purge.validate()
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct RequestLogPurgeConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_request_log_purge_retention")]
    pub retention: RequestLogRetentionWindow,
    #[serde(default = "default_request_log_purge_schedule")]
    pub schedule: String,
}

impl Default for RequestLogPurgeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            retention: default_request_log_purge_retention(),
            schedule: default_request_log_purge_schedule(),
        }
    }
}

impl RequestLogPurgeConfig {
    fn validate(&self) -> anyhow::Result<()> {
        validate_daily_cron_schedule("request_logging.purge.schedule", self.schedule.as_str())
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct RequestLogPayloadConfig {
    #[serde(default)]
    pub capture_mode: RequestLogPayloadCaptureModeConfig,
    #[serde(default = "default_request_log_request_max_bytes")]
    pub request_max_bytes: usize,
    #[serde(default = "default_request_log_response_max_bytes")]
    pub response_max_bytes: usize,
    #[serde(default = "default_request_log_stream_max_events")]
    pub stream_max_events: usize,
    #[serde(default)]
    pub redaction_paths: Vec<String>,
}

impl Default for RequestLogPayloadConfig {
    fn default() -> Self {
        Self {
            capture_mode: RequestLogPayloadCaptureModeConfig::default(),
            request_max_bytes: default_request_log_request_max_bytes(),
            response_max_bytes: default_request_log_response_max_bytes(),
            stream_max_events: default_request_log_stream_max_events(),
            redaction_paths: Vec::new(),
        }
    }
}

impl RequestLogPayloadConfig {
    fn validate(&self) -> anyhow::Result<()> {
        if self.request_max_bytes == 0 {
            bail!("request_logging.payloads.request_max_bytes must be > 0");
        }
        if self.response_max_bytes == 0 {
            bail!("request_logging.payloads.response_max_bytes must be > 0");
        }
        if self.stream_max_events == 0 {
            bail!("request_logging.payloads.stream_max_events must be > 0");
        }
        for path in &self.redaction_paths {
            parse_payload_path(path).map_err(|error| {
                anyhow::anyhow!(
                    "request_logging.payloads.redaction_paths `{path}` is invalid: {error}"
                )
            })?;
        }
        Ok(())
    }

    pub(super) fn to_policy(&self) -> anyhow::Result<RequestLogPayloadPolicy> {
        let paths = self
            .redaction_paths
            .iter()
            .map(|path| {
                parse_payload_path(path).map_err(|error| {
                    anyhow::anyhow!(
                        "request_logging.payloads.redaction_paths `{path}` is invalid: {error}"
                    )
                })
            })
            .collect::<anyhow::Result<Vec<PayloadPath>>>()?;

        Ok(RequestLogPayloadPolicy::new(
            self.capture_mode.into(),
            self.request_max_bytes,
            self.response_max_bytes,
            self.stream_max_events,
            paths,
        ))
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RequestLogPayloadCaptureModeConfig {
    Disabled,
    SummaryOnly,
    #[default]
    RedactedPayloads,
}

impl From<RequestLogPayloadCaptureModeConfig> for RequestLogPayloadCaptureMode {
    fn from(value: RequestLogPayloadCaptureModeConfig) -> Self {
        match value {
            RequestLogPayloadCaptureModeConfig::Disabled => Self::Disabled,
            RequestLogPayloadCaptureModeConfig::SummaryOnly => Self::SummaryOnly,
            RequestLogPayloadCaptureModeConfig::RedactedPayloads => Self::RedactedPayloads,
        }
    }
}
