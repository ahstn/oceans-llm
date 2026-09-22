# Guardrail Secret Redaction For Outbound Prompts

`See also`: [Gateway Guardrails](../operations/gateway-guardrails.md), [ADR: Gateway Guardrails Domain and Composition](../adr/2026-08-22-gateway-guardrails-domain-and-composition.md), [PR #305](https://github.com/ahstn/oceans-llm/pull/305)

- Date: 2026-09-22
- Status: Implemented. See [ADR: Guardrail Secret Redaction](../adr/2026-09-22-guardrail-secret-redaction.md) and [Gateway Guardrails](../operations/gateway-guardrails.md#secret-redaction).
- Primary target: detect API keys and credentials in inference requests and replace them with `[REDACTED:<rule_id>]` before the request leaves Oceans LLM for a provider or a managed guardrail service

## Final decisions

The implementation differs from the draft below in these ways:

- The placeholder is `[REDACTED:<rule_id>]`, not `[REDACTED]`, so reviewers can tell which rule fired.
- Redaction never denies, in either policy mode.
- Request-log payload redaction ships in the same change. It runs inside `redact_json_value_with_policy`, before truncation, for requests, responses, stream events, attempts, and MCP invocations.
- Redaction walks every JSON string in the request, not only `PROMPT_TEXT_FIELDS`. Tool-call arguments and tool results are covered without per-protocol pointer lists.
- There is no `DeterministicTransformer` trait and no `phases` setting. `redact_prompt_secrets` runs in `guard_prompt` before `inspect_text_fields`.
- The rule table is compiled once into a static scanner and filtered by configuration at match time. There is no per-policy redactor.
- CRC32 checksum validation was dropped. Prefix anchoring and entropy were precise enough, and it avoided a dependency.
- Module layout is `redaction/{mod,rules,scanner,tests}.rs`.

## Summary

Add a deterministic, in-process secret redactor to `gateway-guardrails`. It runs first in the prompt phase, before built-in packs and before managed checks. Amazon Bedrock and Google Model Armor are third parties too, so they should also receive only redacted text.

Detection follows the approach that gitleaks, Betterleaks, TruffleHog, and Kingfisher have converged on:

1. A case-insensitive Aho-Corasick keyword prefilter selects candidate rules in one pass.
2. Candidate rules run anchored regexes. Each regex has a capture group that marks the secret span.
3. A per-rule Shannon entropy minimum, checksum validation where the format has one, and placeholder allowlists reject false positives.
4. Overlapping spans are merged and replaced with `[REDACTED]`.

Rules live in a static Rust table, ported from the MIT-licensed gitleaks and Betterleaks configs with attribution. No runtime rule loading and no new Python or C dependencies.

## Research notes

### LiteLLM `hide_secrets`

Source: `enterprise/litellm_enterprise/enterprise_callbacks/secret_detection.py` and `secrets_plugins/`.

- Wraps Yelp `detect-secrets`. About 16 built-in plugins plus 94 custom `RegexBasedDetector` plugins, mostly direct ports of gitleaks rules.
- Writes every message to a temp file, scans it, then does `text.replace(secret, "[REDACTED]")`.
- Only walks user text, `prompt`, and `input`. Tool-call arguments and tool results depend on `walk_user_text` coverage.
- Weaknesses to avoid:
  - OpenAI rule `sk-[a-zA-Z0-9]{5,}` is loose enough to cause false positives, and there is no Anthropic, Groq, xAI, OpenRouter, or other modern AI-provider rule.
  - `Base64HighEntropyString` and `HexHighEntropyString` run at limit `3.0`, far below detect-secrets' own defaults (`4.5` base64, `3.0` hex), which over-redacts ordinary identifiers.
  - Per-message temp files and a Python scan per plugin add latency.
  - Callers can opt out per key through `permissions.hide_secrets`. Oceans LLM guardrails deliberately do not allow caller bypass.

### Other tools

| Tool | Relevance |
|---|---|
| gitleaks (MIT) | Canonical rule format: `regex`, `secretGroup`, `entropy`, `keywords`, per-rule allowlists and stopwords. 222 rules, RE2 syntax, so they compile in Rust `regex` unchanged. |
| Betterleaks (MIT) | 2026 gitleaks successor with 463 rules and the best AI-provider coverage (OpenAI `T3BlbkFJ` marker, Groq, xAI, OpenRouter, Replicate, Cerebras, Together, LangSmith, Vercel, Bedrock `ABSK`). CEL `filter` and `validate` blocks are not portable. |
| TruffleHog (AGPL) | Aho-Corasick keyword prefilter, then per-detector regex on a window around the hit. Design reference only; do not vendor. |
| Kingfisher / Nosey Parker (Apache-2.0) | Rust. Vectorscan multi-pattern matching. Kingfisher now imports Betterleaks rules. Vectorscan is unnecessary at prompt sizes. |
| GitHub token format | `ghp_` and siblings end in a 6-character base62 CRC32 checksum, so offline validation removes nearly all false positives. `npm_` uses the same scheme. |
| detect-secrets, Presidio, NeMo, Kong, Portkey | Either Python wrappers of detect-secrets or PII-oriented. None has a stronger secret catalog than gitleaks plus Betterleaks. |
| secrets-patterns-db | CC-BY-SA share-alike. Do not vendor. |

Sources: [gitleaks config](https://github.com/gitleaks/gitleaks/blob/master/config/gitleaks.toml), [Betterleaks config](https://github.com/betterleaks/betterleaks/blob/main/config/betterleaks.toml), [TruffleHog Aho-Corasick](https://trufflesecurity.com/blog/making-trufflehog-faster-with-aho-corasick), [GitHub token formats](https://github.blog/engineering/platform-security/behind-githubs-new-authentication-token-formats/), [detect-secrets entropy plugins](https://github.com/Yelp/detect-secrets/blob/master/detect_secrets/plugins/high_entropy_strings.py), [LiteLLM secret detection](https://docs.litellm.ai/docs/proxy/guardrails/secret_detection).

## Current integration facts

- `DeterministicEvaluator::evaluate` returns `Option<MatchedRule>` only (`crates/gateway-guardrails/src/evaluation.rs:12`). Only `ManagedOutcome::Transformed` can rewrite content today. Redaction needs a new deterministic transform path, not another pack in `BuiltInEvaluator`.
- `BuiltInEvaluator` ignores `Text` and `TextSegments` payloads, so built-in packs do nothing in the prompt phase today.
- `guard_prompt` (`crates/gateway/src/http/inference_guardrails.rs:44`) builds `TextSegments` from strings under the keys in `PROMPT_TEXT_FIELDS` (`content`, `description`, `input`, `instructions`, `name`, `prompt`, `summary`, `text`, `title`). It writes transformed segments back by JSON pointer (`:816-840`).
- Write-back happens on the core or OpenAI-shaped request before provider translation, so it covers every provider and `/v1/messages`, chat, responses, embeddings, and batch items.
- Not currently inspected: assistant `tool_calls[].function.arguments`, Responses `function_call_output.output`, Anthropic `tool_use.input` objects. Agent traffic regularly carries secrets in exactly these fields, for example a tool that ran `cat .env`.
- Transformations apply regardless of `audit` or `deny` mode. The policy must have `enabled: true` for any prompt guard to run.
- Request payload logs are captured before guarding (`crates/gateway/src/http/handlers.rs:161`, `:473`; `crates/gateway-service/src/request_logging.rs:284`). A redacted secret would still be stored in Oceans LLM's own request log. `gateway-service/src/redaction.rs` only redacts by key name and path.
- `regex` and `aho-corasick` are only transitive dependencies today. `crc32fast` is also already in `Cargo.lock`.

## Design

### Module layout

```text
crates/gateway-guardrails/src/
  redaction.rs               # SecretRedactor, RedactionOutcome, span merge, payload walk
  redaction/rules.rs         # static SecretRule table with gitleaks/Betterleaks attribution
  redaction/entropy.rs       # Shannon entropy on the captured span
  redaction/checksum.rs      # GitHub/npm base62 CRC32 validation
  redaction/allowlist.rs     # placeholder and template rejections
  redaction/tests.rs         # per-rule golden positives and negatives
```

Add `regex`, `aho-corasick`, and `crc32fast` to `[workspace.dependencies]` and to `gateway-guardrails`.

### Rule model

```rust
pub(crate) struct SecretRule {
    pub id: &'static str,              // "anthropic-api-key", stable, used as rule_id
    pub tier: SecretTier,              // ProviderToken | Credential | Generic
    pub keywords: &'static [&'static str], // lowercase prefilter literals
    pub pattern: &'static str,         // RE2-compatible, one secret capture group
    pub secret_group: usize,
    pub min_entropy: Option<f32>,
    pub checksum: Option<Checksum>,    // GithubCrc32, NpmCrc32
}
```

- Every rule must have at least one keyword. Rules with no keyword hit never run.
- `SecretRedactor::new(&[SecretTier], &disabled_rule_ids)` compiles the Aho-Corasick automaton and the regexes once at startup. Build regexes with `RegexBuilder` and a raised `size_limit`. A unit test compiles every rule so a bad pattern fails CI, not startup.
- Use the gitleaks boundary suffix ``(?:[\x60'"\s;]|\\[nr]|$)`` so tokens inside JSON-escaped or code-escaped text still match.

### Tiers

1. `provider_tokens`, on by default. Prefix-anchored formats with low false-positive rates:
   - AI providers: OpenAI (`sk-proj-`, `sk-svcacct-`, `sk-admin-`, and legacy keys, all anchored on `T3BlbkFJ`), Anthropic `sk-ant-api03-` and `sk-ant-admin01-`, Google `AIza`, Groq `gsk_`, xAI `xai-`, OpenRouter `sk-or-v1-`, Hugging Face `hf_` and `api_org_`, Replicate `r8_`, Perplexity `pplx-`, Cerebras `csk-`, Together `tgp_v1_`, LangSmith `lsv2_`, Vercel `vck_` and `vcp_`, Bedrock `ABSK`.
   - Platforms: GitHub (`ghp_`, `gho_`, `ghu_`, `ghs_`, `ghr_` with CRC32, `github_pat_`), GitLab `glpat-` and siblings, Slack `xox[bpeoa]-` and `xapp-`, Slack webhook URLs, AWS access key IDs (`AKIA`, `ASIA`, `ABIA`, `ACCA`), Stripe `sk_`/`rk_` live/test, npm `npm_` with CRC32, PyPI, SendGrid `SG.`, Databricks `dapi`, DigitalOcean `do[por]_v1_`, Linear `lin_api_`, Notion `ntn_`, Doppler `dp.pt.`/`dp.st.`, Shopify `shp(at|ca|pa|ss)_`, 1Password `ops_eyJ`, Azure AD client secrets.
   - Structural: PEM private key blocks (redact the whole block) and JWTs.
2. `credentials`, on by default. Keyword-context rules with medium confidence:
   - Credential URIs (`postgres://user:pass@host` and siblings). Redact only the password group so the prompt stays useful.
   - The AWS secret access key heuristic: a 40-character base64 value next to `secret`, `access`, `key`, or `token`, with entropy above 4.0.
   - Keyword-bound provider keys whose bare format is ambiguous: DeepSeek, Mistral, Cohere, Twilio `SK`.
3. `generic`, opt-in. The gitleaks `generic-api-key` rule with entropy 3.5, its match allowlist, and a trimmed stopword list. Prompts often talk about keys, so this tier will over-redact and must be an explicit choice.

Leave out standalone high-entropy base64 and hex detection. It is the main source of LiteLLM's false positives, and long hashes, UUIDs, and base64 blobs are normal in agent traffic.

### False-positive filters

Applied to the captured span after the regex matches:

- Per-rule `min_entropy`.
- Checksum validation for GitHub and npm.
- Placeholder rejection: contains `EXAMPLE` (case-sensitive, matching AWS docs keys), `your_`, `<`/`>`, `${`, `{{`, `%VAR%`, or is dominated by one repeated character (`xxxx`, `****`, `....`, `0000`).
- For `generic` only: reject values with no digit, UUIDs, and gitleaks stopwords.

### Redaction algorithm

```text
redact(text):
  hits = aho_corasick.find_overlapping_iter(text)  -> candidate rule set
  if empty: return Unchanged
  spans = []
  for rule in candidates (stable table order):
    for caps in rule.regex.captures_iter(text):
      span = caps[rule.secret_group]
      if passes_filters(rule, span): spans.push((range, rule.id))
  merge overlapping or adjacent ranges; keep every rule_id per merged range
  rebuild text left to right, replacing each merged range with "[REDACTED]"
  return Redacted { text, rule_counts }
```

- Replacement is always the literal `[REDACTED]`. It contains no quote or backslash, so redacting inside a JSON-encoded string such as `function.arguments` keeps the JSON valid.
- Output is deterministic for the same input, so provider prompt-cache prefixes stay stable across turns once a secret has been redacted.
- `SecretRedactor` never fails after construction, so there is no failure disposition and no content-size bypass. The Rust `regex` crate is linear-time, so large prompts cannot trigger catastrophic backtracking. Scanning everything is safe, and skipping large content would leak secrets.

### Engine integration

Add a transform stage to `GuardrailEngine` rather than overloading `DeterministicEvaluator`:

```rust
pub trait DeterministicTransformer: Send + Sync {
    fn id(&self) -> &str;
    fn applies_to(&self, phase: GuardPhase) -> bool;
    fn transform(&self, payload: &mut EvaluationPayload, policy: &EffectivePolicy)
        -> Vec<MatchedRule>;
}
```

- `GuardrailEngine::new` takes `transformers: Vec<Arc<dyn DeterministicTransformer>>` and runs them before deterministic evaluators whenever the policy has redaction enabled for the phase.
- Transformers edit the payload in place, segment by segment. They do not use `replace_text` with a JSON round trip. Add `EvaluationPayload::map_strings(&mut self, f)` that visits `Text`, each `TextSegments` entry, `ShellCommand`, and every JSON string leaf in `ToolCall`, `McpCall`, and `McpResult`. The segment count and JSON shape stay the same by construction.
- For each distinct rule that fired, push one `DecisionRecord` with `evaluator: "secret_redaction"`, `action: Transformed`, `transformed: true`, `reason_code: "secret_redaction.redacted"`, and `matched_rule { pack_id: "secret_redaction", rule_id, matched_field: <segment pointer>, ... }`. `content_hash` stays the pre-transform hash, matching managed transforms. There are about 60 stable rule IDs, so metric-label cardinality is bounded. No store migration is needed, because `action` is free text and `transformed` already exists.
- `final_action` becomes `Transformed` if nothing else was flagged, matching managed transforms. Redaction never denies, in either mode.

### Policy configuration

Add a `secret_redaction` block to `PolicyConfig` and `PolicyOverride`, resolved like the other fields:

```yaml
guardrails:
  default:
    enabled: true
    mode: audit
    secret_redaction:
      enabled: true
      tiers: [provider_tokens, credentials]   # add `generic` to opt in
      phases: [prompt]                        # default; also valid: generated_tool_call, mcp_call, mcp_result
      disabled_rules: []                      # stable rule IDs, validated at startup
  model_routes:
    local/ollama/llama3:
      secret_redaction:
        enabled: false                        # route-scoped opt-out, config only
```

- Validate unknown tiers, unknown or duplicate rule IDs, and an empty `phases` set, with new `GuardrailConfigError` variants.
- Default stays `enabled: false` for backward compatibility. Docs recommend turning it on together with the policy.
- `GatewayConfig::guardrail_engine()` builds one `SecretRedactor` per distinct tier and disabled-rule combination across the default and overrides, keyed by that combination.

### Gateway prompt-phase coverage

`PROMPT_TEXT_FIELDS` intentionally skips tool-call arguments and several tool-result shapes. For redaction, add a second, broader pointer collector used only by the redaction pass:

- Collect every string leaf in the core request except a structural skip-list: `model`, `role`, `type`, `id`, `tool_call_id`, `call_id`, `object`, `format`, `detail`, `media_type`, `mime_type`, `encoding_format`, `stop`, `metadata`, and binary payloads (`data`, `b64_json`, `image_url.url` and `file_data` values starting with `data:`).
- This covers `tool_calls[].function.arguments`, `function_call_output.output`, Anthropic `tool_use.input` leaves, and any future content key without extending the list.

Order in `guard_prompt`:

1. Resolve the policy. Return early if the policy or redaction is disabled.
2. Run the redaction pass on the broad `TextSegments` payload through the engine, write back by pointer, and record its decisions.
3. Run the existing `inspect_text_fields` pass on the already-redacted request, so managed checks and `associated_prompt` never see secrets.

The same flow covers the typed wrapper in `handlers.rs:58`, embeddings, and `batch_worker.rs:219` automatically.

### Request log payloads

The provider boundary is the primary target, but request logs currently persist the unredacted prompt. Recommended as part of this change:

- Expose `SecretRedactor::redact_json(&mut Value)` from `gateway-guardrails` and apply it in the request-log payload capture path when the effective policy has redaction enabled. The alternative, capturing the log after guarding, changes log semantics for denied requests.

### Out of scope for the first PR

- Model responses and streams. Redacting secrets a model emits needs stream buffering and would be a separate phase opt-in. The `phases` field leaves room for it.
- MCP call and result phases. The transform works on their payloads, but enabling them is follow-up work with its own tests.
- Live key verification against provider APIs.
- Runtime or admin-defined custom rules. Custom regexes can be a later `custom_rules` list with the same shape.

## Implementation steps

1. Dependencies: add `regex`, `aho-corasick`, and `crc32fast` to workspace and crate manifests.
2. `redaction/` module: rules table, entropy, checksum, allowlist, `SecretRedactor`, span merge, `map_strings` on `EvaluationPayload`.
3. Engine: `DeterministicTransformer` trait, transform stage in `GuardrailEngine::evaluate`, decision records, `lib.rs` exports.
4. Policy: `SecretRedactionConfig`, override resolution, validation, `EffectivePolicy.secret_redaction`.
5. Gateway wiring: build redactors in `config/guardrails.rs`, broad pointer collector and redaction pass in `inference_guardrails.rs`, test stub in `http/test_support.rs`.
6. Request log payload redaction.
7. Admin API and UI: expose `secret_redaction` in the policies endpoint (`crates/gateway/src/http/guardrails.rs:237`), regenerate `admin-api.ts`, and show redaction decisions (the `transformed` filter already exists).
8. Docs: new section in `docs/operations/gateway-guardrails.md`, configuration reference entry, and an ADR recording the in-process deterministic transform decision and rule provenance.

## Tests

- Build all test fixtures at runtime, for example `format!("ghp_{body}{checksum}")` or `concat!` over split literals. Committing realistic tokens will trip GitHub push protection and external secret scanners on this repo.
- Per rule: one positive, one placeholder negative, one near-miss negative (wrong length, bad checksum, low entropy). A table-driven test fails if a rule has no fixture.
- Redactor: overlapping rules, adjacent secrets, secret at start and end of text, JSON-escaped `\n` boundaries, multibyte text around secrets, PEM blocks, credential URI keeps user and host, idempotence (`redact(redact(x)) == redact(x)`).
- False-positive corpus: UUIDs, git SHAs, SHA-256 digests, base64 images, AWS `AKIAIOSFODNN7EXAMPLE`, `sk-...` in docs prose, `api_key = os.environ["X"]`, code samples with `${{ secrets.X }}`.
- Engine: redaction runs before packs and managed checks, managed fakes receive redacted text, audit and deny modes both redact and never deny, decision records carry rule IDs and no secret material.
- Gateway end to end with a fake provider: OpenAI chat, Responses, Anthropic `/v1/messages`, embeddings, and batch. Assert the upstream body contains `[REDACTED]` and no fixture token, including in tool-call arguments and tool results. Assert the request log payload is redacted.
- Performance: a test or bench that redacts a 1 MB agent transcript with no secrets and one with many secrets, with a budget of low single-digit milliseconds on CI hardware.

## Open questions

1. Should `mode: deny` turn a detected secret into a request denial instead of redaction? The recommendation is no: keep redaction mode-independent and add an explicit `action: redact | deny` later if admins ask.
2. Should the placeholder include the rule type, for example `[REDACTED:anthropic-api-key]`? It helps the model explain what happened but reveals the credential type to the provider. The recommendation is plain `[REDACTED]` as requested, with an optional flag later.
3. Should request-log payload redaction ship in this PR or immediately after?
