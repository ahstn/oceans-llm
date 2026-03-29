use std::sync::Arc;

use anyhow::{Context, bail};
use async_trait::async_trait;
use gateway_service::{
    BudgetAlertEmail, BudgetAlertSendResult, BudgetAlertSender, SinkBudgetAlertSender,
};
use lettre::{
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor, message::Mailbox,
    transport::smtp::authentication::Credentials,
};

use crate::config::{
    BudgetAlertEmailConfig, BudgetAlertEmailTransportConfig, SmtpBudgetAlertEmailTransportConfig,
    resolve_secret_reference,
};

pub fn build_budget_alert_sender(
    config: &BudgetAlertEmailConfig,
) -> anyhow::Result<Arc<dyn BudgetAlertSender>> {
    match &config.transport {
        BudgetAlertEmailTransportConfig::Sink => Ok(Arc::new(SinkBudgetAlertSender)),
        BudgetAlertEmailTransportConfig::Smtp(smtp) => {
            Ok(Arc::new(SmtpBudgetAlertSender::new(config, smtp)?))
        }
    }
}

struct SmtpBudgetAlertSender {
    from: Mailbox,
    transport: AsyncSmtpTransport<Tokio1Executor>,
}

impl SmtpBudgetAlertSender {
    fn new(
        config: &BudgetAlertEmailConfig,
        smtp: &SmtpBudgetAlertEmailTransportConfig,
    ) -> anyhow::Result<Self> {
        let from = if let Some(from_name) = &config.from_name {
            Mailbox::new(
                Some(from_name.clone()),
                config.from_email.parse().with_context(|| {
                    format!("invalid budget alert from_email `{}`", config.from_email)
                })?,
            )
        } else {
            Mailbox::new(
                None,
                config.from_email.parse().with_context(|| {
                    format!("invalid budget alert from_email `{}`", config.from_email)
                })?,
            )
        };

        let builder = if smtp.starttls {
            AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&smtp.host)
                .with_context(|| format!("invalid SMTP relay host `{}`", smtp.host))?
        } else {
            AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&smtp.host)
        };

        let mut builder = builder.port(smtp.port);
        if let (Some(username), Some(password)) = (&smtp.username, &smtp.password) {
            let username = resolve_secret_reference(username)?;
            let password = resolve_secret_reference(password)?;
            builder = builder.credentials(Credentials::new(username, password));
        } else if smtp.username.is_some() || smtp.password.is_some() {
            bail!("SMTP username/password must both be configured");
        }

        Ok(Self {
            from,
            transport: builder.build(),
        })
    }
}

#[async_trait]
impl BudgetAlertSender for SmtpBudgetAlertSender {
    async fn send(&self, email: &BudgetAlertEmail) -> anyhow::Result<BudgetAlertSendResult> {
        let recipient = email
            .recipient
            .parse()
            .with_context(|| format!("invalid alert recipient `{}`", email.recipient))?;
        let message = Message::builder()
            .from(self.from.clone())
            .to(Mailbox::new(None, recipient))
            .subject(email.subject.clone())
            .body(email.text_body.clone())
            .context("failed to build SMTP budget alert message")?;

        let response = self
            .transport
            .send(message)
            .await
            .context("SMTP budget alert send failed")?;

        Ok(BudgetAlertSendResult {
            provider_message_id: response.message().next().map(ToString::to_string),
        })
    }
}
