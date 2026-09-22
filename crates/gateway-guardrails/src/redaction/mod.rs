//! Deterministic secret redaction for request payloads.
//!
//! Detected secrets are replaced in place with `[REDACTED:<rule_id>]`.
//! Redaction never denies a request; it runs independently of the policy mode.

use std::{collections::BTreeSet, time::Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    DecisionAction, DecisionId, DecisionRecord, EffectivePolicy, EvaluationPayload, GuardPhase,
    MatchedRule, ReasonCode,
};

mod rules;
mod scanner;

const EVALUATOR_ID: &str = "secret_redaction";
const REASON_CODE: &str = "secret_redaction.redacted";
/// Object keys holding inline media (base64 images, audio, and files).
const SKIPPED_KEYS: &[&str] = &["b64_json", "data"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretTier {
    /// Provider-issued tokens with distinctive prefixes, plus private keys and JWTs.
    ProviderTokens,
    /// Credentials identified by surrounding context, such as URI passwords and
    /// keyword-bound assignments.
    Credentials,
    /// The gitleaks generic API key rule. Higher recall, more false positives.
    Generic,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecretRedactionConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_tiers")]
    pub tiers: BTreeSet<SecretTier>,
    #[serde(default)]
    pub disabled_rules: BTreeSet<String>,
}

impl Default for SecretRedactionConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            tiers: default_tiers(),
            disabled_rules: BTreeSet::new(),
        }
    }
}

impl SecretRedactionConfig {
    fn enables(&self, rule: &rules::Rule) -> bool {
        self.tiers.contains(&rule.tier) && !self.disabled_rules.contains(rule.id)
    }
}

pub(crate) fn is_known_rule(rule_id: &str) -> bool {
    rules::RULES.iter().any(|rule| rule.id == rule_id)
}

fn default_tiers() -> BTreeSet<SecretTier> {
    BTreeSet::from([SecretTier::ProviderTokens, SecretTier::Credentials])
}

/// A JSON string that had one or more secrets redacted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedactedField {
    /// RFC 6901 pointer to the redacted string.
    pub pointer: String,
    pub rule_ids: Vec<&'static str>,
}

/// Redacts secrets in every string of `value`, skipping inline media.
pub fn redact_json_secrets(
    value: &mut Value,
    config: &SecretRedactionConfig,
) -> Vec<RedactedField> {
    let mut redacted = Vec::new();
    if config.enabled {
        redact_json_at(value, &mut String::new(), config, &mut redacted);
    }
    redacted
}

fn redact_json_at(
    value: &mut Value,
    pointer: &mut String,
    config: &SecretRedactionConfig,
    output: &mut Vec<RedactedField>,
) {
    let parent_len = pointer.len();
    match value {
        Value::String(text) => {
            if text.starts_with("data:") {
                return;
            }
            if let Some((replacement, rule_ids)) = scanner::redact_text(text, config) {
                *text = replacement;
                output.push(RedactedField {
                    pointer: pointer.clone(),
                    rule_ids,
                });
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter_mut().enumerate() {
                pointer.push('/');
                pointer.push_str(&index.to_string());
                redact_json_at(item, pointer, config, output);
                pointer.truncate(parent_len);
            }
        }
        Value::Object(object) => {
            for (key, child) in object.iter_mut() {
                if SKIPPED_KEYS.contains(&key.as_str()) {
                    continue;
                }
                pointer.push('/');
                pointer.push_str(&key.replace('~', "~0").replace('/', "~1"));
                redact_json_at(child, pointer, config, output);
                pointer.truncate(parent_len);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

/// Redacts secrets from a prompt-phase request and returns one decision per
/// matched rule. Returns no decisions when the policy or redaction is disabled.
pub fn redact_prompt_secrets(policy: &EffectivePolicy, request: &mut Value) -> Vec<DecisionRecord> {
    if !policy.enabled || !policy.secret_redaction.enabled {
        return Vec::new();
    }
    let started = Instant::now();
    let fields = redact_json_secrets(request, &policy.secret_redaction);
    let latency_micros = DecisionRecord::latency(started.elapsed());

    let mut seen = BTreeSet::new();
    let mut decisions = Vec::new();
    for field in &fields {
        let redacted_text = request
            .pointer(&field.pointer)
            .and_then(Value::as_str)
            .unwrap_or_default();
        for rule_id in &field.rule_ids {
            if seen.insert(*rule_id) {
                decisions.push(redaction_decision(
                    policy,
                    rule_id,
                    &field.pointer,
                    redacted_text,
                    latency_micros,
                ));
            }
        }
    }
    decisions
}

fn redaction_decision(
    policy: &EffectivePolicy,
    rule_id: &str,
    pointer: &str,
    redacted_text: &str,
    latency_micros: u64,
) -> DecisionRecord {
    let reason_code = ReasonCode::new(REASON_CODE).expect("static reason code is valid");
    DecisionRecord {
        decision_id: DecisionId::new(),
        phase: GuardPhase::Prompt,
        scope: policy.scope.clone(),
        evaluator: EVALUATOR_ID.to_string(),
        managed_service: None,
        managed_metadata: None,
        action: DecisionAction::Transformed,
        reason_code: reason_code.clone(),
        matched_rule: Some(MatchedRule {
            pack_id: EVALUATOR_ID.to_string(),
            rule_id: rule_id.to_string(),
            matched_field: pointer.to_string(),
            reason_code,
            description: format!("Redacted a secret matching `{rule_id}`"),
            safer_action:
                "Reference secrets by name or environment variable instead of pasting values"
                    .to_string(),
        }),
        latency_micros,
        failure_disposition: None,
        transformed: true,
        // Hash only redacted content so the decision log never fingerprints a secret.
        content_hash: EvaluationPayload::Text {
            text: redacted_text.to_string(),
        }
        .content_hash(),
    }
}

#[cfg(test)]
mod tests;
