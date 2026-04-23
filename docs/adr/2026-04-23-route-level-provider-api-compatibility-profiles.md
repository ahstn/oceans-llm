# ADR: Route-Level Provider API Compatibility Profiles

## Status

Accepted.

## Context

The gateway exposes an OpenAI-shaped public surface, but providers that advertise OpenAI compatibility do not all implement the same request, response, and stream details.

This decision implements the first runtime-backed slice of [issue #53](https://github.com/ahstn/oceans-llm/issues/53).

Known differences include:

- `store` support
- `max_completion_tokens` versus `max_tokens`
- `developer` versus `system` role support
- `reasoning_effort` passthrough, omission, or remapping
- stream usage placement and whether usage must be requested
- provider-specific reasoning delta field names

Encoding these differences as `extra_body` conventions would hide behavior inside additive overrides and make route behavior hard to reason about. Preserving legacy fallback behavior would also make future adapters copy implicit quirks instead of declaring compatibility explicitly.

## Decision

Provider API compatibility quirks live in typed route-level metadata.

Implementation points:

- `ModelRoute` and `SeedModelRoute` carry `RouteCompatibility`.
- `ProviderRequestContext` carries the selected route's compatibility profile into the provider adapter.
- `model_routes.compatibility_json` persists the typed profile as JSON.
- `gateway.yaml` accepts route metadata under `compatibility.openai_compat`.
- `crates/gateway-providers/src/openai_compat.rs` applies explicit, tested outbound transforms.
- OpenAI-compatible stream normalization covers basic usage and reasoning field variants.

Unsupported API-family work is tracked as separate implementation issues instead of being represented as dormant config flags.

## Consequences

Benefits:

- compatibility behavior is visible in config, storage, provider context, and tests
- different routes through the same provider can have different profiles
- `extra_body` remains an additive provider override mechanism, not a hidden transform contract
- new API families can add their own typed metadata without overloading OpenAI-compatible behavior

Trade-offs:

- adding new quirks requires domain, config, storage, and provider test updates
- existing routes default to a conservative OpenAI-compatible profile, so operators must opt into known quirks
- compatibility metadata is intentionally route-scoped, which is more verbose when many routes share the same profile

## Follow-Up

- Continue expanding OpenAI Responses API coverage on its first-class request, response, and stream boundary.
- Add native Anthropic Messages API-family mapping.
- Add direct Google Generative AI provider support beyond Vertex.
- Add cross-provider tool-call streaming fixtures and normalization.
- Expand token/cache accounting beyond `prompt_tokens`, `completion_tokens`, and `total_tokens`.
- Define multimodal image/file compatibility across OpenAI-compatible, Vertex Google, Anthropic, and Google native APIs.
