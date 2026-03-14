# ADR: Protocol-Neutral Core Request Boundary for Providers

- Date: 2026-03-13
- Status: Accepted

## Context

Issue #23 requires the gateway to stop coupling execution directly to OpenAI wire DTOs. The prior shape made provider adapters and handler flow implicitly OpenAI-centric, which increases long-term risk as model APIs evolve toward Responses-style and multimodal-first interfaces.

Before this change:

- `/v1/chat/completions` and `/v1/embeddings` wire DTOs were also the execution DTOs,
- `ProviderClient` accepted OpenAI protocol types directly,
- adapters were forced to depend on OpenAI request structs even when they were mapping to non-OpenAI upstream APIs.

## Decision

We introduced a protocol-neutral core request model and moved the provider trait boundary to core types while preserving the external OpenAI-compatible HTTP contract.

### 1. Add canonical core protocol request types

`gateway-core::protocol::core` now defines internal request types:

- `ChatRequest`
- `ChatMessage`
- `EmbeddingsRequest`

These are the canonical execution model used by provider adapters.

### 2. Keep OpenAI wire DTOs as the external API layer

`gateway-core::protocol::openai` remains the wire contract for HTTP handlers. This preserves `/v1/chat/completions` and `/v1/embeddings` compatibility while internal execution can evolve independently.

### 3. Add explicit translators between wire and core models

`gateway-core::protocol::translate` introduces explicit conversion functions:

- OpenAI -> core for request ingestion,
- core -> OpenAI for adapters that proxy OpenAI-compatible upstream APIs.

The translation layer is intentionally explicit to avoid hidden or ad hoc protocol coupling.

### 4. Change provider trait boundary to core requests

`ProviderClient` now accepts canonical core request types for chat, chat streaming, and embeddings. This makes adapter contracts protocol-neutral and reduces leakage of OpenAI-specific semantics into provider boundaries.

### 5. Convert at the handler edge

Gateway HTTP handlers now translate OpenAI wire requests into core requests before route resolution and provider execution. This establishes a clean boundary: external wire protocol at the edge, canonical model internally.

## Consequences

Positive:

- execution flow is no longer tied to OpenAI wire DTOs,
- providers can map from one canonical model to provider-native APIs,
- future protocol expansion can be done in `protocol::core` and translators instead of handler/provider rewrites.

Tradeoffs:

- translation code introduces an extra maintenance surface,
- short-term duplication exists between OpenAI wire structs and core structs.

## Follow-up Work

- Extend canonical response-side modeling as part of the remaining #23 work.
- Expand capability-aware request validation and route gating in #35 using canonical request shape.

## Attribution

This ADR was prepared through collaborative human + AI implementation/design work.
