# Vertex Gemini 3 Thinking Support and Adapter Restructure

## Status

Accepted.

## Context

Gemini 3.7 Flash and 3.8 Flash on Vertex AI (Agent Platform) are the primary Google models routed through Oceans. An audit of `crates/gateway-providers/src/vertex/` against the official OpenAI-compatibility docs, Pi, oh-my-pi, and LiteLLM found request and response gaps that break these models for real clients:

- `reasoning_effort`, `max_completion_tokens`, `stream_options`, `response_format`, and other OpenAI fields were forwarded to the body root, which Vertex rejects with `400 INVALID_ARGUMENT`. The generated Pi client config emits `reasoning_effort` on every request, so every Pi user hit this.
- Caller-supplied `generationConfig` overwrote the mapped one instead of merging.
- Thought parts (`thought: true`) were concatenated into user-visible `content`; `thoughtsTokenCount` and `cachedContentTokenCount` were dropped from usage, under-billing thinking responses.
- `functionCall.id` / `functionResponse.id` were sent unconditionally; Vertex rejects them on models before Gemini 3.5. Text-part `thoughtSignature` was dropped, which Gemini 3 requires on the next turn.
- `MALFORMED_FUNCTION_CALL`, prompt blocks, and in-band `{error}` stream objects were reported as clean `stop` turns.
- Tool schemas were sent under the OpenAPI-subset `parameters` field with `strict` and `$schema` intact.
- Anthropic-on-Vertex duplicated `crates/gateway-providers/src/anthropic/` (393 lines of Claude thinking policy) and gated `context_management` on an `anthropic-beta` HTTP header that Vertex `rawPredict` ignores.
- `gateway-client-config` only knew Anthropic thinking policies, so Gemini models rendered `reasoning: false` in Pi and had no OpenCode variants.

## Decision

1. Restructure the Vertex adapter by publisher and by direction:
   - `google_request.rs`, `google_tools.rs`, `google_response.rs`, `google_stream.rs` own the Gemini path.
   - `anthropic.rs` is a thin wrapper over the shared `crate::anthropic` adapter; the duplicated thinking module is deleted.
   - `gemini.rs` holds the model-family table (`GeminiModel`): thinking control style, level collapsing for Pro, budget values for 2.5, and function-id support.
   - `error.rs` introduces `VertexAdapterError` (`thiserror`) with `From<VertexAdapterError> for ProviderError`; ad-hoc `InvalidRequest(format!(...))` strings are removed.
2. Map OpenAI fields in a single pass over `request.extra`: sampling keys into `generationConfig`, `reasoning_effort` / `reasoning.effort` into `thinkingConfig` (`thinkingLevel` for 3.x, `thinkingBudget` for 2.5, `includeThoughts: true`; `"none"` disables), `response_format` into `responseMimeType` / `responseJsonSchema`, `max_completion_tokens` aliased to `maxOutputTokens`, and OpenAI-only keys dropped. Caller `generationConfig` deep-merges over the mapped keys. Conflicts (two different max-token values, `reasoning_effort` alongside a caller `thinkingConfig`) are typed errors.
   - `GeminiModel` encodes the Gemini 3.7+ conventions from the official model guides: `MINIMAL` exists only on Flash / Flash-Lite before 3.7 (later models and Pro start at `LOW`); `temperature`/`top_p`/`top_k` are ignored upstream and dropped; `presence_penalty`/`frequency_penalty`/`n` are rejected unless they carry the no-op default. The client-config `ThinkingPolicy::GeminiLevel { supports_minimal }` mirrors the same rule so Pi and OpenCode never offer `minimal` on 3.7+.
3. Emit tool declarations as `parametersJsonSchema` (full JSON Schema passthrough, `strict` stripped) and support `tool_choice: "validated"`. Emit `functionCall.id` only for Gemini >= 3.5. Replay `thought_signature` from `tool_calls[i]` and from the assistant message onto the matching part.
4. Normalize thoughts into `reasoning_content`, surface text-part signatures as `message.thought_signature` and `provider_metadata.gcp_vertex.thought_signature`, add `thoughtsTokenCount` into `completion_tokens` with `completion_tokens_details.reasoning_tokens`, and map `cachedContentTokenCount` to `prompt_tokens_details.cached_tokens`. `RECITATION`, `LANGUAGE`, `SPII` map to `content_filter`. `MALFORMED_FUNCTION_CALL` and in-band `{error}` objects become `ProviderError::Transport`; a prompt block with no candidates stays an OpenAI `content_filter` finish.
5. Stream Gemini over `streamGenerateContent?alt=sse` with the shared `SseEventParser` and a unit-testable `GoogleStreamState` (`on_response` / `finish`), replacing the hand-rolled JSON array parser.
6. Move `anthropic-beta` values from provider default headers and route headers into the `anthropic_beta` body array for `rawPredict`, appending `effort-2025-11-24` when `output_config.effort` is used with manual thinking.
7. Derive `api_host` from `location` when unset (`global`, `us`/`eu` multi-region, regional). Batch text-embedding `predict` calls at 250 instances (one for `gemini-embedding-001`, which Vertex limits to a single input per request) and a 20,000-token aggregate budget estimated locally (two ASCII chars per token, two tokens per non-ASCII char); validate the prediction count per batch.
8. Replace `AnthropicThinkingPolicy` in `gateway-client-config` with a provider-neutral `ThinkingPolicy` (`AnthropicSafeEffort`, `AnthropicManualBudget`, `GeminiLevel { supports_minimal, supports_medium }`, `GeminiBudget`). Pi renders `reasoning: true` plus a `thinkingLevelMap` for Gemini; OpenCode renders `reasoningEffort` variants.

## Implementation

- `crates/gateway-providers/src/vertex/{mod,error,gemini,google_request,google_tools,google_response,google_stream,anthropic,embeddings}.rs`
- `crates/gateway-providers/src/vertex/tests/` rewritten around behavior: wire shape, precedence, and error paths, with Gemini 3.7 fixtures.
- `crates/gateway/src/config/providers.rs`: `api_host: Option<String>` with `resolved_api_host()`; `gateway_providers::vertex_api_host_for_location` is the single source for the mapping.
- `crates/gateway-client-config/src/{types,thinking}.rs`, `templates/{pi,opencode,notes}.rs`; caller `gateway-service/src/admin_models.rs`.
- `docs/providers/gcp-vertex.md`, `docs/configuration/configuration-reference.md`.

## Trade-Offs

- Gemini 2.5 budgets (128 / 2048 / 8192 / 24576, Pro high 32768) are borrowed from Pi; they are a policy choice, not a Google-documented mapping.
- `functionCall.id` gating on Gemini >= 3.5 follows LiteLLM and Pi; the boundary has not been confirmed against 3.7 Flash with a live call.
- Advertising `json_schema` in provider capabilities is new; it only holds for Gemini routes, and the route-aware capability computation in `admin_models` still decides per route.

## Follow-Up

- Live-verify against a Vertex project: `functionCall.id` acceptance on Gemini 3.7 Flash, and `output_config.effort` through `rawPredict` with the body beta.
- Consider retrying `429`/`503` with `Retry-After` in the Vertex transport; `ProviderError::is_retryable` exists but the adapter does not use it.
