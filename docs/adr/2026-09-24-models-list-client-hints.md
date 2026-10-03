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

`api_formats` comes from route capabilities. The preferred format comes from the primary route's provider type, when the type determines it. Model-name matching is kept only as a fallback for generic OpenAI-compatible providers and Bedrock Converse routes. `ClientApiStyle` was renamed to the public `ApiFormat`, and `ClientConfigInput` gained `native_api_format`, so generated configs use the same rule.

## Exclusions

Hints never include a base URL or API key: the client already knows the URL it called and holds its own key. Pricing, tags, and route provenance stay in `/v1/model-metadata`, which versions its schema. There is no `shutdown_date` field, because the gateway does not track model retirement.

## Trade-offs

`owned_by` changed from `gateway` to `oceans-llm`, and `created` is no longer always zero. Neither field has a documented meaning beyond display. Catalog-derived values change when the snapshot refreshes, so contract tests pin only the gateway-owned fields.

Validation: `gateway-client-config` hint tests, a catalog-backed listing test in `gateway-service`, the authenticated handler test, the E2E gateway contract, workspace Clippy, and Rust formatting.

## Proxy endpoint types (2026-10-02)

omp (Oh My Pi) `discovery.type: proxy` reads `id`, `name`, `context_length`, and new-api's `supported_endpoint_types` from each row. It routes a model over Anthropic Messages whenever `anthropic` appears. Before this change, omp called every Oceans model over chat completions, so Claude models never used `/v1/messages`.

Each card now carries `name` (a copy of `display_name`) and `supported_endpoint_types`, ordered preferred first. `anthropic` is listed only when the preferred format is Anthropic Messages. Listing it for every chat-capable model would be accurate but would send GPT and Gemini models through the Messages translation. `openai` and `openai-response` follow route capabilities. `embeddings` is omitted: route capabilities default embeddings to enabled, so the list would claim embeddings for chat models whose upstream cannot serve them.

Decision models have no new-api endpoint type, and inventing one would only help clients that adopt it. Instead, cards for decision-capable models add `decisions` to `architecture.output_modalities`, OpenRouter's existing marker, which omp's OpenRouter discovery already reads. omp proxy discovery ignores that field and falls back to the provider-level API. Follow-up: propose an omp change that maps `decisions` rows to its `openrouter-decisions` API, whose `{baseUrl}/decisions` path matches the gateway's `/v1/decisions`.

## Review follow-ups (2026-10-03)

- **Anthropic request features.** `/v1/messages` passes Anthropic content blocks, `thinking`, and `output_config` to the route untranslated. Only Anthropic-shaped adapters consume them. `effort`, `thinking`, `image_input`, `pdf_input`, and `structured_outputs` in `capabilities` are therefore reported only when the primary route's native format is Anthropic Messages. Otherwise Claude Code would send fields that an OpenAI-compatible or Gemini upstream rejects. Bedrock now derives its native format from the route `api_style`, so Claude on Bedrock keeps these capabilities. Claude on Converse does not, because Converse fronts many families. Translating these fields for other upstreams would let the gate widen later.
- **Reasoning ceiling.** The listing and generated configs use the strictest `max_reasoning_effort` across the alias chain, the same rule as request enforcement. Previously they used only the first value set. `ClientConfigInput` gained `max_reasoning_effort` (`ReasoningLevel`). OpenCode `variants` above the ceiling are dropped. Pi `thinkingLevelMap` values above it are clamped, so every preset a harness offers is accepted by the gateway.
- **Responses-native routes.** When the primary route's native format is the Responses API, it is preferred even if chat completions is also served. This keeps the provider-derived format authoritative, matching the Anthropic Messages rule.
- **Every routable route gates Anthropic features.** `WeightedRoutePlanner` picks by weight within a priority tier and falls back to later tiers, so `/v1/messages` can land on any eligible route. The Anthropic request features above therefore require every eligible chat-capable route to be Anthropic-native, not just the primary route. `preferred_api_format` remains a primary-route preference, because it chooses a wire rather than promising that request fields survive.
- **Pi clamping stays within the policy.** Pi levels above the ceiling clamp to the strongest effort that the thinking policy itself sends within the ceiling. If there is none, the levels become `null`. This stops a `minimal` ceiling from rewriting Claude presets to `minimal`, which the Anthropic adapters reject.
- **Endpoint order.** `supported_endpoint_types` lists `openai-response` before `openai` when Responses is the preferred format, so it stays preferred-first.
