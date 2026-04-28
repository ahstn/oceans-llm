# Model Routing and API Behavior

`See also`: [Configuration Reference](configuration-reference.md), [Provider API Compatibility](../reference/provider-api-compatibility.md), [Data Relationships](../reference/data-relationships.md), [Identity and Access](../access/identity-and-access.md), [Request Lifecycle and Failure Modes](../reference/request-lifecycle-and-failure-modes.md), [Pricing Catalog and Accounting](pricing-catalog-and-accounting.md), [Observability and Request Logs](../operations/observability-and-request-logs.md), [ADR: Model Aliases and Provider-Only Route Config](../adr/2026-03-10-model-aliases-and-provider-route-config.md), [ADR: Capability-Aware Route Gating with Strict Fail-Fast Validation](../adr/2026-03-13-capability-aware-route-gating.md), [ADR: Route-Level Provider API Compatibility Profiles](../adr/2026-04-23-route-level-provider-api-compatibility-profiles.md)

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
- `POST /v1/responses`
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

Weight affects selection inside a single priority bucket. It does not mean the gateway sends one request to several providers, retries the next route, or falls back after an upstream error. Configurable retry and fallback remains separate follow-up work in [issue #118](https://github.com/ahstn/oceans-llm/issues/118).

## Capability-Aware Gating

Routes are filtered before provider execution based on request requirements and route capability.

Current capability dimensions:

- `chat_completions`
- `responses`
- `stream`
- `embeddings`
- `tools`
- `vision`
- `json_schema`
- `developer_role`

Capability metadata exists to fail early at the gateway edge. It is not a copy of provider marketing language.

Effective capability is the intersection of route metadata and provider runtime support.

- route capability defaults are permissive
- provider implementations can still reject unsupported API families
- partial provider routes should explicitly disable unsupported API families

For example, current Vertex routes support the chat path but not the Responses path. A Vertex chat route should keep `responses: false` so `/v1/responses` fails during capability filtering instead of later inside the provider adapter.

## Compatibility Profiles

Routes can also define provider API compatibility metadata.

Capabilities and compatibility have different jobs:

- `capabilities` gates whether the route can execute a request at all
- `compatibility` rewrites the outbound provider request shape after a route is selected

OpenAI-compatible route profiles currently cover deterministic Chat Completions transforms such as `store` removal, token field renaming, `developer` role rewriting, `reasoning_effort` handling, and stream usage requests. Responses uses a separate typed request/provider path; Chat Completions transforms must not be used as Responses shims.

See [provider-api-compatibility.md](../reference/provider-api-compatibility.md) for the compatibility matrix and field-level contract.

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

- request IDs are assigned once at the HTTP middleware boundary and propagated through `x-request-id`
- budget checks run before provider execution
- successful requests write usage when usage can be normalized
- request logs store both requested and resolved model identity

## `/v1/responses`

`POST /v1/responses` follows the same authentication, model resolution, route planning, budget guard, logging, and ledger flow as Chat Completions.

Important differences:

- route capability filtering requires `responses`
- provider execution calls the provider's Responses methods, not Chat Completions methods
- streaming preserves Responses `response.*` event names and payloads instead of rewriting them into Chat Completions chunks
- usage is normalized from `input_tokens`, `output_tokens`, and `total_tokens`

## `/v1/embeddings`

`POST /v1/embeddings` follows the same high-level path:

- authenticate
- resolve the requested model
- capability-filter the route set
- execute the first eligible route
- record the provider execution attempt when request logging writes a summary row
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
- request-attempt records describe the single provider execution attempt; configurable retry/fallback execution is tracked separately in issue #118
- strict capability filtering before provider execution

The open retry/fallback policy work must amend this section when it lands; see [issue #118](https://github.com/ahstn/oceans-llm/issues/118).

## What This Page Does Not Own

- config field syntax and defaults:
  - [configuration-reference.md](configuration-reference.md)
- full cross-cutting request path:
  - [request-lifecycle-and-failure-modes.md](../reference/request-lifecycle-and-failure-modes.md)
- exact pricing coverage:
  - [pricing-catalog-and-accounting.md](pricing-catalog-and-accounting.md)
- spend enforcement and budget windows:
  - [budgets-and-spending.md](../operations/budgets-and-spending.md)
