# OpenAI Responses API Family Boundary

## Status

Accepted

## Context

The gateway already exposed OpenAI-shaped Chat Completions and Embeddings. The OpenAI Responses API has different request input shapes, output items, usage fields, reasoning items, tool events, and stream event names. Treating Responses as Chat Completions with transformed fields would lose semantics and create a compatibility shim that would be hard to unwind.

## Decision

`POST /v1/responses` is a first-class public API family.

Implementation rules:

- `gateway-core` owns a distinct Responses wire DTO and core request type.
- provider execution uses `ProviderClient::responses` and `ProviderClient::responses_stream`.
- route gating requires the `responses` capability.
- the OpenAI-compatible provider posts to upstream `/v1/responses`.
- Responses streams preserve `response.*` event names and payloads instead of normalizing into Chat Completions chunks.
- usage is normalized from `input_tokens`, `output_tokens`, and `total_tokens`.

## Consequences

The gateway now has a durable place to add Responses-specific behavior without overloading Chat Completions compatibility profiles. The first slice still returns upstream response objects as JSON and preserves stream event payloads; richer semantic normalization for hosted tools, multimodal content, and cache/reasoning counters remains follow-up work.
