# ADR: Guardrail Secret Redaction

Date: 2026-09-22

## Status

Accepted

## Decision

`gateway-guardrails` redacts API keys, tokens, and other credentials from model-route prompts before any other prompt evaluator runs and before the request reaches a provider. Each detected secret becomes `[REDACTED:<rule_id>]`. Redaction is a transformation, not a deny. It applies in both `audit` and `deny` modes.

Redaction is configured per policy with `secret_redaction: {enabled, tiers, disabled_rules}`. It is off by default. The `provider_tokens` and `credentials` tiers are on by default once redaction is enabled. The `generic` tier is opt-in. Model-route and MCP-server overrides set individual fields and inherit the rest.

Detection uses a static rule table adapted from gitleaks and Betterleaks, both MIT-licensed. A case-insensitive Aho-Corasick keyword prefilter selects candidate rules. Each candidate runs an anchored regex, and a per-rule Shannon-entropy minimum and placeholder filters reject the capture. Overlapping findings are merged. The scanner walks every string in the JSON request and skips inline base64 media, identified by `b64_json`, base64 `data:` URIs, or a `data` string beside a media type or format field.

Each redacted request records one `transformed` decision per matched rule, with evaluator `secret_redaction`. Decision content hashes cover only the redacted text.

The same detection redacts captured request-log payloads when any enabled policy redacts secrets. The log redactor uses the union of the enabled tiers across model-route and MCP-server policies, and disables a rule only if every redacting policy disables it.

## Rationale

Agents regularly paste `.env` files and command output into prompts, and tool results carry credentials back into later turns. Providers and managed guardrail services are third parties, so the gateway must remove secrets before either sees them. Denying the request would break the agent loop over data the user usually did not mean to send. Redacting keeps the request useful.

Rule-named placeholders show reviewers and models what was removed without revealing any part of the value. Walking every string covers tool-call arguments and tool results across protocols without per-protocol pointer lists. A static table compiled once keeps latency predictable and avoids runtime rule loading.

## Trade-Offs

Pattern detection misses secrets without a recognizable shape or keyword, and the `generic` tier trades precision for recall. Operators can disable individual noisy rules. Responses are not redacted for the caller. Only their request-log copies are, and a streamed secret split across deltas is not redacted in the logged events.

TruffleHog (AGPL) and secrets-patterns-db (CC-BY-SA) informed the design but are not vendored.
