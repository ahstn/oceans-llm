# Model Routing and API Behavior

`Owns`: model identity, aliases, `tag:` selectors, route-planning inputs, capability gating, and `/v1/*` behavior.
`Depends on`: [configuration-reference.md](configuration-reference.md), [data-relationships.md](../reference/data-relationships.md), [identity-and-access.md](../access/identity-and-access.md)
`See also`: [request-lifecycle-and-failure-modes.md](../reference/request-lifecycle-and-failure-modes.md), [pricing-catalog-and-accounting.md](pricing-catalog-and-accounting.md), [observability-and-request-logs.md](../operations/observability-and-request-logs.md), [adr/2026-03-10-model-aliases-and-provider-route-config.md](../adr/2026-03-10-model-aliases-and-provider-route-config.md), [adr/2026-03-13-capability-aware-route-gating.md](../adr/2026-03-13-capability-aware-route-gating.md)

This page explains how the public `/v1/*` surface resolves a request into one concrete route.

## Source of Truth

- config parsing:
  - [../crates/gateway/src/config.rs](../../crates/gateway/src/config.rs)
- model access and tag selection:
  - [../crates/gateway-service/src/model_access.rs](../../crates/gateway-service/src/model_access.rs)
- alias resolution:
  - [../crates/gateway-service/src/model_resolution.rs](../../crates/gateway-service/src/model_resolution.rs)
- route planning:
  - [../crates/gateway-service/src/route_planner.rs](../../crates/gateway-service/src/route_planner.rs)
- HTTP handlers:
  - [../crates/gateway/src/http/handlers.rs](../../crates/gateway/src/http/handlers.rs)

## Public Endpoints

The live public endpoints are:

- `GET /v1/models`
- `POST /v1/chat/completions`
- `POST /v1/embeddings`

All are authenticated.

## Requested Versus Resolved Model Identity

The gateway keeps two model identities in play:

- requested model
  - what the caller asked for
- resolved model
  - the canonical execution target after alias resolution

That distinction is persisted into request logs.

## Model Forms

Configured gateway models are either:

- provider-backed
- alias-backed

A model cannot define both routes and `alias_of`.

## `tag:` Selectors

The request `model` field can be:

- a concrete gateway model key
- a tag selector such as `tag:fast`

Tag selectors use AND semantics.

- every requested tag must exist on the chosen model
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
- `weight` only matters within the same priority bucket
- disabled routes and routes with non-positive weight are excluded

Current runtime nuance:

- weighted routing is not multi-route fallback
- the planner produces an ordered route list
- the handler executes only the first eligible route

## Capability-Aware Gating

Routes are filtered before provider execution based on request requirements and route capability.

Current capability dimensions:

- `chat_completions`
- `stream`
- `embeddings`
- `tools`
- `vision`
- `json_schema`
- `developer_role`

Capability metadata exists to fail early at the gateway edge. It is not a copy of provider marketing language.

## Worked Request Path

One plain path looks like this:

- request model:
  - `tag:fast`
- allowed model set:
  - `gpt-4o-mini`, `claude-3-5-haiku`
- selection result:
  - `gpt-4o-mini`
- alias result:
  - `openai-gpt-4o-mini`
- planned route order:
  - `openai-primary`, then `openai-backup`
- capability filter:
  - `openai-primary` stays eligible
- execution:
  - the handler uses `openai-primary`
- request-log fields:
  - `model_key = gpt-4o-mini`
  - `resolved_model_key = openai-gpt-4o-mini`
  - `provider_key = openai-primary`

Use [request-lifecycle-and-failure-modes.md](../reference/request-lifecycle-and-failure-modes.md) for the later logging, pricing, and budget effects.

## `/v1/models`

`GET /v1/models` returns the gateway models visible to the authenticated API key.

Important notes:

- it reflects gateway model identity, not raw provider catalogs
- it shows grant-visible identities
- it does not promise executable routes

That last point matters. A model can be visible and still fail if route viability or capability checks remove every route.

## `/v1/chat/completions`

Current behavior highlights:

- request IDs are propagated through `x-request-id`
- budget checks run before provider execution
- successful requests write usage when usage can be normalized
- request logs store both requested and resolved model identity

## `/v1/embeddings`

`POST /v1/embeddings` follows the same high-level path:

- authenticate
- resolve the requested model
- capability-filter the route set
- execute the first eligible route
- write usage when usage can be normalized

Current limitation:

- Vertex embeddings remain out of scope in this slice and should be excluded by capability gating

## Route Viability Versus Capability Mismatch

| Symptom | Meaning |
| --- | --- |
| `invalid_request` | the model resolved, but capability filtering removed every route |
| `no_routes_available` | the model exists, but no usable route survived provider and route-viability checks |

That distinction is one of the fastest ways to debug a visible-but-unusable model.

## Current V1 Behavior

The live runtime is intentionally narrow in this slice:

- single-route execution only
- no retry loop
- no live fallback loop
- strict capability filtering before provider execution

## What This Page Does Not Own

- config field syntax and defaults:
  - [configuration-reference.md](configuration-reference.md)
- full cross-cutting request path:
  - [request-lifecycle-and-failure-modes.md](../reference/request-lifecycle-and-failure-modes.md)
- exact pricing coverage:
  - [pricing-catalog-and-accounting.md](pricing-catalog-and-accounting.md)
- spend enforcement and budget windows:
  - [budgets-and-spending.md](../operations/budgets-and-spending.md)
