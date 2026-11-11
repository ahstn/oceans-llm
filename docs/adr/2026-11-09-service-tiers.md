# ADR: Service Tiers and Fast Models

- Date: 2026-11-09
- Status: Accepted
- Amends: [Multi-Source Model Catalog](2026-10-31-multi-source-model-catalog.md), [Hybrid Pricing Catalog from models.dev](2026-03-06-hybrid-pricing-catalog.md)

## Context

Providers sell the same model at different capacity grades:

| Provider | Request field | Tiers | Reported in |
| --- | --- | --- | --- |
| OpenAI, OpenRouter | body `service_tier` | `default`, `flex`, `priority` (Fast), `ultrafast` (Responses only, some models) | response `service_tier` |
| Anthropic | body `speed: "fast"`; `service_tier: "standard_only"` | standard, fast | `usage.speed`, `usage.service_tier` |
| Vertex Gemini | header `X-Vertex-AI-LLM-Shared-Request-Type` | `priority`, `flex` | `usageMetadata.trafficType` |
| Bedrock Converse | body `serviceTier: {type}` | `default`, `flex`, `priority` | `serviceTier` in the response |
| Bedrock InvokeModel | header `X-Amzn-Bedrock-Service-Tier` | `default`, `flex`, `priority` | response header |

Priority costs about 2× the standard rate on OpenAI GPT-6, Anthropic Opus fast mode, and Gemini (about 1.8×). Ultrafast costs about 6×. Flex costs about 0.5×. OpenAI can serve a priority request at the default tier and report `default`.

Before this change the gateway did not model tiers:

- A caller's `service_tier` went through `extra` to the provider unchanged. Catalog pricing then billed the standard rate. Only a tier in route `extra_body` blocked catalog pricing.
- Usage accounting kept only the `usage` object. OpenAI's top-level `service_tier` was lost.
- Vertex Gemini dropped the field. Bedrock rejected it as an unknown field. The Anthropic adapter forwarded OpenAI values that Anthropic does not accept.
- Discovery did not say which tiers a model serves, so clients could not show a Fast variant.

## Decision

### 1. One tier type

`gateway_core::ServiceTier` has four values: `default`, `flex`, `priority`, and `ultrafast`. `fast` is an alias of `priority`, and `standard` is an alias of `default`.

The handlers for Chat Completions, Responses, and Messages remove every tier field from the request before routing:

- `service_tier`: `auto` and `null` ask for nothing. `standard_only` asks for default.
- `serviceTier`: a string, or Bedrock's `{"type": ...}`.
- `speed`: `fast` asks for priority, and `standard` asks for default.

An unknown value, a value of the wrong type, or two fields that ask for different tiers return 400. `auto` never becomes priority.

### 2. Effective tier and the adapter contract

The effective tier is the route's `service_tier` when the route sets one, else the requested tier. The route wins because an admin publishes a route such as `gpt-6-luna-fast` to fix its tier and price.

The tier reaches the adapter only in `ProviderRequestContext.service_tier`. Each adapter writes its own wire form. `ProviderClient::service_tiers(route)` lists the tiers the adapter can send for a route. The default is `[default]`.

| Adapter | Tiers | Wire form |
| --- | --- | --- |
| `openai_compat` (OpenAI, OpenRouter, Bedrock Mantle) | all | body `service_tier` |
| `anthropic_compat` | default, priority | `speed: "fast"` (with the `fast-mode-2026-02-01` beta for Opus 5 and Opus 4.8); default sends `service_tier: "standard_only"` |
| Vertex Anthropic | default, priority | `speed: "fast"` |
| Vertex Gemini | default, flex, priority | shared request type header |
| Bedrock Converse and InvokeModel | default, flex, priority | `serviceTier` body or `X-Amzn-Bedrock-Service-Tier` header |
| GitHub Copilot, TypeSafe | default | none |

An explicit `default` on a default-only adapter sends nothing.

### 3. Served tier

Adapters that translate a response write the served tier where `served_service_tier` reads it: a top-level `service_tier` on OpenAI-shaped responses and stream chunks, and `usage.speed` or `usage.service_tier` on native Messages. Vertex maps `trafficType`, and Bedrock maps its `serviceTier`.

`scale`, `reserved`, and `batch` lanes read as no tier. They bill outside per-token tier rates.

### 4. Pricing

The billed tier is the served tier, else the requested tier, else default. A provider-reported cost (OpenRouter `usage.cost`) still wins over catalog rates.

Catalog schedules gain `service_tiers`: one rate schedule per non-default tier, with its own context tiers. The store keeps them in `model_pricing.service_tiers_json`.

- **LiteLLM**: keys `<rate>_priority`, `<rate>_flex`, `<rate>_ultrafast`, and `<rate>_above_<N>k_tokens_<tier>`.
- **models.dev**: `experimental.modes.<name>.cost`. The mode's `provider.body.service_tier` or `speed: "fast"` gives the tier.
- **OpenRouter**: no tier rates. OpenRouter reports the cost of each request.

For each tier, the first source in pricing precedence whose tier schedule covers every context-tier threshold of the merged base schedule wins. When no source covers them all, the first source that defines the tier wins, and long prompts at that tier fail closed. The coverage rule matches how the base merge prefers complete schedules; without it, models.dev's priority rate for GPT-6 (no 272k tier) would beat LiteLLM's complete schedule and leave every long priority request unpriced.

The gateway fails closed. A request billed at a tier with no rate for a token bucket it used is unpriced, with the reason `service_tier:<tier>`. A premium tier is never billed at the standard rate. The same rule applies when the base schedule has a context tier but the service tier has none for that threshold.

`pricing_override` on a route prices the tier the route serves by default. Its optional `service_tiers` list gives flat rates for other tiers.

### 5. Routing

A route can serve a requested tier when its adapter supports the tier and one of these is true:

- the route has provider-reported cost;
- the route has a rate for the tier;
- the route has no price at default either.

The last case keeps unpriced routes usable: they bill as unpriced at every tier. When no route can serve the tier, the gateway returns 400 `no configured route supports service tier`. `ResolvedGatewayRequest` carries the eligible tiers for each route, built from the in-memory catalog snapshot like capability hints.

### 6. Policy

A model's optional `service_tiers` lists the non-default tiers callers may request. Unset allows every tier. A requested tier outside the list returns 403 `service_tier_not_allowed`. A tier that a route forces is allowed: the admin who configured the route chose it.

There is no per-key or per-team tier policy. Model grants already control access to a `-fast` model key, and that covers the per-key case.

Batches bill at the provider's batch rate, and batch adapters send no tier. Batch creation strips a `default` tier from each item and rejects any other tier with 400, so no spelling reaches a batch provider unpriced. Batch route selection skips a route that forces a non-default tier.

### 7. Records

`usage_cost_events` gains `requested_service_tier` and `served_service_tier`. The request log stores both in its metadata. The admin UI shows them on the request detail page.

### 8. Discovery

- `/v1/models` lists `service_tiers` for each model. OMP reads this field.
- `/model_group/info` adds `service_tier` to `supported_openai_params` when a route supports a non-default tier.
- The OpenCode plugin adds a variant for each non-default tier.

### 9. Configuration cutover

Routes set `service_tier` as a typed field. Route `extra_body` cannot set `service_tier`, `serviceTier`, or `speed`, and `extra_headers` cannot set `X-Vertex-AI-LLM-Shared-Request-Type` or `X-Amzn-Bedrock-Service-Tier`. Config load fails with a message that names the typed field.

## Trade-offs

- Billing the requested tier when a provider does not report one can overcharge when the provider silently serves default. Undercharging a premium tier is worse for budgets.
- Changing the tier breaks a provider's prompt cache. The gateway does not try to prevent this.
- The route-eligibility check uses catalog rates at request time. A catalog refresh that removes a tier rate makes that tier unavailable on catalog-priced routes until the rate returns or an admin adds a `pricing_override`.

## Follow-up

- Per-key tier budgets, if teams need a limit on fast spend separate from model grants.
- A UI editor for route `service_tier` and override tier rates. Admins edit them in `gateway.yaml` today.
