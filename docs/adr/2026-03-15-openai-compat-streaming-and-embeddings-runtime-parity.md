# ADR: OpenAI-Compatible Streaming and Embeddings Runtime Parity

- Date: 2026-03-15
- Status: Accepted

## Current state

- [../model-routing-and-api-behavior.md](../model-routing-and-api-behavior.md)
- [../request-lifecycle-and-failure-modes.md](../request-lifecycle-and-failure-modes.md)
- [../observability-and-request-logs.md](../observability-and-request-logs.md)

## Context

Two high-priority runtime gaps remained open:

- OpenAI-compatible providers were integrated for non-stream chat but still returned a deferred `NotImplemented` path for streaming.
- `/v1/embeddings` was exposed in the gateway surface but intentionally deferred in the handler after routing.

This left capability metadata ahead of runtime behavior and reduced confidence in provider-route selection and observability paths.

## Decision

### 1. Execute `/v1/embeddings` in the live request path

`POST /v1/embeddings` now follows the same execution shape as non-stream chat:

- authenticate and resolve model/routes,
- filter by capability-aware requirements,
- execute against the first eligible provider route (single-route execution),
- record usage and request logs through existing service paths.

### 2. Implement SSE streaming for OpenAI-compatible providers

`OpenAiCompatProvider::chat_completions_stream` is now implemented and no longer deferred. The adapter:

- forces stream request shape for upstream calls,
- maps upstream non-success HTTP responses to `ProviderError::UpstreamHttp`,
- requires upstream `Content-Type: text/event-stream`,
- proxies valid SSE data events,
- appends `[DONE]` if upstream closes without emitting it,
- emits OpenAI-style stream error chunks for mid-stream transport/parse failures,
- emits deterministic stream errors for malformed/empty/incomplete SSE finalization.

### 3. Promote OpenAI-compatible stream capability to runtime truth

`ProviderCapabilities::openai_compat_baseline()` now advertises stream support, aligning capability metadata with actual runtime behavior.

### 4. Use capability-gated unsupported behavior for v1

Unsupported runtime behavior for chat/embeddings is now surfaced through capability filtering (`400 invalid_request` for no compatible route) instead of runtime `provider_not_implemented` branches.

For Vertex embeddings in this slice, routes are expected to advertise `embeddings=false`, so unsupported behavior is deterministic at route selection time.

### 5. Add operation-aware request-log metadata

Request log metadata now includes `operation` (`chat_completions` or `embeddings`) and `stream`. Fallback-era metadata fields were removed with single-route execution.

## Consequences

Positive:

- `/v1/embeddings` now executes for supported routes instead of blanket deferral.
- OpenAI-compatible streaming is available across the gateway contract, not only Vertex.
- Capability-aware routing now deterministically controls unsupported behavior for stream + embeddings.
- Observability can distinguish chat vs embeddings without schema changes.
- Runtime execution is simpler (no idempotency-gated fallback path).

Tradeoffs:

- OpenAI-compatible stream normalization adds stricter parser/adapter checks.
- Embeddings usage accounting remains on the existing token model; provider-specific embeddings usage nuance may require future refinements.
- Vertex embeddings still requires dedicated follow-up implementation work.

## Follow-up Work

- Implement Vertex embeddings mapping and normalization when provider contract details are finalized.
- Reuse shared stream parsing utilities in future provider adapters to avoid parser drift.
- Keep provider capability metadata aligned with runtime support to avoid reintroducing runtime `NotImplemented` branches for chat/embeddings.

## Attribution

This ADR was prepared through collaborative human + AI implementation/design work.
