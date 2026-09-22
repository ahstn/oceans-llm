use serde_json::json;

use super::*;
use crate::{EffectiveScope, PolicyMode};

// Fixtures are assembled at runtime so no literal credential lands in the repository.
const ALNUM: &[u8] = b"aZ3kQ9mB7xR2tW5nL8pJ4vC6yH1dF0gS";
const DIGITS: &[u8] = b"0123456789";
const HEX: &[u8] = b"0123456789abcdef";
const UPPER_BASE32: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

/// Deterministic xorshift output drawn from `alphabet`.
fn random(alphabet: &[u8], len: usize, seed: u64) -> String {
    let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            alphabet[(state % alphabet.len() as u64) as usize] as char
        })
        .collect()
}

fn anthropic_key() -> String {
    format!("sk-ant-api03-{}AA", random(ALNUM, 93, 1))
}

fn config(tiers: &[SecretTier]) -> SecretRedactionConfig {
    SecretRedactionConfig {
        enabled: true,
        tiers: tiers.iter().copied().collect(),
        disabled_rules: BTreeSet::new(),
    }
}

fn default_config() -> SecretRedactionConfig {
    SecretRedactionConfig {
        enabled: true,
        ..SecretRedactionConfig::default()
    }
}

fn redact(text: &str, config: &SecretRedactionConfig) -> String {
    scanner::redact_text(text, config)
        .map(|(redacted, _)| redacted)
        .unwrap_or_else(|| text.to_string())
}

#[test]
fn redacts_provider_tokens_with_rule_placeholders() {
    let cases = [
        ("anthropic-api-key", anthropic_key()),
        (
            "openai-api-key",
            format!(
                "sk-proj-{}T3BlbkFJ{}",
                random(ALNUM, 74, 2),
                random(ALNUM, 74, 3)
            ),
        ),
        ("google-api-key", format!("AIza{}", random(ALNUM, 35, 4))),
        ("groq-api-key", format!("gsk_{}", random(ALNUM, 52, 5))),
        (
            "openrouter-api-key",
            format!("sk-or-v1-{}", random(HEX, 64, 6)),
        ),
        ("github-token", format!("ghp_{}", random(ALNUM, 36, 7))),
        (
            "aws-access-key-id",
            format!("AKIA{}", random(UPPER_BASE32, 16, 8)),
        ),
        (
            "stripe-api-key",
            format!("sk_live_{}", random(ALNUM, 24, 9)),
        ),
        (
            "slack-bot-token",
            format!(
                "xox{}-{}-{}-{}",
                'b',
                random(DIGITS, 10, 19),
                random(DIGITS, 10, 20),
                random(ALNUM, 24, 10)
            ),
        ),
        (
            "jwt",
            format!(
                "eyJ{}.eyJ{}.{}",
                random(ALNUM, 30, 11),
                random(ALNUM, 40, 12),
                random(ALNUM, 43, 13)
            ),
        ),
    ];
    for (rule_id, secret) in cases {
        let text = format!("use `{secret}` for the call");
        assert_eq!(
            redact(&text, &default_config()),
            format!("use `[REDACTED:{rule_id}]` for the call"),
            "{rule_id} was not redacted"
        );
    }
}

#[test]
fn redacts_private_key_blocks() {
    let key = format!(
        "-----BEGIN RSA PRIVATE KEY-----\n{}\n-----END RSA PRIVATE KEY-----",
        random(ALNUM, 128, 14)
    );
    assert_eq!(
        redact(&format!("key:\n{key}\nend"), &default_config()),
        "key:\n[REDACTED:private-key]\nend"
    );
}

#[test]
fn redacts_only_the_password_in_credential_uris() {
    let password = random(ALNUM, 18, 15);
    let text = format!("postgres://app:{password}@db.internal:5432/app");
    assert_eq!(
        redact(&text, &default_config()),
        "postgres://app:[REDACTED:credential-uri]@db.internal:5432/app"
    );
}

#[test]
fn redacts_keyword_bound_credentials() {
    let secret = random(ALNUM, 40, 16);
    assert_eq!(
        redact(
            &format!("AWS_SECRET_ACCESS_KEY={secret}\n"),
            &default_config()
        ),
        "AWS_SECRET_ACCESS_KEY=[REDACTED:aws-secret-access-key]\n"
    );
}

#[test]
fn leaves_placeholders_and_ordinary_text_alone() {
    for text in [
        "AKIAIOSFODNN7EXAMPLE",
        "export ANTHROPIC_API_KEY=${ANTHROPIC_API_KEY}",
        "postgres://app:${DB_PASSWORD}@db/app",
        "The quick brown fox jumps over the lazy dog.",
        "sk-ant-api03-short",
    ] {
        assert_eq!(redact(text, &default_config()), text);
    }
}

#[test]
fn tiers_and_disabled_rules_select_rules() {
    let key = anthropic_key();
    assert_eq!(redact(&key, &config(&[SecretTier::Credentials])), key);

    let mut disabled = default_config();
    disabled.disabled_rules.insert("anthropic-api-key".into());
    assert_eq!(redact(&key, &disabled), key);

    let generic = format!("app_token = \"{}\"", random(ALNUM, 32, 17));
    assert_eq!(redact(&generic, &default_config()), generic);
    assert_eq!(
        redact(&generic, &config(&[SecretTier::Generic])),
        "app_token = \"[REDACTED:generic-api-key]\""
    );
}

#[test]
fn overlapping_findings_report_every_matching_rule() {
    let token = format!("ghp_{}", random(ALNUM, 36, 21));
    let text = format!("github_token = \"{token}\"");
    let (redacted, rule_ids) = scanner::redact_text(
        &text,
        &config(&[SecretTier::ProviderTokens, SecretTier::Generic]),
    )
    .unwrap();
    assert_eq!(redacted, "github_token = \"[REDACTED:github-token]\"");
    assert_eq!(rule_ids, ["github-token", "generic-api-key"]);
}

#[test]
fn generic_rule_ignores_identifier_values() {
    let text = "api_key_name = \"primary_service_account\"";
    assert_eq!(redact(text, &config(&[SecretTier::Generic])), text);
}

#[test]
fn redaction_is_idempotent() {
    let config = config(&[
        SecretTier::ProviderTokens,
        SecretTier::Credentials,
        SecretTier::Generic,
    ]);
    let once = redact(
        &format!("ANTHROPIC_API_KEY=\"{}\"", anthropic_key()),
        &config,
    );
    assert_eq!(once, "ANTHROPIC_API_KEY=\"[REDACTED:anthropic-api-key]\"");
    assert_eq!(redact(&once, &config), once);
}

#[test]
fn redacts_json_strings_and_skips_inline_media() {
    let key = anthropic_key();
    let mut request = json!({
        "messages": [
            {"role": "user", "content": format!("my key is {key}")},
            {"role": "assistant", "tool_calls": [{"function": {
                "name": "deploy",
                "arguments": format!("{{\"token\":\"{key}\"}}"),
            }}]},
            {"role": "user", "content": [
                {"type": "image", "source": {"type": "base64", "data": key.clone()}},
                {"type": "image_url", "image_url": {"url": format!("data:text/plain,{key}")}},
                {"type": "image_url", "image_url": {"url": format!("data:image/png;base64,{key}")}},
                {"type": "input_audio", "input_audio": {"data": key.clone(), "format": "wav"}},
            ]},
        ],
        "metadata": {"data": {"token": key.clone()}, "note": {"data": format!("key {key}")}},
        "a/b": key.clone(),
    });

    let fields = redact_json_secrets(&mut request, &default_config());

    let pointers = fields
        .iter()
        .map(|field| field.pointer.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        pointers,
        [
            "/a~1b",
            "/messages/0/content",
            "/messages/1/tool_calls/0/function/arguments",
            "/messages/2/content/1/image_url/url",
            "/metadata/data/token",
            "/metadata/note/data",
        ]
    );
    assert_eq!(
        request["messages"][0]["content"],
        "my key is [REDACTED:anthropic-api-key]"
    );
    let arguments: Value = serde_json::from_str(
        request["messages"][1]["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(arguments["token"], "[REDACTED:anthropic-api-key]");
    assert_eq!(request["messages"][2]["content"][0]["source"]["data"], key);
}

#[test]
fn disabled_config_redacts_nothing() {
    let key = anthropic_key();
    let mut request = json!({"prompt": key});
    assert!(redact_json_secrets(&mut request, &SecretRedactionConfig::default()).is_empty());
    assert_eq!(request["prompt"], key);
}

fn policy(enabled: bool, secret_redaction: SecretRedactionConfig) -> EffectivePolicy {
    EffectivePolicy {
        enabled,
        mode: PolicyMode::Deny,
        packs: Vec::new(),
        managed_checks: Vec::new(),
        stream_buffer_bytes: 1024,
        stream_buffer_timeout_ms: 1_000,
        secret_redaction,
        scope: EffectiveScope::ModelRoute("openai/gpt".into()),
    }
}

#[test]
fn prompt_redaction_records_one_transformed_decision_per_rule() {
    let key = anthropic_key();
    let github = format!("ghp_{}", random(ALNUM, 36, 18));
    let mut request = json!({
        "messages": [
            {"role": "user", "content": format!("{key} and {github}")},
            {"role": "user", "content": key},
        ],
    });

    let decisions = redact_prompt_secrets(&policy(true, default_config()), &mut request);

    let rules = decisions
        .iter()
        .map(|decision| {
            let rule = decision.matched_rule.as_ref().unwrap();
            (rule.rule_id.as_str(), rule.matched_field.as_str())
        })
        .collect::<Vec<_>>();
    assert_eq!(
        rules,
        [
            ("anthropic-api-key", "/messages/0/content"),
            ("github-token", "/messages/0/content"),
        ]
    );
    for decision in &decisions {
        assert_eq!(decision.action, DecisionAction::Transformed);
        assert!(decision.transformed);
        assert_eq!(decision.evaluator, "secret_redaction");
        assert_eq!(decision.reason_code.as_str(), "secret_redaction.redacted");
        assert_eq!(decision.phase, GuardPhase::Prompt);
    }
    assert!(!request.to_string().contains(&key));
}

#[test]
fn prompt_redaction_requires_enabled_policy() {
    let key = anthropic_key();
    let mut request = json!({"prompt": key});
    assert!(redact_prompt_secrets(&policy(false, default_config()), &mut request).is_empty());
    assert_eq!(request["prompt"], key);
}
