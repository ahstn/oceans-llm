use super::*;

#[derive(Debug, Clone, Deserialize, Default)]
pub struct BudgetAlertConfig {
    #[serde(default)]
    pub email: BudgetAlertEmailConfig,
}

impl BudgetAlertConfig {
    pub(super) fn validate(&self) -> anyhow::Result<()> {
        self.email.validate()
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct BudgetAlertEmailConfig {
    #[serde(default = "default_budget_alert_from_email")]
    pub from_email: String,
    #[serde(default)]
    pub from_name: Option<String>,
    #[serde(default = "default_budget_alert_poll_interval_secs")]
    pub poll_interval_secs: u64,
    #[serde(default = "default_budget_alert_batch_size")]
    pub batch_size: u32,
    #[serde(default)]
    pub transport: BudgetAlertEmailTransportConfig,
}

impl Default for BudgetAlertEmailConfig {
    fn default() -> Self {
        Self {
            from_email: default_budget_alert_from_email(),
            from_name: None,
            poll_interval_secs: default_budget_alert_poll_interval_secs(),
            batch_size: default_budget_alert_batch_size(),
            transport: BudgetAlertEmailTransportConfig::default(),
        }
    }
}

impl BudgetAlertEmailConfig {
    fn validate(&self) -> anyhow::Result<()> {
        if self.from_email.trim().is_empty() {
            bail!("budget_alerts.email.from_email cannot be empty");
        }
        if self.poll_interval_secs == 0 {
            bail!("budget_alerts.email.poll_interval_secs must be > 0");
        }
        if self.batch_size == 0 {
            bail!("budget_alerts.email.batch_size must be > 0");
        }
        match &self.transport {
            BudgetAlertEmailTransportConfig::Sink => {}
            BudgetAlertEmailTransportConfig::Smtp(smtp) => {
                if smtp.host.trim().is_empty() {
                    bail!("budget_alerts.email.transport.smtp.host cannot be empty");
                }
                if smtp.username.is_some() != smtp.password.is_some() {
                    bail!(
                        "budget_alerts.email.transport.smtp.username and password must be set together"
                    );
                }
                if let Some(username) = &smtp.username {
                    validate_env_reference_if_needed(username)?;
                }
                if let Some(password) = &smtp.password {
                    resolve_secret_reference(password)?;
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BudgetAlertEmailTransportConfig {
    #[default]
    Sink,
    Smtp(SmtpBudgetAlertEmailTransportConfig),
}

#[derive(Debug, Clone, Deserialize)]
pub struct SmtpBudgetAlertEmailTransportConfig {
    pub host: String,
    #[serde(default = "default_budget_alert_smtp_port")]
    pub port: u16,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default = "default_budget_alert_smtp_starttls")]
    pub starttls: bool,
}
