# Request Lifecycle and Failure Modes

`See also`: [Model Routing and API Behavior](../configuration/model-routing-and-api-behavior.md), [Provider API Compatibility](provider-api-compatibility.md), [Pricing Catalog and Accounting](../configuration/pricing-catalog-and-accounting.md), [Budgets and Spending](../operations/budgets-and-spending.md), [Observability and Request Logs](../operations/observability-and-request-logs.md), [Configuration Reference](../configuration/configuration-reference.md), [Identity and Access](../access/identity-and-access.md), [Data Relationships](data-relationships.md), [ADR: V1 Runtime Simplification for Routing and Streaming](../adr/2026-03-15-v1-runtime-simplification.md), [ADR: Route-Level Provider API Compatibility Profiles](../adr/2026-04-23-route-level-provider-api-compatibility-profiles.md)

This page is the cross-cutting view. Neighboring docs own their own policy slices. This page explains how those slices connect during one request.

## Source of Truth

- HTTP handlers: [../crates/gateway/src/http/handlers.rs](../../crates/gateway/src/http/handlers.rs)
- Model access and tag selection: [../crates/gateway-service/src/model_access.rs](../../crates/gateway-service/src/model_access.rs)
- Alias resolution: [../crates/gateway-service/src/model_resolution.rs](../../crates/gateway-service/src/model_resolution.rs)
- Route planning: [../crates/gateway-service/src/route_planner.rs](../../crates/gateway-service/src/route_planner.rs)
- Budget enforcement: [../crates/gateway-service/src/budget_guard.rs](../../crates/gateway-service/src/budget_guard.rs)
- Pricing resolution: [../crates/gateway-service/src/pricing_catalog.rs](../../crates/gateway-service/src/pricing_catalog.rs)
- Request logging: [../crates/gateway-service/src/request_logging.rs](../../crates/gateway-service/src/request_logging.rs)
- Ledger writes: [../crates/gateway-service/src/service.rs](../../crates/gateway-service/src/service.rs)

## Request Path

The live request path is single-route in this slice.

1. The gateway authenticates the API key.
2. The allowed gateway model set is reduced by API-key grants and any user or team allowlists.
3. The requested model is resolved.
   - A concrete model key stays concrete.
   - A `tag:` selector picks one allowed gateway model.
   - An alias resolves to a canonical execution model.
4. The route planner builds an ordered route list.
   - Lower `priority` wins first.
   - `weight` only matters inside the same priority bucket.
   - Disabled routes and non-positive weights drop out.
5. Capability filtering removes routes that cannot satisfy the API family and feature requirements. For example, `/v1/responses` requires `responses`, while `/v1/chat/completions` requires `chat_completions`.
6. The budget guard runs before provider execution.
   - hard-limit rejection returns `429 budget_exceeded`
   - no provider call occurs on this path
7. Route compatibility metadata is passed into the provider adapter.
8. The provider adapter applies declared compatibility transforms only for API families with defined transforms.
9. The first eligible route executes.
10. Request logs are written for the user-visible outcome.
11. Usage is normalized when possible.
12. Pricing is resolved exactly or the request is marked `unpriced`.
13. A ledger row is written when the request has usable usage data.
14. Post-provider budget math runs before the priced ledger row is committed.

Compatibility transforms can affect the provider request body and stream options for the selected API family. They do not change the public request model identity, alias resolution, API-key grants, or request-log attribution. Current OpenAI-compatible profile transforms are Chat Completions-specific; Responses and Embeddings use their own typed provider paths.

## Worked Example

One common request path looks like this:

- Request:
  - `POST /v1/responses`
  - API key belongs to team `growth`
  - model is `tag:fast`
- Access:
  - the API key grant allows `gpt-4o-mini` and `claude-3-5-haiku`
  - the team allowlist is unrestricted
- Resolution:
  - `tag:fast` resolves to gateway model `gpt-4o-mini`
  - `gpt-4o-mini` is an alias of `openai-gpt-4o-mini`
- Planning:
  - `openai-gpt-4o-mini` has two routes in config
  - route A has priority `50`
  - route B has priority `100`
  - route A wins before weight is considered
- Capability filter:
  - the request asks for the Responses API family, no tools, no vision
  - route A stays eligible
- Execution:
  - the provider request goes to the route A provider and upstream model through the Responses adapter
- Logging:
  - `request_logs.model_key` stores `gpt-4o-mini`
  - `request_logs.resolved_model_key` stores `openai-gpt-4o-mini`
  - `request_logs.provider_key` stores the route A provider id
- Accounting:
  - usage is normalized
  - pricing resolves exactly
  - `usage_cost_events.pricing_status` becomes `priced`
  - the team budget window includes the charge

## Model Visibility Versus Execution

A model can be visible and still fail at runtime.

- `/v1/models` shows grant-visible gateway identities.
- `/v1/models` does not promise that a route is executable right now.
- Route viability still depends on:
  - provider existence
  - route `enabled`
  - positive weight
  - capability match
  - pricing readiness, if spend accuracy matters for the request path

This is why a model can appear in `/v1/models` and still fail with `invalid_request` or `no_routes_available`.

## Failure Classes

These failures look similar from far away, but they mean different things.

### `invalid_request`

- The model resolved.
- Capability filtering removed every remaining route.
- Common causes:
  - embeddings against a chat-only route
  - Responses requests against a route with `responses: false`
  - tools against a route with tools disabled
  - vision against a route that does not advertise vision

### `no_routes_available`

- The model exists.
- No usable route survived provider and route-viability checks.
- Common causes:
  - missing provider id in the live config
  - all routes disabled
  - all routes have non-positive weight

### `budget_exceeded`

- A pre-provider hard-limit check blocked the request.
- The HTTP response is `429`.
- No provider call occurs on this path.
- Observability records this as a request outcome rather than as provider execution.

### `unpriced`

- The provider request succeeded.
- Usage exists.
- Exact pricing could not be resolved.
- Common causes:
  - unsupported `pricing_provider_id`
  - unsupported Vertex publisher or location
  - unsupported billing modifiers
  - missing exact rate coverage

`unpriced` requests stay visible in reports but do not count toward budget totals.

### `usage_missing`

- The provider request succeeded.
- Usage could not be normalized into the gateway accounting model.
- The request log still records the user-visible outcome.
- The ledger row stays visible, but it does not count toward spend totals.

## Logging and Ledger Boundaries

Request logs and spend rows are related, but they are not the same object.

- `request_logs` owns the user-visible request outcome.
- `request_log_payloads` owns sanitized request and response bodies.
- `usage_cost_events` owns spend enforcement and spend reporting.

That separation matters in two common cases:

- a request can be logged even when it becomes `unpriced`
- a request can be logged even when a later accounting step hits a rough edge

For streaming requests, the request-log payload path parses SSE incrementally across UTF-8 and frame boundaries and retains the latest coherent usage snapshot seen before stream completion or failure. Chat Completions streams usually expose usage at top level. Responses streams expose usage on completed response events as `response.usage`. Stored stream events can be capped by payload policy without weakening usage or provider-error parsing.

## Known Rough Edges

- Request-log payload policy details, redaction rules, and retention status are owned by [observability-and-request-logs.md](../operations/observability-and-request-logs.md).
- Configurable retry and fallback execution is not part of the current request path; see [issue #118](https://github.com/ahstn/oceans-llm/issues/118).

For the current observability cleanup notes, see [observability-and-request-logs.md](../operations/observability-and-request-logs.md).

## What This Page Does Not Own

- config field syntax and validation: [configuration-reference.md](../configuration/configuration-reference.md)
- identity and ownership policy: [identity-and-access.md](../access/identity-and-access.md)
- route-planning contract and endpoint behavior: [model-routing-and-api-behavior.md](../configuration/model-routing-and-api-behavior.md)
- exact pricing coverage rules: [pricing-catalog-and-accounting.md](../configuration/pricing-catalog-and-accounting.md)
- budget windows and spend APIs: [budgets-and-spending.md](../operations/budgets-and-spending.md)
- request-log storage and payload policy: [observability-and-request-logs.md](../operations/observability-and-request-logs.md)
