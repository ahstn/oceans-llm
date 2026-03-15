# ADR: OpenAI-Compatible Streaming and Embeddings Runtime Parity

- Date: 2026-03-15
- Status: Accepted

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
- execute against the selected provider route,
- preserve deterministic fallback policy (retry/fallback only with `Idempotency-Key`),
- record usage and request logs through existing service paths.

### 2. Implement SSE streaming for OpenAI-compatible providers

`OpenAiCompatProvider::chat_completions_stream` is now implemented and no longer deferred. The adapter:

- forces stream request shape for upstream calls,
- maps upstream non-success HTTP responses to `ProviderError::UpstreamHttp`,
- proxies valid SSE data events,
- appends `[DONE]` if upstream closes without emitting it,
- emits OpenAI-style stream error chunks for mid-stream transport/parse failures.

### 3. Promote OpenAI-compatible stream capability to runtime truth

`ProviderCapabilities::openai_compat_baseline()` now advertises stream support, aligning capability metadata with actual runtime behavior.

### 4. Keep Vertex embeddings deferred in this slice

Vertex embeddings remains explicitly deferred (`provider_not_implemented`) to keep scope focused on closing #21 and #22 with deterministic behavior, without introducing provider-specific embeddings mapping in this delivery.

### 5. Add operation-aware request-log metadata

Request log metadata now includes an `operation` key (`chat_completions` or `embeddings`) while preserving existing `stream`, `fallback_used`, and `attempt_count` fields.

## Consequences

Positive:

- `/v1/embeddings` now executes for supported routes instead of blanket deferral.
- OpenAI-compatible streaming is available across the gateway contract, not only Vertex.
- Capability-aware routing now better reflects runtime reality for stream + embeddings.
- Observability can distinguish chat vs embeddings without schema changes.

Tradeoffs:

- OpenAI-compatible stream normalization adds parser/adapter complexity.
- Embeddings usage accounting remains on the existing token model; provider-specific embeddings usage nuance may require future refinements.
- Vertex embeddings still requires dedicated follow-up implementation work.

## Follow-up Work

- Implement Vertex embeddings mapping and normalization when provider contract details are finalized.
- Reuse shared stream parsing utilities in future provider adapters to avoid parser drift.
- Update issue checklists for #21 and #22 when the merge that ships this ADR lands.

## Attribution

This ADR was prepared through collaborative human + AI implementation/design work.
