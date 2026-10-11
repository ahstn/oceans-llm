# Model Routing and APIs

Oceans gives callers stable gateway model names while admins control the providers, upstream models, capabilities, and compatibility settings behind them. This page explains how an authenticated API request becomes one provider request and how behavior differs across the public API families.

`See also`: [Configuration Reference](configuration-reference.md), [Provider API Compatibility](../reference/provider-api-compatibility.md), [Identity and Access](../access/identity-and-access.md), [Request Lifecycle and Failure Modes](../reference/request-lifecycle-and-failure-modes.md), [Pricing Catalog and Accounting](pricing-catalog-and-accounting.md), [Observability and Request Logs](../operations/observability-and-request-logs.md)

## Public API surface

The gateway exposes these authenticated endpoints:

| Endpoint | API family | Route requirement |
| --- | --- | --- |
| `GET /v1/models` | OpenAI- and Anthropic-compatible model discovery | Model is visible to the caller |
| `POST /v1/chat/completions` | OpenAI Chat Completions | `chat_completions: true` |
| `POST /v1/responses` | OpenAI Responses | `responses: true` |
| `POST /v1/embeddings` | OpenAI Embeddings | `embeddings: true` |
| `POST /v1/decisions` | System One Decisions | `decisions: true` |
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
  -> enabled routes with positive weights
  -> API and feature capability checks
  -> continuation origin, session binding, or routing policy
  -> one eligible route
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

### Use the GPT-6 Astra routes

The root and deployment gateway configurations route `openai-fast` and `openai-fast-v2` to `gpt-6-astra`. The root local configuration also exposes `gpt-6-astra` directly. The two OpenAI routes retain their 9:1 weights and existing credential references.

Clients must follow the [official OpenAI migration guide](https://developers.openai.com/api/docs/guides/latest-model):

- Send tool calls through `/v1/responses`. Astra supports Chat Completions for requests without tools. Oceans does not convert Chat Completions requests into Responses requests.
- Replace explicit `none` or `minimal` reasoning effort with `low`. Preserve other supported effort settings. Use `reasoning.effort` in Responses and `reasoning_effort` in Chat Completions.
- Omit `temperature`, `top_p`, and `top_logprobs`. Also omit `logprobs` in Chat Completions and `message.output_text.logprobs` from Responses `include`.
- When upgrading a client from GPT-5.5 or earlier, replace `prompt_cache_retention` with `prompt_cache_options: {"ttl": "30m"}`.
- For EU data residency, use Standard processing; omit `fast` and `priority` service tiers.

For example, send this body to `/v1/responses` through an authorized gateway key:

```json
{
  "model": "openai-fast",
  "input": "Explain the purpose of a gateway model alias in one sentence.",
  "reasoning": { "effort": "low" }
}
```

The gateway forwards client parameters; these route changes do not remove unsupported fields or change client reasoning settings. General route capabilities do not express Astra's endpoint-specific tool restriction. Check client request bodies before switching existing traffic.

Apply changed configuration through the normal config-seeding workflow. Editing these files does not update an already seeded deployment. Confirm Astra access for both OpenAI credentials, refresh pricing metadata, and verify a Responses request before enabling traffic. Configuration validation does not prove provider access or live inference. See [Pricing Catalog and Accounting](pricing-catalog-and-accounting.md) for pricing refresh and missing-price behavior.

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
| Chat Completions and Anthropic Messages | `reasoning_effort`; `reasoning.effort`; `output_config.effort`; `thinking.effort`; and per-message `reasoning.effort`, `output_config.effort`, or `thinking.effort` |
| Responses | `reasoning.effort`, plus flattened `reasoning_effort`, `output_config.effort`, and `thinking.effort` compatibility fields |
| Bedrock-shaped JSON | `additionalModelRequestFields.{reasoning,output_config,thinking}.effort` and the snake-case `additional_model_request_fields` form |
| Gemini-shaped JSON | `generationConfig.thinkingConfig.thinkingLevel` and the snake-case `generation_config.thinking_config.thinking_level` form |
| Chat-template and batch JSON | `chat_template_kwargs.reasoning_effort`, `chat_template_args.reasoning_effort`, the applicable request paths above, and nested `messages` or `input` item paths |

Known values at or below the ceiling pass unchanged. The disable values `none` and `off` also pass as lower than `minimal`; provider compatibility still determines whether a given field accepts either spelling. A known value above the ceiling returns `invalid_request`; the gateway does not clamp or mutate it. Unknown future strings and malformed non-string values also return `invalid_request` while a ceiling is active, so a new provider value cannot silently bypass policy. Omitted effort fields and explicit `null` values pass. If the effective model policy is omitted, this categorical check does not reject the request.

This policy applies only to explicit categorical values. It does not cap numeric budgets such as `thinking.budget_tokens` or `reasoning.budget_tokens`, and it does not override an effort default chosen internally by the provider when no explicit value is present. Existing provider mapping still owns conflicts between two explicit effort fields; every field must first satisfy the gateway ceiling.

Validation runs after alias resolution and before route selection, provider transforms, or budget enforcement. Route `extra_body` values are validated during configuration startup against both their target model and every alias policy that can resolve to that target; see [Configuration Reference](configuration-reference.md#reasoning-effort-ceilings).

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

Each provider-backed model can define several routes. Admins declare routes and routing policy in YAML, then apply them through the normal config-seeding workflow. Routing policy is not editable in the admin UI.

| Field | Behavior |
| --- | --- |
| `id` | Stable route name, unique within the model; required when the model defines `routing` |
| `enabled` | Excludes the route when `false` |
| `priority` | Lower values are considered before higher values |
| `weight` | Must be positive for eligibility; controls selection probability with `weighted_random` |
| `provider` | Selects the configured provider connection |
| `upstream_model` | Names the model sent to that provider |

Oceans treats models placed in one routing pool as equivalent. Existing API and feature capability checks still exclude routes that cannot serve the request. A route that needs a caller's Copilot connection also requires that connection before selection.

### Choose a routing policy

The optional model-level `routing` block selects a policy:

| `routing.strategy` | Selection for a new placement |
| --- | --- |
| `preferred` | Use the eligible route with the lowest priority value. Break equal-priority ties by stable internal route ID. |
| `weighted_random` | Use weighted selection among eligible routes at the lowest priority. This is the default strategy. |
| `round_robin` | Rotate among eligible routes at the lowest priority, ordered by stable internal route ID. Share a database cursor for each resolved model and eligible route set. |

If no route in one priority tier is eligible, the gateway can select from the next tier. YAML list order does not establish preference. Use different priority values when one provider should be preferred over another.

For example, this pool rotates new sessions between two configured provider connections:

```yaml
models:
  - id: coding-pool
    routing:
      strategy: round_robin
      affinity:
        idle_timeout_seconds: 3600
    routes:
      - id: primary
        provider: openai-primary
        upstream_model: gpt-6-sol
        priority: 10
        weight: 3
      - id: secondary
        provider: openai-secondary
        upstream_model: gpt-6-sol
        priority: 10
        weight: 1
```

The provider connections and upstream model names must exist in the deployment. The same structure can combine different provider types, such as Copilot, OpenRouter, and Bedrock, with each route's native model name and compatibility settings.

Both example routes receive turns under `round_robin`; the `3:1` weights affect placement only if the strategy changes to `weighted_random`. Round robin counts new placements, not requests within an active session. Requests without session affinity also count as new placements. Callers with different eligible route sets use separate cursors, so a restricted caller cannot consume another set's turns. It does not balance tokens, cost, or concurrent work.

Omit `routing` to retain the existing weighted selection on every request, without session affinity. `routing: {}` also selects `weighted_random` without affinity, but requires explicit route IDs. Aliases cannot define their own `routing` block; they use the resolved target's policy.

### Keep route identities stable

An explicit route `id` must contain 1–128 ASCII letters, digits, periods, underscores, or hyphens. It must be unique within the model. Every route needs an ID when the model defines `routing`.

Keep the ID when changing a route's priority, weight, or position in YAML. These edits preserve route identity and do not break a valid session binding. A provider or upstream model change creates a different route identity.

The execution fingerprint uses the initialized provider's endpoint, authentication, and headers, plus request-affecting route settings and the caller's linked provider credential ID. Changes to these values invalidate a binding. Display, pricing, timeout, and batch settings do not affect the fingerprint. Normal provider token refresh does not change it.

Providers using AWS `default_chain` or GCP `adc` or `service_account` authentication need a `routing_account_scope` when a configured routing pool uses them. This label identifies the underlying account or principal. Change it and restart the gateway when that identity changes. Temporary token refresh for the same principal needs no label change. The gateway relies on the configured label; it cannot detect an incorrect label or an unreported ambient identity change. See [Routing Account Scope](configuration-reference.md#routing-account-scope) for syntax.

### Keep sessions on one route

Add `routing.affinity: {}` to enable session affinity with the default idle timeout of 3,600 seconds. Set `idle_timeout_seconds` to another positive integer when required. Omit `affinity` to disable it.

Callers can send `x-oceans-session-id` with a conversation ID. Affinity keys include the API key, requested gateway model, and session namespace. Two callers with the same session string do not share a binding. Two aliases that resolve to one target also keep separate bindings. Use the same API key, model name, and session source on subsequent requests.

The gateway also accepts the session identifiers already used by Claude Code, Codex, OpenCode, Pi, and Oh My Pi. Harness detection must identify the relevant client. This shares the parsers used by agent session analysis, but routing rejects malformed or conflicting values. If the canonical header and a recognized harness session ID are both present, they must agree. Session IDs must contain 1–256 ASCII letters, digits, periods, underscores, colons, or hyphens. Prompt cache keys and prompt content do not identify sessions.

For a new session, the gateway selects a route and stores the binding atomically in PostgreSQL or libSQL. Gateway replicas using that database share the binding. Concurrent first requests for the same session and execution context therefore use one route. Successful responses extend the idle deadline; streams extend it only after a clean completion. Errors and cancelled streams do not refresh it.

If a database error prevents a soft affinity refresh, the gateway preserves the successful response and records a warning. The existing binding can then expire sooner than intended. Response-origin persistence is required and has different failure behavior, as described below.

An active binding takes precedence over the routing strategy while its route remains eligible. Adding a more preferred route does not move active sessions. After the idle deadline, or when the bound route is disabled, removed, changed, or no longer eligible, the next request uses the routing strategy again. A late completion from an old binding cannot refresh a replacement binding.

Affinity applies to Chat Completions, Responses, and Anthropic Messages. The API family forms part of the execution fingerprint because provider state and cache behavior can differ by API. Switching API families within a session can therefore create a new binding. The routing strategy also applies to embeddings and decisions, which do not use session affinity. Durable batch admission keeps its existing route selection and stored route lifecycle.

The idle timeout controls gateway routing state. It does not set provider cache retention or prove a cache hit. Provider eviction, account and region boundaries, prompt changes, and routing within a provider can still affect cache reuse. Keep OpenRouter's own provider policy stable when cache continuity depends on its selected upstream.

### Keep Responses continuations on their origin

For a model with a `routing` policy, a `previous_response_id` is bound to the route that created it. The gateway stores caller-scoped response ownership for 30 days, separately from the session idle timeout. The origin takes precedence over ordinary placement and session affinity.

The gateway must store the origin before it completes a successful Responses result. If that write fails, the request or stream reports an error. A successful upstream result alone is not sufficient to promise a safe continuation.

The gateway rejects a continuation when its origin is unknown, expired, changed, or unavailable for that caller and request. It does not send the identifier to another provider. If affinity is enabled and a session ID is present, a known continuation binds that session to its origin without advancing the round-robin cursor. A successful continuation refreshes that binding's idle deadline.

Responses created before origin tracking was enabled cannot be continued through a routing pool unless their origin is known. Opaque `conversation` references are not supported in configured routing pools. Use full request history when a new placement is needed.

### Selection does not retry provider failures

The gateway executes one selected route. It does not retry another route after an upstream error and does not send the request to several providers. Session affinity does not add failover.

For example, weights of `3` and `1` at the same priority produce weighted first-route selection. They do not mean “try the first provider three times, then fail over to the second.” Configure each selectable route as a valid execution target and monitor provider failures independently.

## Gate routes by capability

Capabilities remove incompatible routes before provider execution:

| Capability | Required when the request uses |
| --- | --- |
| `chat_completions` | `/v1/chat/completions` or a compatible chat path |
| `responses` | `/v1/responses` |
| `embeddings` | `/v1/embeddings` |
| `decisions` | `/v1/decisions` |
| `stream` | Streaming output |
| `tools` | Function, custom, MCP, or other supported tools |
| `vision` | Image or supported multimodal input |
| `json_schema` | Structured output using JSON Schema |
| `developer_role` | A developer-role message |

Effective support is the intersection of configured capability metadata and provider runtime support. Capability defaults are permissive, so partial provider routes should explicitly disable unsupported families and features.

`decisions` defaults to `false`. A decisions route must explicitly enable it and disable `chat_completions`, `responses`, `embeddings`, and `stream`. Current Decisions transports are native TypeSafe and OpenRouter routes with `compatibility.openrouter.api: decisions`.

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

Generated client configurations and `GET /v1/models` use the same conservative limits. Route-level detail, pricing, and source provenance stay in `GET /v1/model-metadata`.

The context value is metadata, not request-time token enforcement. Oceans does not currently tokenize every request and reject an oversized prompt before provider execution.

## Understand API-specific behavior

### Model discovery

`GET /v1/models` returns the gateway models visible to the authenticated API key. One response serves both OpenAI and Anthropic clients: Claude Code calls it through `ANTHROPIC_BASE_URL`, and the two list shapes use different keys.

| Field | Source |
| --- | --- |
| `id`, `object`, `type`, `owned_by` | Gateway model key; `owned_by` is always `oceans-llm` |
| `created`, `created_at` | Catalog release date of the first route that has one, else the Unix epoch |
| `display_name`, `name` | Catalog display name, else the model key; `name` is the OpenRouter spelling |
| `description`, `alias_of` | Gateway model configuration |
| `context_length`, `max_input_tokens`, `max_tokens` | Conservative limits across enabled routes |
| `architecture` | Input and output modalities every route with catalog data shares; decision-capable models add a `decisions` output modality |
| `capabilities` | Anthropic `ModelCapabilities` shape (see below) |
| `supported_endpoint_types` | new-api endpoint types, preferred first (see below) |
| `client_hints` | Harness settings for chat-shaped models (see below) |
| `has_more`, `first_id`, `last_id` | The list is never paginated, so `has_more` is always `false` |

`capabilities` follows the Anthropic SDK types exactly and describes what `POST /v1/messages` can accept for the model. `batch`, `citations`, `code_execution`, and `context_management` are always unsupported because the gateway does not serve those Anthropic features. `/v1/messages` hands content blocks, `thinking`, and `output_config` to the route unchanged, so `effort`, `thinking`, `image_input`, `pdf_input`, and `structured_outputs` are reported only when every eligible chat-capable route's upstream speaks Anthropic Messages. Selection can reach later priority tiers, including through an active session binding, so a single eligible non-Anthropic route turns these off. Those Anthropic routes are `anthropic_compat`, Vertex `anthropic/*`, GitHub Copilot with `chat_api: anthropic_messages`, and Bedrock with an Anthropic `api_style`. For other models, `architecture.input_modalities` still describes what the model accepts through the OpenAI APIs. Effort levels come from the model's thinking policy and are clamped to the effective ceiling, which is the strictest `max_reasoning_effort` across the alias chain. `thinking.types.adaptive` is set for Claude families that accept only adaptive thinking; `enabled` means manual budgets are accepted.

`client_hints` lists every gateway API that can serve the model, based on route capabilities, not on the model name:

```json
{
  "api_formats": ["openai-chat-completions", "openai-responses", "anthropic-messages"],
  "preferred_api_format": "anthropic-messages",
  "harnesses": {
    "opencode": { "npm": "@ai-sdk/anthropic", "variants": { "...": "..." } },
    "pi": { "api": "anthropic-messages", "compat": { "forceAdaptiveThinking": true } },
    "claude_code": { "model_env_var": "ANTHROPIC_DEFAULT_OPUS_MODEL" }
  }
}
```

`anthropic-messages` is listed for every chat-capable model because `/v1/messages` is translated onto the chat pipeline. `preferred_api_format` comes from the primary route's provider type: `anthropic_compat` and Vertex `anthropic/*` routes prefer Anthropic Messages, GitHub Copilot follows its configured `chat_api`, and Bedrock follows its `api_style`. A route whose upstream speaks the Responses API prefers it even when chat completions is also served. Other provider types fall back to model-name matching. OpenCode `variants` and Pi `thinkingLevelMap` respect the effective reasoning ceiling: OpenCode presets above it are dropped, and Pi levels above it map to the strongest effort the thinking policy sends within the ceiling. If the policy has no such effort, for example Claude under a `minimal` ceiling, those Pi levels are `null`. Harness blocks use each harness's own config keys and match the snippets from [Client Harness Configuration](client-harness-configuration.md). They never contain a base URL or API key. Embedding-only and Decisions-only models omit `client_hints`.

`supported_endpoint_types` uses the new-api vocabulary (`anthropic`, `openai`, `openai-response`) so proxy-aware clients can pick a wire per model. The preferred format comes first: `anthropic` for Anthropic-preferred models, and `openai-response` before `openai` for Responses-preferred models. Clients such as omp with `discovery.type: proxy` choose Anthropic Messages whenever `anthropic` is listed. The gateway therefore lists `anthropic` only when `preferred_api_format` is `anthropic-messages`, even though `/v1/messages` accepts every chat-capable model. Use `client_hints.api_formats` for the complete list.

Decision models served through `POST /v1/decisions` have no new-api endpoint type, so their `supported_endpoint_types` is empty and they carry no `client_hints`. Following OpenRouter, they report `decisions` in `architecture.output_modalities`. Proxy-discovery clients that do not read that modality may list them as chat models.

Visibility does not guarantee that a route can execute every API family. A model can be visible while all routes are disabled, non-viable, or incompatible with the requested operation.

### Chat Completions

`POST /v1/chat/completions` uses the shared authentication, model resolution, route planning, budget, logging, and accounting path. Compatibility transforms apply after route selection and before the provider request.

### Anthropic Messages

`POST /v1/messages` and `POST /messages` accept Anthropic Messages-compatible requests. The gateway supports Anthropic-style `x-api-key` authentication and returns Anthropic-compatible JSON or server-sent events.

Messages support still depends on the selected provider and route. Disable unsupported tools, vision, or other features so the request fails before provider execution.

### Responses

`POST /v1/responses` requires the `responses` capability and invokes the provider's Responses implementation. Streaming preserves `response.*` event names rather than converting them into Chat Completions chunks. Usage is normalized from Responses token fields.

### Decisions

`POST /v1/decisions` accepts a `state` value and named `noul`, `choice`, or `score` questions. It is a first-class API family and is not translated through Chat Completions or Responses.

Oceans sends OpenRouter routes to `/api/alpha/decisions`, native TypeSafe routes to `/v1/systemone`, and reserves `/v1/decisions` as the default upstream path for future compatible adapters. Decisions routes are non-streaming and skip prompt and model-response guardrails. They still use gateway authentication, model grants, budgets, request logs, provider attempts, and usage accounting.

See [TypeSafe](../providers/typesafe.md) for the request shape and route examples.

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
| `invalid_request` | Model policy, session identity, continuation origin, or route capability checks rejected the request | Explicit effort fields, session IDs, response origin, API family, and required feature flags |
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
7. Exercise the configured strategy with distinct sessions or no session ID. Weighted selection does not promise a fixed sequence; round robin rotates new placements.
8. Repeat one session and confirm the same provider is selected. Test expiry with a short idle timeout in a test deployment, then restore the intended value.
9. With multiple gateway replicas, send concurrent first requests for one session and confirm they use one route.
10. For Responses continuation, confirm a known ID reaches its origin and an unknown ID fails before provider execution.

For failures after route selection, continue with [Request Lifecycle and Failure Modes](../reference/request-lifecycle-and-failure-modes.md). For provider-specific request and response behavior, use [Provider API Compatibility](../reference/provider-api-compatibility.md).
