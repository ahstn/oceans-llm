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
            "slack-workflow-token",
            format!(
                "xwf{}-{}-{}",
                'p',
                random(DIGITS, 13, 54),
                random(ALNUM, 32, 55)
            ),
        ),
        (
            "slack-rotating-access-token",
            format!("xoxe.xoxp-1-{}", random(ALNUM, 164, 56)),
        ),
        (
            "slack-refresh-token",
            format!("xoxe-1-{}", random(ALNUM, 146, 57)),
        ),
        (
            "jwt",
            format!(
                // base64url `-` and `_` can appear in any segment, the header included.
                "eyJ{}-{}_.eyJ{}.{}",
                random(ALNUM, 15, 11),
                random(ALNUM, 15, 63),
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
    // A mixed-case `Example` prefix is part of a real password, not a placeholder.
    for password in [
        random(ALNUM, 18, 15),
        format!("Example{}", random(ALNUM, 12, 16)),
    ] {
        let text = format!("postgres://app:{password}@db.internal:5432/app");
        assert_eq!(
            redact(&text, &default_config()),
            "postgres://app:[REDACTED:credential-uri]@db.internal:5432/app"
        );
    }
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
fn redacts_tokens_after_json_escapes() {
    // Tool-call arguments are JSON-encoded strings, so a line break inside them
    // is the two characters `\` and `n`.
    let github = format!("ghp_{}", random(ALNUM, 36, 41));
    let key = anthropic_key();
    let arguments = json!({
        "path": ".env",
        "content": format!("GITHUB={github}\nANTHROPIC=x\n{key}\n\tpostgres://app:{}@db/app", random(ALNUM, 16, 42)),
    })
    .to_string();
    assert_eq!(
        redact(&arguments, &default_config()),
        r#"{"content":"GITHUB=[REDACTED:github-token]\nANTHROPIC=x\n[REDACTED:anthropic-api-key]\n\tpostgres://app:[REDACTED:credential-uri]@db/app","path":".env"}"#
    );
}

#[test]
fn redacts_assignments_followed_by_escaped_tabs() {
    let arguments = json!({
        "content": format!("PASSWORD={}\tNEXT=1", random(ALNUM, 20, 47)),
    })
    .to_string();
    assert_eq!(
        redact(&arguments, &default_config()),
        r#"{"content":"PASSWORD=[REDACTED:secret-assignment]\tNEXT=1"}"#
    );
}

#[test]
fn redacts_stateless_github_installation_tokens() {
    // GitHub App installation tokens now use `ghs_<app id>_<jwt>`.
    let token = format!(
        "ghs_123456_eyJ{}.eyJ{}.{}",
        random(ALNUM, 30, 48),
        random(ALNUM, 200, 49),
        random(ALNUM, 86, 50)
    );
    assert_eq!(
        redact(&format!("GH_TOKEN {token} expires soon"), &default_config()),
        "GH_TOKEN [REDACTED:github-token] expires soon"
    );
}

#[test]
fn redacts_secret_assignments_and_bearer_tokens_by_default() {
    let cases = [
        (
            format!("DATABASE_PASSWORD={}\n", random(ALNUM, 20, 43)),
            "DATABASE_PASSWORD=[REDACTED:secret-assignment]\n",
        ),
        (
            format!("export SERVICE_API_TOKEN='{}'", random(ALNUM, 32, 44)),
            "export SERVICE_API_TOKEN='[REDACTED:secret-assignment]'",
        ),
        (
            format!("{{\"password\": \"{}\"}}", random(ALNUM, 16, 45)),
            "{\"password\": \"[REDACTED:secret-assignment]\"}",
        ),
        (
            format!("clientSecretValue: {}", random(ALNUM, 24, 53)),
            "clientSecretValue: [REDACTED:secret-assignment]",
        ),
        (
            format!(
                "PASSWORD=\"{}:{}\"",
                random(ALNUM, 8, 58),
                random(ALNUM, 8, 59)
            ),
            "PASSWORD=\"[REDACTED:secret-assignment-quoted]\"",
        ),
        (
            format!(
                "PASSWORD=\"{}<{}\"",
                random(ALNUM, 8, 66),
                random(ALNUM, 8, 67)
            ),
            "PASSWORD=\"[REDACTED:secret-assignment-quoted]\"",
        ),
        (
            format!("Authorization: Bearer {}", random(ALNUM, 40, 46)),
            "Authorization: Bearer [REDACTED:bearer-token]",
        ),
    ];
    for (text, expected) in cases {
        assert_eq!(redact(&text, &default_config()), expected);
    }
}

#[test]
fn secrets_containing_placeholder_markers_are_still_redacted() {
    for (text, expected) in [
        (
            "PASSWORD=\"Keep_your_data7Safe!\"",
            "PASSWORD=\"[REDACTED:secret-assignment]\"",
        ),
        (
            "password: \"x9{{Kp2mT4!zQ\"",
            "password: \"[REDACTED:secret-assignment-quoted]\"",
        ),
    ] {
        assert_eq!(redact(text, &default_config()), expected);
    }
    for text in [
        "api_key = \"your_api_key_here\"",
        "token: \"{{ secrets.token }}\"",
    ] {
        assert_eq!(redact(text, &default_config()), text);
    }
}

#[test]
fn secret_assignment_ignores_references_counts_and_calls() {
    for text in [
        "password = getPassword()",
        "token: process.env.SERVICE_TOKEN",
        "\"max_tokens\": 4096",
        "\"total_tokens\": 1234567890123",
        "api_key: ${SERVICE_API_KEY}",
        "token_type: \"Bearer\"",
        "\"tokenizer\": \"cl100k_base\"",
        "passwords: 12345678abc",
        "\"password\": \"must be at least 8 characters\"",
        "Authorization: Bearer <token>",
        "use the bearer authentication_scheme_name",
    ] {
        assert_eq!(redact(text, &default_config()), text);
    }
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

    let generic = format!("client_key = \"{}\"", random(ALNUM, 32, 17));
    assert_eq!(redact(&generic, &default_config()), generic);
    assert_eq!(
        redact(&generic, &config(&[SecretTier::Generic])),
        "client_key = \"[REDACTED:generic-api-key]\""
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
    // Base64 whose bytes happen to contain an AWS access key ID.
    let media = format!(
        "{}+AKIA{}/{}==",
        random(ALNUM, 40, 20),
        random(UPPER_BASE32, 16, 21),
        random(ALNUM, 40, 22)
    );
    let mut request = json!({
        "messages": [
            {"role": "user", "content": format!("my key is {key}")},
            {"role": "assistant", "tool_calls": [{"function": {
                "name": "deploy",
                "arguments": format!("{{\"token\":\"{key}\"}}"),
            }}]},
            {"role": "user", "content": [
                {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": media.clone()}},
                {"type": "image_url", "image_url": {"url": format!("data:text/plain,{key}")}},
                {"type": "image_url", "image_url": {"url": format!("data:image/png;base64,{media}")}},
                {"type": "image_url", "image_url": {"url": format!("data:image/png;base64,{key}")}},
                {"type": "input_audio", "input_audio": {"data": media.clone(), "format": "wav"}},
                {"inline_data": {"mime_type": "image/png", "data": format!("see {key}")}},
            ]},
        ],
        "metadata": {
            "data": {"token": key.clone()},
            "note": {"data": format!("key {key}")},
            "json": {"format": "json", "data": format!("deploy with {key}")},
        },
        "b64_json": format!("text with {key}"),
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
            "/b64_json",
            "/messages/0/content",
            "/messages/1/tool_calls/0/function/arguments",
            "/messages/2/content/1/image_url/url",
            "/messages/2/content/3/image_url/url",
            "/messages/2/content/5/inline_data/data",
            "/metadata/data/token",
            "/metadata/json/data",
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
    let content = &request["messages"][2]["content"];
    assert_eq!(content[0]["source"]["data"], media);
    assert_eq!(
        content[2]["image_url"]["url"],
        format!("data:image/png;base64,{media}")
    );
    assert_eq!(content[4]["input_audio"]["data"], media);
}

#[test]
fn skips_base64_data_uris_whatever_their_case() {
    let media = format!(
        "{}+AKIA{}/==",
        random(ALNUM, 40, 64),
        random(UPPER_BASE32, 16, 65)
    );
    let mut request = json!({"url": format!("DATA:image/png;BASE64,{media}")});

    assert!(redact_json_secrets(&mut request, &default_config()).is_empty());
}

#[test]
fn tokens_after_json_unicode_escapes_are_redacted() {
    let key = anthropic_key();
    for escape in ["\\u000a", "\\u000A", "\\b", "\\f"] {
        let text = format!("{{\"note\":\"line{escape}{key}{escape}\"}}");
        let expected = format!("{{\"note\":\"line{escape}[REDACTED:anthropic-api-key]{escape}\"}}");
        assert_eq!(redact(&text, &default_config()), expected);
    }
    let password = "{\"cmd\":\"PASSWORD=Kp2mT4zQ9x\\u000a\"}";
    assert_eq!(
        redact(password, &default_config()),
        "{\"cmd\":\"PASSWORD=[REDACTED:secret-assignment]\\u000a\"}"
    );
}

#[test]
fn percent_wrapped_values_are_placeholders_only_as_variable_names() {
    assert_eq!(
        redact("PASSWORD=\"%A7x9!K2mN5%\"", &default_config()),
        "PASSWORD=\"[REDACTED:secret-assignment]\""
    );
    let reference = "PASSWORD=\"%DB_PASSWORD%\"";
    assert_eq!(redact(reference, &default_config()), reference);
}

#[test]
fn short_b64_json_values_are_scanned() {
    let key = format!("AKIA{}", random(UPPER_BASE32, 16, 70));
    let mut request = json!({"b64_json": key.clone()});
    assert_eq!(
        redact_json_secrets(&mut request, &default_config()).len(),
        1
    );
    assert_eq!(request["b64_json"], "[REDACTED:aws-access-key-id]");

    let image = format!("iVBORw0KGgo{}{key}", random(ALNUM, 1100, 71));
    let mut request = json!({"b64_json": image.clone()});
    assert!(redact_json_secrets(&mut request, &default_config()).is_empty());

    // Long base64-alphabet padding without an image signature is still scanned.
    let padded = format!("{key}+{}", random(ALNUM, 1100, 71));
    let mut request = json!({"b64_json": padded});
    assert_eq!(
        redact_json_secrets(&mut request, &default_config()).len(),
        1
    );
}

#[test]
fn assignments_end_at_collection_closers_and_match_every_private_key_spelling() {
    let value = random(ALNUM, 16, 72);
    for (text, expected) in [
        (
            format!("{{password: {value}}}"),
            "{password: [REDACTED:secret-assignment]}",
        ),
        (
            format!("[token={value}]"),
            "[token=[REDACTED:secret-assignment]]",
        ),
        (
            format!("connect(password={value})"),
            "connect(password=[REDACTED:secret-assignment])",
        ),
        (
            format!("{{\"privateKey\":\"{value}\"}}"),
            "{\"privateKey\":\"[REDACTED:secret-assignment]\"}",
        ),
        (
            format!("private-key={value}"),
            "private-key=[REDACTED:secret-assignment]",
        ),
    ] {
        assert_eq!(redact(&text, &default_config()), expected, "{text}");
    }
}

#[test]
fn assignment_names_need_a_left_boundary() {
    let value = random(ALNUM, 16, 73);
    for name in [
        "dbPassword",
        "DATABASE_PASSWORD",
        "x.secret",
        "myApiKey",
        "dbAPIKey",
        "openAIApiKey",
        "awsIAMSecret",
    ] {
        let text = format!("{name}={value}");
        assert_eq!(
            redact(&text, &default_config()),
            format!("{name}=[REDACTED:secret-assignment]"),
            "{text}"
        );
    }
    let escaped = format!(r#"{{\"a\":1,\npassword={value}}}"#);
    assert!(!redact(&escaped, &default_config()).contains(&value));

    for name in ["mysecretvalue", "bitoken", "nopassword"] {
        let text = format!("{name}={value}");
        assert_eq!(redact(&text, &default_config()), text);
        let mut request = json!({ name: value.clone() });
        assert!(
            redact_json_secrets(&mut request, &default_config()).is_empty(),
            "{name}"
        );
    }
}

#[test]
fn adjacent_assignments_share_a_delimiter() {
    let first = random(ALNUM, 16, 74);
    let second = random(ALNUM, 16, 75);
    for separator in ["\n", ";", r"\n"] {
        let text = format!("PASSWORD={first}{separator}TOKEN={second}");
        let redacted = redact(&text, &default_config());
        assert!(
            !redacted.contains(&first) && !redacted.contains(&second),
            "{redacted}"
        );
    }
}

#[test]
fn credential_named_arrays_scan_their_strings_as_assignments() {
    let value = random(ALNUM, 16, 76);
    let mut request =
        json!({"password": [value.clone(), [value.clone()], {"note": value.clone()}]});
    let fields = redact_json_secrets(&mut request, &default_config());
    assert_eq!(fields.len(), 2);
    assert_eq!(request["password"][0], "[REDACTED:secret-assignment]");
    assert_eq!(request["password"][1][0], "[REDACTED:secret-assignment]");
    assert_eq!(request["password"][2]["note"], value);
}

#[test]
fn data_uri_headers_are_scanned_but_payloads_are_not() {
    let key = format!("AKIA{}", random(UPPER_BASE32, 16, 77));
    let payload = random(ALNUM, 64, 78);
    let mut request = json!({"url": format!("data:image/png;name={key};base64,{payload}")});
    let fields = redact_json_secrets(&mut request, &default_config());
    assert_eq!(fields.len(), 1);
    assert_eq!(
        request["url"],
        format!("data:image/png;name=[REDACTED:aws-access-key-id];base64,{payload}")
    );
}

#[test]
fn connection_uris_with_escaped_slashes_are_redacted() {
    let password = random(ALNUM, 16, 79);
    let text = format!(r#"{{"dsn":"postgres:\/\/app:{password}@db:5432\/main"}}"#);
    let redacted = redact(&text, &default_config());
    assert!(!redacted.contains(&password), "{redacted}");
    assert!(redacted.contains("[REDACTED:credential-uri]"), "{redacted}");
}

#[test]
fn media_data_needs_a_media_type_descriptor() {
    let key = format!("AKIA{}", random(UPPER_BASE32, 16, 69));
    for descriptor in [json!(null), json!("not a type"), json!(7)] {
        let mut request = json!({"mime_type": descriptor, "data": key.clone()});
        let fields = redact_json_secrets(&mut request, &default_config());
        assert_eq!(fields.len(), 1, "{descriptor} exempted the data field");
        assert_eq!(request["data"], "[REDACTED:aws-access-key-id]");
    }
    let mut request = json!({"type": "base64", "data": key.clone()});
    assert_eq!(
        redact_json_secrets(&mut request, &default_config()).len(),
        1
    );
    let mut request = json!({"mime_type": "image/png; q=1", "data": key.clone()});
    assert!(redact_json_secrets(&mut request, &default_config()).is_empty());
}

#[test]
fn only_aws_rules_treat_example_suffixes_as_placeholders() {
    assert_eq!(
        redact("PASSWORD=\"Tr0ub4dorEXAMPLE\"", &default_config()),
        "PASSWORD=\"[REDACTED:secret-assignment]\""
    );
}

#[test]
fn redacts_secrets_in_object_keys() {
    let github = format!("ghp_{}", random(ALNUM, 36, 51));
    let other = format!("ghp_{}", random(ALNUM, 36, 52));
    let mut request = json!({
        "tool_result": {
            github.clone(): {"scopes": "repo"},
            other.clone(): {"scopes": format!("admin {github}")},
            "[REDACTED:github-token]": "existing entry",
            "plain": 1,
        }
    });

    let fields = redact_json_secrets(&mut request, &default_config());

    assert!(!request.to_string().contains(&github));
    assert!(!request.to_string().contains(&other));
    // No entry is dropped: the literal key stays, and the redacted keys get suffixes.
    assert_eq!(
        request,
        json!({
            "tool_result": {
                "[REDACTED:github-token]": "existing entry",
                "[REDACTED:github-token] (2)": {"scopes": "repo"},
                "[REDACTED:github-token] (3)": {"scopes": "admin [REDACTED:github-token]"},
                "plain": 1,
            }
        })
    );
    let mut pointers: Vec<_> = fields
        .iter()
        .map(|field| (field.pointer.as_str(), field.text.as_str()))
        .collect();
    pointers.sort_unstable();
    assert_eq!(
        pointers,
        [
            (
                "/tool_result/[REDACTED:github-token] (2)",
                "[REDACTED:github-token] (2)"
            ),
            (
                "/tool_result/[REDACTED:github-token] (3)",
                "[REDACTED:github-token] (3)"
            ),
            (
                "/tool_result/[REDACTED:github-token] (3)/scopes",
                "admin [REDACTED:github-token]"
            ),
        ]
    );
}

#[test]
fn redacts_string_values_of_credential_named_fields() {
    let password = format!("{}:{}", random(ALNUM, 8, 60), random(ALNUM, 8, 61));
    let notes = random(ALNUM, 16, 62);
    let mut request = json!({
        "state": {"password": password, "notes": notes, "secret_count": 3},
    });

    let fields = redact_json_secrets(&mut request, &default_config());

    assert_eq!(
        request,
        json!({
            "state": {
                "password": "[REDACTED:secret-assignment-quoted]",
                "notes": notes,
                "secret_count": 3,
            },
        })
    );
    let [field] = fields.as_slice() else {
        panic!("expected one redacted field, got {}", fields.len());
    };
    assert_eq!(field.pointer, "/state/password");
}

#[test]
fn aws_secret_rule_ignores_fields_that_only_start_like_its_names() {
    let id = random(ALNUM, 40, 68);
    let mut request = json!({"accessibility_id": id});

    assert!(redact_json_secrets(&mut request, &default_config()).is_empty());
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
