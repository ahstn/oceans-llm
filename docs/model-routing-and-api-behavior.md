# Model Routing and API Behavior

`Owns`: model identity, aliases, `tag:` selectors, route planning inputs, capability gating, and `/v1/*` behavior.
`Depends on`: [data-relationships.md](data-relationships.md), [identity-and-access.md](identity-and-access.md)
`See also`: [budgets-and-spending.md](budgets-and-spending.md), [observability-and-request-logs.md](observability-and-request-logs.md), [adr/2026-03-10-model-aliases-and-provider-route-config.md](adr/2026-03-10-model-aliases-and-provider-route-config.md), [adr/2026-03-13-capability-aware-route-gating.md](adr/2026-03-13-capability-aware-route-gating.md), [adr/2026-03-15-v1-runtime-simplification.md](adr/2026-03-15-v1-runtime-simplification.md)

This document describes how requests move from the public `/v1/*` surface to a concrete provider route.

## Source of Truth

- Config parsing: [../crates/gateway/src/config.rs](../crates/gateway/src/config.rs)
- Model access and tag selection: [../crates/gateway-service/src/model_access.rs](../crates/gateway-service/src/model_access.rs)
- Alias resolution: [../crates/gateway-service/src/model_resolution.rs](../crates/gateway-service/src/model_resolution.rs)
- Route planning: [../crates/gateway-service/src/route_planner.rs](../crates/gateway-service/src/route_planner.rs)
- HTTP handlers: [../crates/gateway/src/http/handlers.rs](../crates/gateway/src/http/handlers.rs)

## Public Endpoints

The gateway exposes:

- `GET /v1/models`
- `POST /v1/chat/completions`
- `POST /v1/embeddings`

All three endpoints are authenticated.

## Requested vs Resolved Model Identity

The gateway distinguishes between:

- the requested model: what the client asked for
- the resolved model: the canonical execution target after alias resolution

That distinction is persisted into request logs so operators can see both the client-facing contract and the execution identity.

## Model Forms

Configured gateway models are either:

- provider-backed
- alias-backed

A model cannot define both routes and `alias_of`.

## `tag:` Model Selectors

The request `model` field can be a concrete gateway model key or a tag selector:

- `tag:fast`
- `tag:fast,cheap`

Tag selectors use AND semantics:

- every requested tag must be present on the chosen model
- selection only considers models already allowed for the authenticated API key
- candidates are ordered by model `rank`, then model key

## Routes, Priority, and Weight

Provider-backed models resolve to one or more routes.

Each route can define:

- `provider`
- `upstream_model`
- `priority`
- `weight`
- `enabled`
- `capabilities`

Current planner behavior:

- lower `priority` is attempted first
- weights apply only within the same priority bucket
- disabled routes or routes with non-positive weight are excluded

## Capability-Aware Gating

Routes are filtered before provider execution based on request requirements and effective route capability.

Current capability dimensions:

- `chat_completions`
- `stream`
- `embeddings`
- `tools`
- `vision`
- `json_schema`
- `developer_role`

If no compatible route remains, the gateway returns a deterministic `400 invalid_request` instead of relying on provider-specific behavior.

## Current V1 Runtime Behavior

The runtime is intentionally simplified in this slice:

- single-route execution only
- no retry or fallback loop in the live request path
- no idempotency-gated route retries
- strict capability filtering before provider execution

This means the request flow is:

1. authenticate
2. resolve allowed requested model
3. canonicalize aliases
4. plan routes
5. filter by capability
6. execute the first eligible route
7. record usage and logs

## `/v1/models`

`GET /v1/models` returns the models visible to the authenticated API key after grants and access overlays are applied.

Important notes:

- it reflects gateway model identity, not raw upstream provider catalogs
- it does not expose `tag:` selectors directly; tags remain a request-time convenience

## `/v1/chat/completions`

Current behavior highlights:

- request IDs are propagated through `x-request-id`
- budget checks run before provider execution
- successful requests write usage into the spend ledger when usage can be normalized
- request logs store both requested and resolved model identity

Known current rough edge:

- stream and non-stream chat paths still differ when a post-provider ledger write fails

That follow-up is tracked in [issue #49](https://github.com/ahstn/oceans-llm/issues/49).

## `/v1/embeddings`

`POST /v1/embeddings` is live in the runtime and follows the same high-level execution model:

- authenticate
- resolve requested model and alias
- capability-filter the route set
- execute the first eligible route
- write usage when usage can be normalized

Current limitation:

- Vertex embeddings remain out of scope in this slice and are expected to be excluded by capability gating

## Config Notes That Matter Operationally

- `openai_compat` providers must declare a supported `pricing_provider_id`
- `gcp_vertex` routes require `upstream_model` in `<publisher>/<model_id>` form
- route capabilities default permissively unless explicitly constrained in config

For the operator-facing admin view built on top of these rules, see [admin-control-plane.md](admin-control-plane.md).
