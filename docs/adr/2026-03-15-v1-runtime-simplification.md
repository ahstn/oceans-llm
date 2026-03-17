# ADR: V1 Runtime Simplification for Routing and Streaming

- Date: 2026-03-15
- Status: Accepted

## Implemented By

- Canonical docs:
  - [../model-routing-and-api-behavior.md](../model-routing-and-api-behavior.md)
  - [../observability-and-request-logs.md](../observability-and-request-logs.md)

## Context

After closing the initial runtime gaps for embeddings and OpenAI-compatible streaming, the request path still contained compatibility-era complexity:

- idempotency-gated retry/fallback loops in handlers,
- runtime `provider_not_implemented` style behavior for unsupported operations,
- lenient stream handling that accepted non-SSE success responses,
- observability metadata fields tied to removed fallback behavior.

For v1, this complexity added maintenance cost without product value and encouraged drift between configured capabilities and runtime truth.

## Decision

### 1. Single-route execution in v1

Chat and embeddings requests now execute against the first eligible route only.

- Removed retry/fallback loops.
- Removed idempotency-gated routing behavior.
- Removed idempotency key plumbing from provider request context.

### 2. Capability-gated unsupported behavior

Unsupported chat/embeddings behavior is now determined at capability filtering time.

- If no compatible route exists, return deterministic `400 invalid_request`.
- Avoid runtime reliance on `provider_not_implemented` for these operations.
- Keep Vertex embeddings out of scope in this slice via capability gating.

### 3. Strict OpenAI-compatible SSE contract

OpenAI-compatible streaming now enforces protocol correctness.

- Require `text/event-stream` content type.
- Require valid SSE framing and valid stream finalization.
- Emit one OpenAI-style SSE error chunk on parse/transport/protocol failure and terminate without `[DONE]`.
- Append `[DONE]` only on clean completion when upstream omits it.

### 4. Observability cleanup

Request-log metadata now keeps only fields that reflect active behavior.

- Retained: `operation`, `stream`.
- Removed: fallback-era `fallback_used`, `attempt_count`.

## How it was implemented

- Handler runtime flow was rewritten into a shared single-route execution shape for chat and embeddings.
- Provider context was simplified by removing idempotency-key plumbing.
- OpenAI-compatible stream adapter gained strict content-type and finalization checks.
- Shared SSE parsing utilities gained explicit finalization APIs for incomplete UTF-8 and incomplete event detection.
- Gateway and provider tests were updated to validate single-route behavior and strict SSE behavior.

## Consequences

Positive:

- Runtime behavior now matches configured capability intent.
- Request execution is smaller and easier to reason about.
- Streaming failures are deterministic and surfaced consistently.
- Logging metadata reflects current runtime behavior rather than legacy fallback semantics.

Tradeoffs:

- Non-SSE or malformed upstream stream responses now fail fast where previously they might have been tolerated.
- v1 intentionally drops fallback behavior, which may reduce resilience in some transient outage scenarios.

## Follow-up

- Add a configurable fallback strategy only when there is a clear product requirement and ownership for idempotency semantics.
- Implement Vertex embeddings in a dedicated slice with explicit capability and contract tests.
