# Enriched model list and client hints

Status: Accepted

## Context

`GET /v1/models` returned only `id`, `object`, `created: 0`, and `owned_by: "gateway"`. Claude Code reads the same endpoint through `ANTHROPIC_BASE_URL` and expects Anthropic's `display_name`, `created_at`, `max_input_tokens`, `max_tokens`, and `capabilities`. OpenCode and Pi users had to copy the npm package, API style, and compatibility flags from generated snippets. OpenRouter shows that clients accept a richer list response.

## Decision

Keep one `/v1/models` response for both API families and add fields only. OpenAI and Anthropic list keys do not collide, so both SDKs parse the same payload.

- Add Anthropic's `type`, `display_name`, `created_at`, `max_input_tokens`, `max_tokens`, and top-level `has_more`, `first_id`, `last_id`.
- Emit `capabilities` in Anthropic's exact `ModelCapabilities` shape, because Anthropic SDKs type it. Gateway-specific data lives elsewhere.
- Populate `created` from the catalog release date. `owned_by` becomes `oceans-llm`, the value generated client configs already use as the provider ID.
- Add `alias_of`, `context_length`, and `architecture` modalities, following OpenRouter's naming.
- Add `client_hints`, built by `gateway-client-config` from the same helpers as the rendered templates, so hints and snippets cannot disagree.

`api_formats` comes from route capabilities. The preferred format comes from the primary route's provider type, when the type determines it. Model-name matching is kept only as a fallback for generic OpenAI-compatible and Bedrock providers. `ClientApiStyle` was renamed to the public `ApiFormat`, and `ClientConfigInput` gained `native_api_format`, so generated configs use the same rule.

## Exclusions

Hints never include a base URL or API key: the client already knows the URL it called and holds its own key. Pricing, tags, and route provenance stay in `/v1/model-metadata`, which versions its schema. There is no `shutdown_date` field, because the gateway does not track model retirement.

## Trade-offs

`owned_by` changed from `gateway` to `oceans-llm`, and `created` is no longer always zero. Neither field has a documented meaning beyond display. Catalog-derived values change when the snapshot refreshes, so contract tests pin only the gateway-owned fields.

Validation: `gateway-client-config` hint tests, a catalog-backed listing test in `gateway-service`, the authenticated handler test, the E2E gateway contract, workspace Clippy, and Rust formatting.
