# Model Routing and APIs

Oceans gives callers stable gateway model names while admins control the providers, upstream models, capabilities, and compatibility settings behind them. This page explains how an authenticated API request becomes one provider request and how behavior differs across the public API families.

`See also`: [Configuration Reference](configuration-reference.md), [Provider API Compatibility](../reference/provider-api-compatibility.md), [Identity and Access](../access/identity-and-access.md), [Request Lifecycle and Failure Modes](../reference/request-lifecycle-and-failure-modes.md), [Pricing Catalog and Accounting](pricing-catalog-and-accounting.md), [Observability and Request Logs](../operations/observability-and-request-logs.md)

## Public API surface

The gateway exposes these authenticated endpoints:

| Endpoint | API family | Route requirement |
| --- | --- | --- |
| `GET /v1/models` | OpenAI- and Anthropic-compatible model discovery | Model is visible to the caller |
| `GET /model_group/info` | LiteLLM-compatible model metadata | Model is visible to the caller |
| `POST /v1/chat/completions` | OpenAI Chat Completions | `chat_completions: true` |
| `POST /v1/responses` | OpenAI Responses | `responses: true` |
| `POST /v1/embeddings` | OpenAI Embeddings | `embeddings: true` |
| `POST /v1/images` | OpenAI Images generation | `images: true` |
| `POST /v1/images/generations` | OpenAI Images generation, at the OpenAI SDK path | `images: true` |
| `POST /v1/decisions` | Decisions, in the System One or OpenAI format | `decisions: true` |
| `POST /v1/systemone` | Decisions, in the System One format only | `decisions: true` |
| `POST /v1/messages` | Anthropic Messages | Chat-capable route with provider support |
| `POST /messages` | Anthropic Messages compatibility alias | Chat-capable route with provider support |
| `POST /v1/messages/count_tokens` | Anthropic Messages token counting | Chat-capable route whose provider has an upstream count API |
| `POST /messages/count_tokens` | Anthropic Messages token counting alias | Chat-capable route whose provider has an upstream count API |
| `POST /api/v1/batches` | Durable batch admission for Chat Completions, Responses, or Embeddings items | Capability matching the batch `endpoint` |

Clients whose base URL stops at the gateway root can drop the `/v1` prefix. `POST /chat/completions`, `POST /responses`, `POST /embeddings`, `POST /images`, `POST /images/generations`, `POST /decisions`, and `GET /models` share the handler, authentication, and body limit of their `/v1` endpoint. These aliases sit beside the admin UI (served under `/admin` by default) and the `/api/*` admin and batch routes without overlapping them. `/v1/model-metadata` and `/api/v1/batches` have no unversioned alias. `/model_group/info` has no `/v1` form, because LiteLLM serves it at the root.

Provider support varies by API family. A provider that supports Chat Completions does not necessarily support Responses, embeddings, image generation, Anthropic Messages, or every hosted tool. Use [Provider API Compatibility](../reference/provider-api-compatibility.md) as the current support matrix.

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
      - provider: openrouter
        upstream_model: openai/gpt-5-mini
        priority: 10
        weight: 1
        enabled: true
        capabilities:
          embeddings: false
```

The model ID is the stable name callers send in the `model` field. Each route identifies a configured provider and the model name expected by that provider.

Routes set only the capability flags that the gateway cannot find for itself. The OpenAI route sets none: the catalog states its API endpoints, tools, and image input. The OpenRouter catalog does not state API endpoints, so that route turns off `embeddings`. See [Gate routes by capability](#gate-routes-by-capability).

See [Configuration Reference](configuration-reference.md) for complete field syntax, provider credentials, compatibility profiles, and validation constraints.

### Use the GPT-6 Astra routes

The root and deployment gateway configurations route `openai-fast` and `openai-fast-v2` to `gpt-6-astra`. The root local configuration also exposes `gpt-6-astra` directly. Each uses the single `openai-prod` route and its `OPENAI_API_KEY` credential.

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

Apply changed configuration through the normal config-seeding workflow. Editing these files does not update an already seeded deployment. Confirm Astra access for the OpenAI credential, refresh pricing metadata, and verify a Responses request before enabling traffic. Configuration validation does not prove provider access or live inference. See [Pricing Catalog and Accounting](pricing-catalog-and-accounting.md) for pricing refresh and missing-price behavior.

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
        provider: openrouter
        upstream_model: openai/gpt-6-sol
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

Affinity applies to Chat Completions, Responses, and Anthropic Messages. The API family forms part of the execution fingerprint because provider state and cache behavior can differ by API. Switching API families within a session can therefore create a new binding. The routing strategy also applies to embeddings, images, and decisions, which do not use session affinity. Durable batch admission keeps its existing route selection and stored route lifecycle.

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
| `images` | `/v1/images` or `/v1/images/generations` |
| `decisions` | `/v1/decisions` or `/v1/systemone` |
| `stream` | Streaming output |
| `tools` | Function, custom, MCP, or other supported tools |
| `vision` | Image or supported multimodal input |
| `json_schema` | Structured output using JSON Schema |
| `developer_role` | A developer-role message |

### How the gateway finds each flag

Each capability flag can be `true`, `false`, or not set. Omit a flag to let the gateway find its value. Set a flag only to override that value. The gateway finds each flag in this order, and the first source with a value wins:

1. **Route config.** A flag that you set in `capabilities`.
2. **Compatibility.** Facts that the route's `compatibility` settings and provider type guarantee. See the table below.
3. **Catalog hints.** Facts from the [pricing catalog](pricing-catalog-and-accounting.md#pricing-catalog-refresh) entry of the route's upstream model.
4. **Defaults.** `images` and `decisions` are `false`. All other flags are `true`.

The provider adapter then caps the result. A route can turn off a feature that the provider supports, but it cannot turn on a feature that the provider does not support.

The gateway infers these flags from route compatibility:

| Route | Inferred flags |
| --- | --- |
| Bedrock `runtime_converse`, `runtime_openai_chat`, `mantle_openai_chat`, `mantle_anthropic_messages` | `chat_completions` and `stream` on. `responses`, `json_schema`, `embeddings`, `images`, and `decisions` off |
| Bedrock `runtime_anthropic_invoke` | `chat_completions` on. `responses`, `stream`, `json_schema`, `embeddings`, `images`, and `decisions` off |
| Bedrock `mantle_openai_responses` | `responses` and `stream` on. `chat_completions`, `embeddings`, `images`, and `decisions` off. `tools`, `vision`, and `json_schema` depend on the model |
| GitHub Copilot with `compatibility.github_copilot` | Every flag, from the Copilot route evidence |
| Native TypeSafe, `compatibility.openai_compat.decisions_api`, or `compatibility.openrouter.api: decisions` | `decisions` on. Every other flag off |
| Any other route | None |

Catalog hints fill these flags when the catalog states them:

| Flag | Catalog hint |
| --- | --- |
| `chat_completions`, `responses`, `embeddings`, `images` | The model's API endpoints. LiteLLM states them for many native models, for example an embedding model serves only embeddings. OpenRouter states them only for image-only models. models.dev does not state them |
| `tools` | The model supports tool calls |
| `json_schema` | The model supports structured output |
| `vision` | The model's input modalities include `image` |
| `stream`, `developer_role`, `decisions` | Never hinted |

A hint only fills a flag that config and compatibility leave unset. A hint can set such a flag to `false`. A hint never changes a flag that you set. Hints follow the catalog, so a catalog refresh can change a hinted flag. Set the flag explicitly when a route must not change.

Startup validation checks route config, then compatibility, then defaults. It does not use hints. It rejects a flag you set that contradicts compatibility, such as `stream: true` on a `runtime_anthropic_invoke` route, or `chat_completions: true` on a Decisions route.

Route selection, batches, the admin Models page, `GET /v1/models`, and `GET /v1/model-metadata` all use the same effective flags.

### Decisions and images

`decisions` defaults to `false`. Current Decisions transports are native TypeSafe, `openai_compat` routes with `compatibility.openai_compat.decisions_api: openai`, and the alpha route with `compatibility.openrouter.api: decisions`. On these routes the gateway turns `decisions` on and every other flag off, so they need no `capabilities` block.

`images` also defaults to `false`, so image generation is opt-in per route. Only `openai_compat` and `gcp_cloud_run_openai_compat` providers serve it. Enabling it on any other provider type, or on a decisions route, fails startup. The OpenRouter catalog marks image-only models such as `openai/gpt-image-2.5-flare`, so their routes get `images` on and the other API families off from catalog hints. Set the flags explicitly when the catalog does not list the model, or to keep the route stable:

```yaml
routes:
  - provider: openrouter
    upstream_model: openai/gpt-image-2.5-flare
    capabilities:
      images: true
      chat_completions: false
      responses: false
      embeddings: false
      stream: false
```

When the catalog does not describe an embedding model, an embedding-only route should disable unrelated capabilities:

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
- Pricing uses the primary route and reports when pricing varies by route. Catalog [context tiers](pricing-catalog-and-accounting.md#context-tiers) of the primary route appear in the model info dialog.

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
| `architecture` | Input and output modalities every route with catalog data shares; decision-capable models add a `decisions` output modality and image-capable models add an `image` output modality |
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
    "opencode": { "package": "@opencode/ai/providers/anthropic-compatible", "variants": [{ "id": "...", "settings": { "...": "..." } }] },
    "pi": { "api": "anthropic-messages", "compat": { "forceAdaptiveThinking": true } },
    "claude_code": { "model_env_var": "ANTHROPIC_DEFAULT_OPUS_MODEL" }
  }
}
```

`anthropic-messages` is listed for every chat-capable model because `/v1/messages` is translated onto the chat pipeline. `preferred_api_format` comes from the primary route's provider type: `anthropic_compat` and Vertex `anthropic/*` routes prefer Anthropic Messages, GitHub Copilot follows its configured `chat_api`, and Bedrock follows its `api_style`. A route whose upstream speaks the Responses API prefers it even when chat completions is also served. Other provider types fall back to model-name matching. OpenCode `variants` and Pi `thinkingLevelMap` respect the effective reasoning ceiling: OpenCode presets above it are dropped, and Pi levels above it map to the strongest effort the thinking policy sends within the ceiling. If the policy has no such effort, for example Claude under a `minimal` ceiling, those Pi levels are `null`. Harness blocks use each harness's own config keys and match the snippets from [Client Harness Configuration](client-harness-configuration.md). They never contain a base URL or API key. Embedding-only and Decisions-only models omit `client_hints`.

`supported_endpoint_types` uses the new-api vocabulary (`anthropic`, `openai`, `openai-response`) so proxy-style clients, which read new-api model lists, can pick a wire per model. The preferred format comes first: `anthropic` for Anthropic-preferred models, and `openai-response` before `openai` for Responses-preferred models. Proxy-style clients choose Anthropic Messages whenever `anthropic` is listed. The gateway therefore lists `anthropic` only when `preferred_api_format` is `anthropic-messages`, even though `/v1/messages` accepts every chat-capable model. Use `client_hints.api_formats` for the complete list.

omp (Oh My Pi) can also read this field with `discovery.type: proxy`. That mode ignores prices and the Responses endpoint type, and it sends no reasoning effort to OpenAI-family models. Configure omp with `discovery.type: litellm` instead, as the next section shows.

Decision models served through `POST /v1/decisions` have no new-api endpoint type, so their `supported_endpoint_types` is empty and they carry no `client_hints`. Following OpenRouter, they report `decisions` in `architecture.output_modalities`. Proxy-discovery clients that do not read that modality may list them as chat models.

Models with an eligible `capabilities.images` route report `image` in `architecture.output_modalities`, following OpenRouter. When the catalog has no modality data for an image-only model, its route reports `text` input (the prompt) and `image` output. The model also reports `text` output when any eligible route serves Chat Completions or Responses, also when that is a different route than the image route. `GET /v1/model-metadata` adds the same modalities to that route's `modalities`. An image-only model has no chat endpoint, so its `supported_endpoint_types` is empty and it carries no `client_hints`.

Visibility does not guarantee that a route can execute every API family. A model can be visible while all routes are disabled, non-viable, or incompatible with the requested operation.

### LiteLLM model metadata (`GET /model_group/info`)

`GET /model_group/info` returns model metadata in the LiteLLM proxy format. Clients that read LiteLLM metadata use it to get the limits, prices, and feature flags of each model. omp with `discovery.type: litellm` uses these values to select the API and the reasoning effort for each model, and to calculate cost.

The endpoint uses the same authentication and visibility as `GET /v1/models`. Send the API key in `Authorization: Bearer` or `x-api-key`. The key must have the `inference` scope:

- A request without a key gets `401`.
- A key without the `inference` scope gets `403` with code `insufficient_scope`.
- The response is `{ "data": [...] }`, with one row for each model that `/v1/models` shows to the same key.

Each row has these fields:

| Field | Source | Rule |
| --- | --- | --- |
| `model_group` | Gateway model key | Equal to `id` in `/v1/models` |
| `providers` | One lowercase vendor of the primary route's upstream model | `["openai"]` only when every eligible chat route serves the Responses API, and always when no route serves Chat Completions. See below |
| `mode` | Route transport | `chat` when the model serves Chat Completions or Responses. Otherwise `embedding` when the model serves embeddings, or `image_generation` when it serves only `/v1/images` |
| `max_input_tokens` | Conservative input limit, else the context window | `null` when unknown. omp uses this value as the context window |
| `max_output_tokens` | Conservative output limit | `null` when unknown |
| `input_cost_per_token`, `output_cost_per_token`, `cache_read_input_token_cost`, `cache_creation_input_token_cost` | Primary route price | USD per token. Input and output are present together, or no cost field is present. A cache cost is present only when its price is known |
| `supports_function_calling` | `tools` capability of the routes that serve tools | See below |
| `supports_vision` | `vision` capability of the routes that serve images | Always `true` or `false`, never `null` |
| `supports_reasoning` | Catalog reasoning flag or thinking policy of every eligible chat route | Not limited to Anthropic Messages routes. `false` when one chat route cannot reason |
| `supports_response_schema` | Structured output capability of the routes that serve JSON schemas | Standard LiteLLM field. omp ignores it |
| `supported_openai_params` | Derived from the fields above | See below |

The gateway stores prices in USD per million tokens. This endpoint divides each price by 1,000,000. For example, an input price of 4 USD per million tokens gives `"input_cost_per_token": 0.000004`. When the input or the output price is unknown, the row has no cost fields, and omp uses its own price for the model ID, if it has one. Many models have no cache price, so the row then has input and output costs only, and omp counts the cache tokens at 0.

`supported_openai_params` always contains `max_tokens` and `max_completion_tokens`. The gateway adds these values:

- `stream` and `stream_options` when an eligible chat route can stream.
- `tools` and `tool_choice` when the model supports tools.
- `reasoning_effort` when every eligible chat route accepts at least one effort level. omp enables reasoning effort only when this value is present. A model can have `supports_reasoning: true` without `reasoning_effort`, for example a Claude model that needs a manual thinking budget, or a model whose reasoning ceiling is below `low`.
- `response_format` when the model supports structured output.
- `temperature`, unless the catalog sets `temperature: false` for the model.

Reasoning uses every eligible chat route, not only the primary route. Route selection does not check for reasoning, so a request with `reasoning_effort` can go to any of these routes. A provider rejects `reasoning_effort` for a model that cannot reason, for example a Gemini model without thinking on Vertex.

Tools, images, JSON schemas, and streaming are different, because route selection checks for them. A request that uses tools goes only to the eligible routes whose provider and route settings serve tools. A route without them therefore does not remove the feature. The feature is `true` when at least one eligible chat route serves it, and the catalog says that the model on each of these routes has it. A route that serves tools for a model that the catalog does not know makes `supports_function_calling` `false`, because a tools request can go to that route. Streaming has no catalog flag, so one eligible chat route that can stream is enough.

The gateway finds the vendor for `providers` in this order:

1. The model family in the upstream model name: `anthropic` for Claude, `openai` for GPT and other OpenAI models, `google` for Gemini, `deepseek`, `qwen`, or `typesafe`. Hosts such as OpenRouter and Vertex are not vendors.
2. The prefix before `/` in the upstream model. For example, `acme/model-x` gives `acme`.
3. The provider type of the primary route, for example `openai_compat`.
4. `oceans-llm`.

When the vendor is `openai` but an eligible chat route does not serve Responses, the gateway sends the provider type of the primary route instead, for example `openai_compat`. If that value is empty or `openai`, the gateway sends `oceans-llm`.

When no eligible route serves Chat Completions but a route serves Responses, the gateway sends `["openai"]` for every vendor. For example, a Claude model on a Responses-only route gets `["openai"]`.

The `providers` rule controls the API that omp uses. omp sends a model with `providers: ["openai"]` to `/v1/responses` with `reasoning.effort`. omp sends all other models to the provider `api`, for example `/v1/chat/completions` with `reasoning_effort`. Route selection can use any eligible route, and a route that cannot serve Responses rejects a `/v1/responses` request. The gateway therefore reports `openai` only when every eligible chat route serves Responses. A GPT model behind a route that serves only Chat Completions does not get `["openai"]`. A model that no route serves over Chat Completions gets `["openai"]`, because a chat completions request would find no route.

No field selects Anthropic Messages. omp sends Claude models to `/v1/chat/completions` by default. The gateway translates these requests for Claude routes.

This example `models.yml` configures omp for Oceans:

```yaml
providers:
  oceans:
    baseUrl: https://<gateway>/v1
    apiKey: OCEANS_LLM_API_KEY
    api: openai-completions
    discovery:
      type: litellm
    modelOverrides:
      claude-fable-5.1:
        api: anthropic-messages
```

- `baseUrl` must end in `/v1`. omp removes `/v1` before it calls `/model_group/info`. Without `/v1`, omp sends chat requests to `/chat/completions`, which the unversioned alias serves.
- `modelOverrides` is optional. Add `api: anthropic-messages` for each Claude model that must use `/v1/messages`.
- omp gets the list of effort levels from its own catalog, by model ID. Aliases such as `claude-sonnet-latest` do not match that catalog, so omp shows its generic list for them. Set `modelOverrides.<id>.thinking` in omp to correct the list.
- omp ignores tiered prices, for example a higher price above a context threshold.
- `mode` has no value for decision models, so omp lists them as chat models.

The gateway does not serve `/model/info`, `/v1/model/info`, `/v2/model/info`, or `/v1/model_group/info`. omp reads `/model_group/info` first. omp calls the other endpoints when a row has some costs but not all four costs above zero, or when `supports_vision` is unknown. These calls get 404, and omp then uses the rows from `/model_group/info`. This happens only when omp refreshes its model list. See [ADR: LiteLLM model group info for omp discovery](../adr/2026-10-21-litellm-model-group-info.md).

### Chat Completions

`POST /v1/chat/completions` uses the shared authentication, model resolution, route planning, budget, logging, and accounting path. Compatibility transforms apply after route selection and before the provider request.

### Anthropic Messages

`POST /v1/messages` and `POST /messages` accept Anthropic Messages-compatible requests. The gateway supports Anthropic-style `x-api-key` authentication and returns Anthropic-compatible JSON or server-sent events.

Messages support still depends on the selected provider and route. Disable unsupported tools, vision, or other features so the request fails before provider execution.

#### Turn thinking off on adaptive-only Claude models

Newer Claude models accept only adaptive thinking, and each handles a request to turn thinking off differently. Claude Code sends `thinking: { "type": "disabled" }` on background work such as WebFetch summaries and session titles. The gateway keeps that request in the form the upstream model accepts, on `anthropic_compat`, Vertex `anthropic/*`, and Bedrock Anthropic Messages routes. Bedrock Converse routes apply the same rules to `additionalModelRequestFields.thinking`, with effort in `additionalModelRequestFields.output_config.effort`:

| Upstream model | `thinking.type: "disabled"` |
| --- | --- |
| Claude Opus 4.7, Opus 4.8, Sonnet 5 | Forwarded unchanged at every effort level. |
| Claude Opus 5, Haiku 5.5 | Forwarded unchanged at effort `high` or below. With `xhigh` or `max`, the gateway rejects the request. |
| Claude Sonnet 5.5 | Rewritten to `thinking: { "type": "between_tools" }`, the model's thinking-off setting. With `xhigh` or `max`, the gateway rejects the request. Callers can also send `between_tools` directly. |
| Claude Opus 5.5, Fable 5, Fable 5.1, Mythos Preview | Rejected. These models always think, so the gateway returns a 400 instead of running thinking the caller turned off. |

With thinking off, the gateway adds no adaptive thinking and no default effort, and forwards any effort the caller set. Other models reject `between_tools`. For Chat Completions on `anthropic_compat` and Vertex `anthropic/*` routes, adaptive-only models receive adaptive thinking and `output_config.effort: high` when `thinking` is omitted. Bedrock Chat Completions adds adaptive thinking only when the request sets an effort. Native `/v1/messages` and `/messages` requests use a separate thinking implementation across these transports: omitted thinking and effort remain omitted, and native `output_config.effort` remains independent of thinking mode.

To keep Claude Code background work on a model that always thinks, point it at a model that can turn thinking off. See [Claude Code background model](client-harness-configuration.md#claude-code-background-model).

### Anthropic Messages token counting

`POST /v1/messages/count_tokens` and `POST /messages/count_tokens` accept a Messages request body and return `{"input_tokens": N}`. They use the same authentication (`Authorization` or `x-api-key`), model resolution, model grants, reasoning-effort ceiling, and route selection as `POST /v1/messages`. Errors use the Anthropic error envelope.

The gateway sends the prompt-defining fields (`messages`, `system`, `tools`, `tool_choice`, `thinking`, and related prompt settings) to the selected route's upstream count API. It drops generation controls such as `temperature`, `stop_sequences`, `store`, and `stream`, and replaces the caller's `max_tokens` with a valid limit that leaves room for any thinking budget, so settings that only shape generation never fail a count. The route's prompt guardrails run exactly as they do for Messages before the request leaves the gateway: secrets are redacted, and a prompt that Messages would deny is denied here too. Guardrail decisions are recorded under the count's request ID.

| Selected route | Upstream count API |
| --- | --- |
| `anthropic_compat` | `POST /v1/messages/count_tokens` on the provider base URL |
| `gcp_vertex` with an `anthropic/*` upstream model | `count-tokens:rawPredict` on the Anthropic publisher |
| `aws_bedrock` with `api_style: runtime_anthropic_invoke` or `runtime_converse` | Bedrock Runtime `CountTokens`, which needs the `bedrock:CountTokens` [IAM action](../providers/aws-bedrock.md#token-counting-permissions). It supports only some models and Regions; see [the Bedrock limits](../providers/aws-bedrock.md#token-counting-permissions) |
| Any other route | None. Returns `501 not_implemented_error` |

The gateway never estimates a count. When the selected route has no upstream count API, the response is a 501 whose message names the provider key, provider type, and upstream model. A 404 would wrongly suggest the endpoint does not exist.

Counting generates no output, so it is not billed or logged as a request. It writes no usage-ledger row, does not check or charge budgets, creates no request log, and is not counted in the gateway request metrics. Because counts are free to the caller, each API key may have at most 8 counts in flight on each gateway instance. A further concurrent count is rejected before route selection with a 429 `rate_limit_error`, and a slot frees as soon as an earlier count finishes. That cap is per instance; to cap sequential volume across every replica, add a `count_tokens` [rate limit](rate-limits.md). Anthropic SDKs retry 429 responses with backoff. Counts are visible in traces as the `count_tokens` provider operation under the `/v1/messages/count_tokens` route, and as an `anthropic count_tokens request completed` log event with the input token count and whether guardrails were enabled.

Route selection runs exactly as it does for Messages, so a count matches the route the next Messages request would most likely use. A count does not refresh session affinity. With `round_robin` routing, a count still advances the rotation, so the next Messages request can land on a different route than the one that was counted.

### Responses

`POST /v1/responses` requires the `responses` capability and invokes the provider's Responses implementation. Streaming preserves `response.*` event names rather than converting them into Chat Completions chunks. Usage is normalized from Responses token fields.

### Decisions

`POST /v1/decisions` accepts two request formats. The System One format sends a `state` value and named `noul`, `choice`, or `score` questions. The OpenAI format sends an `input` (text, or user messages with text and image parts) and an ordered list of `predicate`, `choice`, or `score` questions. Oceans detects the format from the `input` or `state` key and returns the response in the same format. `POST /v1/systemone` accepts the System One format only. Decisions is a first-class API family and is not translated through Chat Completions or Responses.

Each route speaks one format. Native TypeSafe routes send System One to `/v1/systemone`. The alpha route sends System One to `/api/alpha/decisions`. `openai_compat` routes with `compatibility.openai_compat.decisions_api: openai` send the OpenAI format to `{base_url}/decisions`:

```yaml
providers:
  - id: openai-prod
    type: openai_compat
    base_url: https://api.openai.com/v1
    pricing_provider_id: openai
    auth:
      kind: bearer
      token: env.OPENAI_API_KEY

models:
  - id: luna
    routes:
      - provider: openai-prod
        upstream_model: gpt-6-luna
        compatibility:
          openai_compat:
            decisions_api: openai
        pricing_override:
          input_usd_per_million_tokens: "<OpenAI input rate>"
          cache_read_usd_per_million_tokens: "<OpenAI cached input rate>"
          output_usd_per_million_tokens: "0"
```

`decisions_api` requires an `openai_compat` provider. It turns `decisions` on and every other capability off, so the route needs no `capabilities` block. An explicit `decisions: false`, or an explicit `true` for another flag, fails startup. It cannot be combined with `compatibility.openrouter`. Decisions pricing for `gpt-6-luna` is input-only, so the route override sets output to zero. Use OpenAI's published input and cached-input rates.

Oceans selects only routes whose format can carry the request. When none can, it returns 400 before the upstream call. `safety_identifier` works only on OpenAI-format routes, and System One routes reject image inputs. Decisions routes are non-streaming and skip prompt and model-response guardrails. They still use gateway authentication, model grants, budgets, request logs, provider attempts, and usage accounting. Decisions requests have their own request metrics instead of the chat request metrics; see [Decisions Request Metrics](../operations/observability-and-request-logs.md#decisions-request-metrics) for what they record.

See [TypeSafe](../providers/typesafe.md) for the System One request shape and [Provider API Compatibility](../reference/provider-api-compatibility.md#decisions-api) for the translation rules.

### Batch admission

`POST /api/v1/batches` resolves the outer `model` and validates every item body against the resulting model policy. Chat Completions and Responses items use the same effort paths and fail-closed rules as synchronous requests. Validation happens before the batch job or any item is persisted; one violating item rejects the batch instead of leaving a partially admitted job. Embeddings items have no categorical effort field.

### Embeddings

`POST /v1/embeddings` requires the `embeddings` capability. OpenAI-compatible routes support provider-compatible embeddings endpoints. Native Vertex text embeddings require an explicitly supported Google embedding model and text input.

The Vertex mapper rejects unsupported token arrays, nested arrays, non-string values, empty input, multimodal payloads, and `encoding_format: "base64"` before provider execution. See [Provider API Compatibility](../reference/provider-api-compatibility.md) for the current model list.

### Images

`POST /v1/images` and `POST /v1/images/generations` generate images through the OpenAI Images API and require the `images` capability. A request needs `model` and `prompt`. Other fields, such as `n`, `size`, `quality`, `aspect_ratio`, or `input_references`, pass to the provider unchanged. The gateway sets `model` to the route's upstream model and sends the request to `{base_url}/images`.

The response is the provider's body: `created`, `data[]` entries with `b64_json` and `media_type`, and `usage`. It has no `model` field. If a provider adds one, the gateway replaces it with the gateway model name, so the upstream name never reaches the caller.

Image generation does not stream. A request with `stream: true` gets 400 before model resolution, with no request log. Prompt guardrails inspect the request. Model-response guardrails do not run on generated images. Image requests still go through gateway authentication, model grants, rate limits, budgets, request logs, provider attempts, and usage accounting. The `inference` API key scope covers them. Image requests record their own request metrics instead of the chat request metrics; see [Images Request Metrics](../operations/observability-and-request-logs.md#images-request-metrics) for what they record.

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
