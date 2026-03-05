# ADR: Vertex-First Provider Foundation for Chat Completions

- Date: 2026-03-05
- Status: Accepted

## Context

The gateway exposed an OpenAI-compatible API surface (`/v1/chat/completions`, `/v1/embeddings`) but chat execution was not implemented end-to-end. We needed a production-ready first provider slice that:

- keeps external OpenAI-compat behavior,
- supports Google Vertex AI as the first fully supported provider type,
- works for both Gemini (`google/*`) and Claude-on-Vertex (`anthropic/*`),
- supports non-streaming and streaming output,
- maintains safe retry/fallback semantics,
- avoids schema churn for provider config storage already persisted as JSON blobs.

## Decision

### 1. Keep OpenAI-compatible external API; normalize provider differences internally

We kept `/v1/chat/completions` as the single external contract and added provider-specific mapping/normalization inside adapters.

Why:
- avoids client-facing API fragmentation,
- preserves compatibility with existing OpenAI SDK clients,
- allows adding providers without changing public API.

### 2. Add explicit provider capability contract

We introduced `ProviderCapabilities` and required `ProviderClient::capabilities()`.

Why:
- execution routing now depends on operation support (`chat`, `chat_stream`, `embeddings`),
- avoids attempting unsupported operations and makes deferred features explicit,
- enables gradual provider rollout by capability.

### 3. Extend provider request context instead of widening core request DTOs

We extended `ProviderRequestContext` with:
- route overrides: `extra_headers`, `extra_body`,
- request metadata: `idempotency_key`, `request_headers`.

Why:
- preserves stable OpenAI request DTOs,
- keeps provider-specific controls at route/context layer,
- supports per-route partner requirements (headers/body defaults/overrides).

### 4. Use typed provider config with tagged enum

`providers[*]` is now a tagged enum with:
- `openai_compat`
- `gcp_vertex`

`gcp_vertex` includes explicit fields and auth mode enum (`adc`, `service_account`, `bearer`) with parse-time validation.

Why:
- removes weakly-typed provider parsing logic,
- surfaces config errors early at startup,
- keeps DB migration unnecessary because provider config remains JSON in seed upserts.

### 5. Implement Vertex adapter as one provider serving multiple publishers

We parse `upstream_model` as `<publisher>/<model_id>` and route within one adapter to:
- `google/*` -> `generateContent` / `streamGenerateContent`
- `anthropic/*` -> `rawPredict` / `streamRawPredict`

Why:
- keeps provider registry simple (`gcp_vertex` provider type),
- supports mixed publisher families without duplicate provider implementations,
- aligns with Vertex publisher model addressing.

### 6. Introduce token source abstraction with in-memory near-expiry cache

Added internal `AccessTokenSource` implementations:
- `AdcTokenSource`
- `ServiceAccountTokenSource`
- `StaticBearerTokenSource`

Using scope: `https://www.googleapis.com/auth/cloud-platform`.

Why:
- cleanly separates auth acquisition from transport,
- supports multiple runtime auth modes with one adapter,
- avoids token-fetch on every request and reduces auth overhead.

### 7. Constrain fallback/retry policy to safe, deterministic cases

Handler fallback policy for chat is now:
- allowed only for non-stream requests with `Idempotency-Key`,
- otherwise single provider attempt,
- missing adapters are skipped safely.

Why:
- prevents duplicate side effects under ambiguous replay conditions,
- keeps stream behavior deterministic and avoids mid-stream fallback complexity,
- matches conservative idempotency requirements.

### 8. Normalize streaming output to OpenAI SSE chunks

We normalize heterogeneous upstream streams into OpenAI-style SSE chunks and always terminate with `data: [DONE]\n\n`.

Why:
- clients get one predictable stream format regardless of upstream provider,
- decouples client behavior from provider event model,
- keeps interoperability with existing OpenAI streaming clients.

### 9. Defer embeddings for Vertex in this slice

Embeddings execution remains deferred; capability checks now return `provider_not_implemented` where appropriate.

Why:
- keeps first slice focused and shippable,
- prevents partial/implicit behavior for unsupported operations,
- provides a clear extension point for the next slice.

## Consequences

Positive:
- `/v1/chat/completions` now executes end-to-end with provider routing.
- Vertex Gemini and Claude-on-Vertex are available through one gateway API.
- Config validation is stricter and failures surface earlier.
- Streaming behavior is standardized across providers.

Tradeoffs:
- Added adapter complexity (mapping + stream parsing/normalization).
- Budget enforcement, cost accounting, and full request/response payload logging are still follow-up concerns.
- Current implementation assumes UTF-8 textual stream framing for normalization paths.

## Follow-up Work

- Deterministic token usage + pricing ledger for spend accounting.
- Request/response payload logging design for PostgreSQL scale (BRIN, selective JSON indexing, retention/partitioning).
- Vertex embeddings support.

## Attribution

This ADR was prepared through collaborative human + AI implementation/design work.
