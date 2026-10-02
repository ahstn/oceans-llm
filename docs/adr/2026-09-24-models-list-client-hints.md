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

## Proxy endpoint types (2026-10-02)

omp (Oh My Pi) `discovery.type: proxy` reads `id`, `name`, `context_length`, and new-api's `supported_endpoint_types` from each row. It routes a model over Anthropic Messages whenever `anthropic` appears. Before this change, omp called every Oceans model over chat completions, so Claude models never used `/v1/messages`.

Each card now carries `name` (a copy of `display_name`) and `supported_endpoint_types`, ordered preferred first. `anthropic` is listed only when the preferred format is Anthropic Messages. Listing it for every chat-capable model would be accurate but would send GPT and Gemini models through the Messages translation. `openai` and `openai-response` follow route capabilities. `embeddings` is omitted: route capabilities default embeddings to enabled, so the list would claim embeddings for chat models whose upstream cannot serve them.

Decision models have no new-api endpoint type, and inventing one would only help clients that adopt it. Instead, cards for decision-capable models add `decisions` to `architecture.output_modalities`, OpenRouter's existing marker, which omp's OpenRouter discovery already reads. omp proxy discovery ignores that field and falls back to the provider-level API. Follow-up: propose an omp change that maps `decisions` rows to its `openrouter-decisions` API, whose `{baseUrl}/decisions` path matches the gateway's `/v1/decisions`.
