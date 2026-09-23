//! Secret detection rules.
//!
//! Provider-token patterns are adapted from gitleaks
//! (<https://github.com/gitleaks/gitleaks>, MIT, Copyright (c) 2019 Zachary Rice)
//! and Betterleaks (<https://github.com/betterleaks/betterleaks>, MIT). Keep this
//! attribution when adding rules from either project.
//!
//! Each rule redacts capture group 1 when present, otherwise the whole match.
//! Keywords feed a case-insensitive prefilter: a rule only runs when one of its
//! keywords appears in the text.

use super::SecretTier;

pub(super) struct Rule {
    pub id: &'static str,
    pub tier: SecretTier,
    pub keywords: &'static [&'static str],
    pub pattern: &'static str,
    pub min_entropy: f32,
    /// Rejects values made only of letters, `_`, `.` and `-`, which are usually
    /// identifiers rather than secrets.
    pub reject_word_like: bool,
}

/// Matches `<keyword>... = <value>` style assignments in code, config, and
/// environment files, capturing only the value.
macro_rules! assigned {
    ($keyword:literal, $value:literal) => {
        concat!(
            r#"(?i)"#,
            $keyword,
            r#"[[:word:].-]{0,20}[\s'"]{0,3}(?:=|:{1,3}=|:|=>)[\x60'"\s=]{0,5}("#,
            $value,
            r#")(?:[\x60'"\s;,]|\\[nr]|$)"#
        )
    };
}

const fn provider(
    id: &'static str,
    keywords: &'static [&'static str],
    pattern: &'static str,
    min_entropy: f32,
) -> Rule {
    Rule {
        id,
        tier: SecretTier::ProviderTokens,
        keywords,
        pattern,
        min_entropy,
        reject_word_like: false,
    }
}

const fn credential(
    id: &'static str,
    keywords: &'static [&'static str],
    pattern: &'static str,
    min_entropy: f32,
) -> Rule {
    Rule {
        id,
        tier: SecretTier::Credentials,
        keywords,
        pattern,
        min_entropy,
        reject_word_like: false,
    }
}

pub(super) const RULES: &[Rule] = &[
    // AI and inference providers.
    provider(
        "openai-api-key",
        &["t3blbkfj"],
        r"\b(sk-(?:proj|svcacct|admin)-(?:[A-Za-z0-9_-]{74}|[A-Za-z0-9_-]{58}|[A-Za-z0-9_-]{20})T3BlbkFJ(?:[A-Za-z0-9_-]{74}|[A-Za-z0-9_-]{58}|[A-Za-z0-9_-]{20})\b|sk-[a-zA-Z0-9]{20}T3BlbkFJ[a-zA-Z0-9]{20})",
        3.0,
    ),
    provider(
        "anthropic-api-key",
        &["sk-ant-api03"],
        r"\b(sk-ant-api03-[a-zA-Z0-9_\-]{93}AA)",
        0.0,
    ),
    provider(
        "anthropic-admin-api-key",
        &["sk-ant-admin01"],
        r"\b(sk-ant-admin01-[a-zA-Z0-9_\-]{93}AA)",
        0.0,
    ),
    provider("google-api-key", &["aiza"], r"\b(AIza[[:word:]-]{35})", 4.0),
    provider("groq-api-key", &["gsk_"], r"(?i)\b(gsk_[A-Z0-9]{52})", 3.5),
    provider(
        "xai-api-key",
        &["xai-"],
        r"(?i)\b(xai-[A-Za-z0-9_-]{70,120})",
        3.5,
    ),
    provider(
        "openrouter-api-key",
        &["sk-or-v1-"],
        r"(?i)\b(sk-or-v1-[0-9a-f]{64})",
        0.0,
    ),
    provider(
        "huggingface-access-token",
        &["hf_"],
        r"\b(hf_(?i:[a-z]{34}))",
        2.0,
    ),
    provider(
        "huggingface-organization-token",
        &["api_org_"],
        r"\b(api_org_(?i:[a-z]{34}))",
        2.0,
    ),
    provider(
        "replicate-api-token",
        &["r8_"],
        r"(?i)\b(r8_[A-Za-z0-9]{37})",
        3.0,
    ),
    provider(
        "perplexity-api-key",
        &["pplx-"],
        r"\b(pplx-[a-zA-Z0-9]{48})",
        4.0,
    ),
    provider(
        "cerebras-api-key",
        &["csk-"],
        r"(?i)\b(csk-[a-z0-9]{48})",
        0.0,
    ),
    provider(
        "together-api-key",
        &["tgp_v1_"],
        r"(?i)\b(tgp_v1_[A-Za-z0-9_-]{43})",
        0.0,
    ),
    provider(
        "langsmith-api-key",
        &["lsv2_"],
        r"\b(lsv2_(?:pt|sk)_[0-9a-fA-F]{32}_[0-9a-fA-F]{10})",
        0.0,
    ),
    provider(
        "aws-bedrock-api-key",
        &["absk"],
        r"\b(ABSK[A-Za-z0-9+/]{109,269}={0,2})",
        0.0,
    ),
    // Source control, package registries, and developer platforms.
    provider(
        "github-token",
        &["ghp_", "gho_", "ghu_", "ghs_", "ghr_"],
        r"\b((?:ghp|gho|ghu|ghs|ghr)_[0-9a-zA-Z]{36})",
        0.0,
    ),
    provider(
        "github-fine-grained-pat",
        &["github_pat_"],
        r"\b(github_pat_[[:word:]]{82})",
        0.0,
    ),
    provider("gitlab-pat", &["glpat-"], r"\b(glpat-[[:word:]-]{20})", 0.0),
    provider(
        "npm-access-token",
        &["npm_"],
        r"(?i)\b(npm_[a-z0-9]{36})",
        2.0,
    ),
    provider(
        "pypi-upload-token",
        &["pypi-ageichlwas5vcmc"],
        r"\b(pypi-AgEIcHlwaS5vcmc[[:word:]-]{50,1000})",
        0.0,
    ),
    provider(
        "vercel-token",
        &["vck_", "vcp_"],
        r"(?i)\b(vc[kp]_[A-Za-z0-9_-]{56})",
        0.0,
    ),
    provider(
        "databricks-api-token",
        &["dapi"],
        r"\b(dapi[a-f0-9]{32}(?:-\d)?)",
        3.0,
    ),
    provider(
        "digitalocean-token",
        &["dop_v1_", "doo_v1_", "dor_v1_"],
        r"\b(do[por]_v1_[a-f0-9]{64})",
        0.0,
    ),
    provider(
        "linear-api-key",
        &["lin_api_"],
        r"\b(lin_api_(?i:[a-z0-9]{40}))",
        2.0,
    ),
    provider(
        "notion-api-token",
        &["ntn_"],
        r"\b(ntn_[0-9]{11}[A-Za-z0-9]{32}[A-Za-z0-9]{3})",
        0.0,
    ),
    provider(
        "doppler-token",
        &["dp.pt.", "dp.st."],
        r"\b(dp\.(?:pt|st)\.(?i:[a-z0-9]{43}))",
        2.0,
    ),
    provider(
        "shopify-token",
        &["shpat_", "shpca_", "shppa_", "shpss_"],
        r"\b(shp(?:at|ca|pa|ss)_[a-fA-F0-9]{32})",
        0.0,
    ),
    provider(
        "onepassword-service-account-token",
        &["ops_eyj"],
        r"\b(ops_eyJ[a-zA-Z0-9+/]{250,}={0,3})",
        0.0,
    ),
    // Cloud, messaging, and payments.
    provider(
        "aws-access-key-id",
        &["a3t", "akia", "asia", "abia", "acca"],
        r"\b((?:A3T[A-Z0-9]|AKIA|ASIA|ABIA|ACCA)[A-Z2-7]{16})\b",
        3.0,
    ),
    provider(
        "azure-ad-client-secret",
        &["q~"],
        r#"(?:^|[\\'"\x60\s>=:(,)])([a-zA-Z0-9_~.]{3}\dQ~[a-zA-Z0-9_~.-]{31,34})(?:$|[\\'"\x60\s<),])"#,
        3.0,
    ),
    provider(
        "slack-bot-token",
        &["xoxb"],
        r"\b(xoxb-[0-9]{10,13}-[0-9]{10,13}[a-zA-Z0-9-]*)",
        0.0,
    ),
    provider(
        "slack-user-token",
        &["xoxp", "xoxe"],
        r"\b(xox[pe](?:-[0-9]{10,13}){3}-[a-zA-Z0-9-]{28,34})",
        0.0,
    ),
    provider(
        "slack-app-token",
        &["xapp"],
        r"(?i)\b(xapp-\d-[A-Z0-9]+-\d+-[a-z0-9]+)",
        0.0,
    ),
    provider(
        "slack-webhook-url",
        &["hooks.slack.com"],
        r"(?i)(hooks\.slack\.com/(?:services|workflows|triggers)/[A-Za-z0-9+/]{43,56})",
        0.0,
    ),
    provider(
        "stripe-api-key",
        &[
            "sk_test", "sk_live", "sk_prod", "rk_test", "rk_live", "rk_prod",
        ],
        r"\b((?:sk|rk)_(?:test|live|prod)_[a-zA-Z0-9]{10,99})",
        2.0,
    ),
    provider(
        "sendgrid-api-token",
        &["sg."],
        r"\b(SG\.(?i:[a-z0-9=_\-\.]{66}))",
        2.0,
    ),
    // Structured credentials.
    provider(
        "jwt",
        &["ey"],
        r"\b(ey[a-zA-Z0-9]{17,}\.ey[a-zA-Z0-9/\\_-]{17,}\.(?:[a-zA-Z0-9/\\_-]{10,}={0,2})?)",
        3.0,
    ),
    provider(
        "private-key",
        &["private key"],
        r"(?i)(-----BEGIN[ A-Z0-9_-]{0,100}PRIVATE KEY(?: BLOCK)?-----[\s\S-]{64,}?KEY(?: BLOCK)?-----)",
        0.0,
    ),
    // Credentials that need surrounding context to identify.
    credential(
        "credential-uri",
        &["://"],
        r#"(?i)\b(?:https?|postgres(?:ql)?|mysql|mariadb|mongodb(?:\+srv)?|rediss?|amqps?|ldaps?|smtps?|ftps?|ssh)://[^:/@\s'"\x60]{0,128}:([^/@\s'"\x60]{1,256})@"#,
        0.0,
    ),
    credential(
        "aws-secret-access-key",
        &["aws", "secret", "access"],
        assigned!(r"(?:aws|secret|access)", r"[A-Za-z0-9/+=]{40}"),
        4.0,
    ),
    credential(
        "deepseek-api-key",
        &["deepseek"],
        assigned!("deepseek", r"sk-[a-f0-9]{32}"),
        3.0,
    ),
    credential(
        "mistral-api-key",
        &["mistral"],
        assigned!("mistral", r"[a-z0-9]{32}"),
        3.0,
    ),
    credential(
        "cohere-api-key",
        &["cohere"],
        assigned!("cohere", r"[a-z0-9]{40}"),
        3.0,
    ),
    credential(
        "azure-openai-api-key",
        &["azure"],
        assigned!("azure", r"[a-f0-9]{32}"),
        3.0,
    ),
    credential(
        "twilio-api-key",
        &["twilio"],
        assigned!("twilio", r"SK[0-9a-f]{32}"),
        3.0,
    ),
    // Opt-in: gitleaks `generic-api-key`, which trades precision for recall.
    Rule {
        id: "generic-api-key",
        tier: SecretTier::Generic,
        keywords: &[
            "access",
            "api",
            "auth",
            "credential",
            "creds",
            "key",
            "passw",
            "secret",
            "token",
        ],
        pattern: r#"(?i)[[:word:].-]{0,50}?(?:access|auth|(?-i:[Aa]pi|API)|credential|creds|key|passw(?:or)?d|secret|token)(?:[ \t[:word:].-]{0,20})[\s'"]{0,3}(?:=|>|:{1,3}=|\|\||:|=>|\?=|,)[\x60'"\s=]{0,5}([[:word:].=-]{10,150}|[a-z0-9][a-z0-9+/]{11,}={0,3})(?:[\x60'"\s;]|\\[nr]|$)"#,
        min_entropy: 3.5,
        reject_word_like: true,
    },
];
