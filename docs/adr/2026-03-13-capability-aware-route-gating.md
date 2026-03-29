# ADR: Capability-Aware Route Gating with Strict Fail-Fast Validation

- Date: 2026-03-13
- Status: Accepted

## Implemented By

- Canonical docs:
  - [../model-routing-and-api-behavior.md](../model-routing-and-api-behavior.md)

## Current state

- [../model-routing-and-api-behavior.md](../model-routing-and-api-behavior.md)
- [../request-lifecycle-and-failure-modes.md](../request-lifecycle-and-failure-modes.md)

## Context

Issue #35 requires richer capability modeling and deterministic request validation before provider execution. The previous model exposed only coarse booleans and allowed incompatible requests to fall through to provider-specific runtime errors.

This created three risks:

- route selection could choose targets that cannot satisfy request shape,
- invalid requests produced non-deterministic provider errors instead of gateway validation,
- capability metadata was too weak to safely evolve protocol features.

## Decision

We introduced richer per-route capability metadata and pre-execution capability filtering based on canonical request requirements.

### 1. Expand capability dimensions

`ProviderCapabilities` now models:

- `chat_completions`
- `stream`
- `embeddings`
- `tools`
- `vision`
- `json_schema`
- `developer_role`

### 2. Persist capability metadata on routes

`ModelRoute` and `SeedModelRoute` now include capabilities, and route records persist this data via `model_routes.capabilities_json` in both libsql and postgres.

### 3. Derive requirements from canonical request shape

Canonical core requests derive required capabilities before execution:

- streaming requests require `stream`,
- tool-bearing requests require `tools`,
- image-bearing content requires `vision`,
- `response_format.type=json_schema` requires `json_schema`,
- developer messages require `developer_role`,
- embeddings requests require `embeddings`.

### 4. Filter routes before execution and fail fast

Handlers now intersect provider capabilities with route capabilities and pre-filter incompatible routes. If no compatible route remains, the gateway returns deterministic `400 invalid_request` and does not attempt provider execution.

## Consequences

Positive:

- capability mismatch is validated consistently at the gateway edge,
- route selection is constrained to known-compatible targets,
- capability metadata becomes explicit and durable in configuration and storage.

Tradeoffs:

- route configuration now carries additional metadata that must be maintained,
- capability defaults must remain conservative and backward-compatible for existing seeded routes.

## Follow-up Work

- Add additional requirement derivation as new request features are supported.
- Extend constrained route-selection tests for embeddings when execution moves past deferred mode.

## Attribution

This ADR was prepared through collaborative human + AI implementation/design work.
