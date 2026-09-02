# Model Routing and APIs

Oceans gives callers stable gateway model names while admins control the providers, upstream models, capabilities, and compatibility settings behind them. This page explains how an authenticated API request becomes one provider request and how behavior differs across the public API families.

`See also`: [Configuration Reference](configuration-reference.md), [Provider API Compatibility](../reference/provider-api-compatibility.md), [Identity and Access](../access/identity-and-access.md), [Request Lifecycle and Failure Modes](../reference/request-lifecycle-and-failure-modes.md), [Pricing Catalog and Accounting](pricing-catalog-and-accounting.md), [Observability and Request Logs](../operations/observability-and-request-logs.md)

## Public API surface

The gateway exposes these authenticated endpoints:

| Endpoint | API family | Route requirement |
| --- | --- | --- |
| `GET /v1/models` | OpenAI-compatible model discovery | Model is visible to the caller |
| `POST /v1/chat/completions` | OpenAI Chat Completions | `chat_completions: true` |
| `POST /v1/responses` | OpenAI Responses | `responses: true` |
| `POST /v1/embeddings` | OpenAI Embeddings | `embeddings: true` |
| `POST /v1/messages` | Anthropic Messages | Chat-capable route with provider support |
| `POST /messages` | Anthropic Messages compatibility alias | Chat-capable route with provider support |
| `POST /api/v1/batches` | Durable batch admission for Chat Completions, Responses, or Embeddings items | Capability matching the batch `endpoint` |

Provider support varies by API family. A provider that supports Chat Completions does not necessarily support Responses, embeddings, Anthropic Messages, or every hosted tool. Use [Provider API Compatibility](../reference/provider-api-compatibility.md) as the current support matrix.

## Follow the request path

A model request passes through these routing stages:

```text
requested model
  -> caller access and model grants
  -> tag selection, when requested
  -> alias resolution and effective model policy
  -> explicit reasoning-effort validation
  -> enabled and weighted routes
  -> API and feature capability checks
  -> first eligible route
  -> provider compatibility transforms
  -> upstream provider request
```

Oceans records the requested model, resolved model, selected provider, and provider attempt in request observability. This keeps the caller-facing identity separate from the route that executed it.

## Configure a provider-backed model

A provider-backed model has one or more routes:

```yaml
models:
  - id: fast
    description: General-purpose low-latency model
    tags: [chat, fast]
    rank: 10
    routes:
      - provider: openai-primary
        upstream_model: gpt-5-mini
        priority: 10
        weight: 3
        enabled: true
        capabilities:
          chat_completions: true
          responses: true
          embeddings: false
          stream: true
          tools: true
          vision: true
          json_schema: true
          developer_role: true
      - provider: openai-secondary
        upstream_model: gpt-5-mini
        priority: 10
        weight: 1
        enabled: true
        capabilities:
          chat_completions: true
          responses: true
          embeddings: false
          stream: true
          tools: true
          vision: true
          json_schema: true
          developer_role: true
```

The model ID is the stable name callers send in the `model` field. Each route identifies a configured provider and the model name expected by that provider.

See [Configuration Reference](configuration-reference.md) for complete field syntax, provider credentials, compatibility profiles, and validation constraints.

## Understand requested and resolved models

Oceans keeps two model identities:

| Identity | Meaning |
| --- | --- |
| Requested model | The model name or selected gateway model requested by the caller |
| Resolved model | The canonical provider-backed model after alias resolution |

Both identities are written to request logs. This distinction matters when an alias presents a stable client name while its target changes over time.

### Use model aliases

An alias is a gateway model that points to another gateway model:

```yaml
models:
  - id: coding-default
    alias_of: coding-primary

  - id: coding-primary
    routes:
      - provider: openai-primary
        upstream_model: gpt-5
```

A model cannot define both `alias_of` and `routes`. Startup rejects missing alias targets and cycles. Request resolution rejects alias chains beyond the supported depth.

Aliases are independent authorization keys. Access to `coding-default` does not imply access to `coding-primary`, and access to the target does not automatically grant access to the alias. Grant the identity callers are expected to request.

### Enforce reasoning effort ceilings

When one or more models in an alias chain define `max_reasoning_effort`, the gateway uses the strictest value across the complete requested-model-to-provider-backed-target chain. For example, an alias capped at `medium` that targets a model capped at `high` has an effective ceiling of `medium`. An uncapped chain member does not weaken a cap set elsewhere in the chain.

The canonical effort order is `minimal`, `low`, `medium`, `high`, `xhigh`, then `max`. The gateway checks every non-null explicit effort occurrence independently:

| API or body shape | Effort paths checked |
| --- | --- |
| Chat Completions and Anthropic Messages | `reasoning_effort`, `reasoning.effort`, `output_config.effort`, and `messages[*].output_config.effort` |
| Responses | `reasoning.effort`, plus flattened `reasoning_effort` and `output_config.effort` compatibility fields |
| Provider-shaped JSON and batch item bodies | The applicable request-level paths above and `messages[*].output_config.effort` |

Known values at or below the ceiling pass unchanged. A known value above the ceiling returns `invalid_request`; the gateway does not clamp or mutate it. Unknown future strings and malformed non-string values also return `invalid_request` while a ceiling is active, so a new provider value cannot silently bypass policy. Omitted effort fields and explicit `null` values pass. If the effective model policy is omitted, this categorical check does not reject the request.

This policy applies only to explicit categorical values. It does not cap numeric budgets such as `thinking.budget_tokens` or `reasoning.budget_tokens`, and it does not override an effort default chosen internally by the provider when no explicit value is present. Existing provider mapping still owns conflicts between two explicit effort fields; every field must first satisfy the gateway ceiling.

Validation runs after alias resolution and before route selection, provider transforms, or budget enforcement. Route `extra_body` values are validated during configuration startup; see [Configuration Reference](configuration-reference.md#reasoning-effort-ceilings).

### Use tag selectors

A caller can request a concrete model ID or a selector such as:

```json
{
  "model": "tag:chat,fast"
}
```

Tag selectors use AND semantics. The selected model must contain every requested tag and must be available in the caller's effective model set.

Oceans applies API-key grants, principal restrictions, and model allowlists before choosing a tag candidate. It then orders eligible models by ascending `rank` and model ID. A blocked model is skipped rather than selected and rejected later.

Use tags for policy-oriented choices such as `fast`, `coding`, or `low-cost`. Use a concrete model ID when the caller requires a specific gateway contract.

## Control route selection

Each provider-backed model can define several routes. Route selection uses these fields:

| Field | Behavior |
| --- | --- |
| `enabled` | Excludes the route when `false` |
| `priority` | Lower values are considered before higher values |
| `weight` | Controls weighted selection among routes with the same priority |
| `provider` | Selects the configured provider connection |
| `upstream_model` | Names the model sent to that provider |

Routes with non-positive weight are excluded. Within each priority group, weight changes the probability that a route is selected first.

### Weight is not fallback

The gateway currently executes only the first eligible route. It does not retry another route after an upstream error and does not send the request to several providers.

For example, weights of `3` and `1` at the same priority produce weighted first-route selection. They do not mean “try the first provider three times, then fail over to the second.” Configure each selectable route as a valid execution target and monitor provider failures independently.

## Gate routes by capability

Capabilities remove incompatible routes before provider execution:

| Capability | Required when the request uses |
| --- | --- |
| `chat_completions` | `/v1/chat/completions` or a compatible chat path |
| `responses` | `/v1/responses` |
| `embeddings` | `/v1/embeddings` |
| `stream` | Streaming output |
| `tools` | Function, custom, MCP, or other supported tools |
| `vision` | Image or supported multimodal input |
| `json_schema` | Structured output using JSON Schema |
| `developer_role` | A developer-role message |

Effective support is the intersection of configured capability metadata and provider runtime support. Capability defaults are permissive, so partial provider routes should explicitly disable unsupported families and features.

For example, an embedding-only route should normally disable unrelated capabilities:

```yaml
capabilities:
  chat_completions: false
  responses: false
  embeddings: true
  stream: false
  tools: false
  vision: false
  json_schema: false
  developer_role: false
```

Capability checks fail at the gateway edge. They do not make an unsupported upstream feature available merely because its flag is enabled.

## Apply compatibility profiles

Capabilities and compatibility have different purposes:

- `capabilities` decides whether a route may execute a request.
- `compatibility` adjusts the provider request after route selection.

OpenAI-compatible Chat Completions profiles can remove unsupported `store` fields, rename token-limit fields, rewrite the `developer` role, handle `reasoning_effort`, control stream-usage requests, and omit unsupported empty tool lists.

Responses is a separate API family with its own typed request and streaming path. Chat Completions transforms are not used as Responses shims.

Provider-specific profiles also cover Amazon Bedrock API styles and OpenRouter provider policy. OpenRouter's `order`, `only`, `ignore`, zero-data-retention, latency, and price settings affect upstream selection inside the chosen OpenRouter route. They do not change Oceans route priority, weight, or single-route execution.

Put additive provider request fields that are not compatibility behavior in route `extra_body` or `extra_headers`. See [Provider API Compatibility](../reference/provider-api-compatibility.md) for supported profiles and API-specific constraints.

## Configure route metadata

`context_window_tokens` sets a deployment-specific context cap for one route. When the pricing catalog also knows the model limit, Oceans uses the smaller value. A configured cap above a known catalog limit fails startup.

The Models admin API reports logical-model metadata conservatively across selectable routes:

- Each token-limit dimension is the minimum known value.
- A dimension is unknown when any selectable route lacks it.
- Context provenance is `configured_override`, `catalog`, or `mixed`.
- Pricing uses the primary route and reports when pricing varies by route.

Generated client configurations use the same conservative limits. `GET /v1/models` remains an identity and discovery response; it does not expose Oceans-specific route metadata.

The context value is metadata, not request-time token enforcement. Oceans does not currently tokenize every request and reject an oversized prompt before provider execution.

## Understand API-specific behavior

### Model discovery

`GET /v1/models` returns gateway model identities visible to the authenticated API key. Visibility does not guarantee that a route can execute every API family. A model can be visible while all routes are disabled, non-viable, or incompatible with the requested operation.

### Chat Completions

`POST /v1/chat/completions` uses the shared authentication, model resolution, route planning, budget, logging, and accounting path. Compatibility transforms apply after route selection and before the provider request.

### Anthropic Messages

`POST /v1/messages` and `POST /messages` accept Anthropic Messages-compatible requests. The gateway supports Anthropic-style `x-api-key` authentication and returns Anthropic-compatible JSON or server-sent events.

Messages support still depends on the selected provider and route. Disable unsupported tools, vision, or other features so the request fails before provider execution.

### Responses

`POST /v1/responses` requires the `responses` capability and invokes the provider's Responses implementation. Streaming preserves `response.*` event names rather than converting them into Chat Completions chunks. Usage is normalized from Responses token fields.

### Batch admission

`POST /api/v1/batches` resolves the outer `model` and validates every item body against the resulting model policy. Chat Completions and Responses items use the same effort paths and fail-closed rules as synchronous requests. Validation happens before the batch job or any item is persisted; one violating item rejects the batch instead of leaving a partially admitted job. Embeddings items have no categorical effort field.

### Embeddings

`POST /v1/embeddings` requires the `embeddings` capability. OpenAI-compatible routes support provider-compatible embeddings endpoints. Native Vertex text embeddings require an explicitly supported Google embedding model and text input.

The Vertex mapper rejects unsupported token arrays, nested arrays, non-string values, empty input, multimodal payloads, and `encoding_format: "base64"` before provider execution. See [Provider API Compatibility](../reference/provider-api-compatibility.md) for the current model list.

## Diagnose routing failures

Start with the returned error and the request log:

| Symptom | Meaning | Check |
| --- | --- | --- |
| Model not found | The model ID does not exist, is not granted, or no tag candidate is accessible | Requested model, tags, API-key grants, and allowlists |
| `invalid_request` | Model policy or route capability checks rejected the request | Explicit effort fields and effective ceiling; API family and required feature flags |
| `no_routes_available` | No enabled, positively weighted, viable route remained | Route state, provider configuration, and weight |
| Provider error | The selected route reached the provider and the upstream request failed | Provider attempt, credentials, compatibility profile, and upstream response |
| Visible model cannot execute | Discovery access succeeded but no route supports this request | Route capabilities and provider runtime support |

Use the request ID to correlate the gateway response with **Observability > Request Logs** and exported traces. Request logs preserve requested and resolved model identities, the selected provider, and the provider attempt.

## Verify a routing change

1. Restart or reseed the gateway as required by the deployment method.
2. Call `GET /v1/models` with the intended API key.
3. Send one request for each enabled API family.
4. Exercise streaming, tools, vision, or structured output when the route advertises them.
5. Confirm the request log shows the expected requested model, resolved model, provider, and outcome.
6. Test one unsupported capability and confirm it fails before provider execution.
7. If routes share a priority, send enough representative requests to observe weighted selection without assuming a fixed sequence.

For failures after route selection, continue with [Request Lifecycle and Failure Modes](../reference/request-lifecycle-and-failure-modes.md). For provider-specific request and response behavior, use [Provider API Compatibility](../reference/provider-api-compatibility.md).
