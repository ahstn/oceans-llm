# ADR: LiteLLM Model Group Info for omp Discovery

- Date: 2026-10-21
- Status: Accepted
- Issue: [#568](https://github.com/ahstn/oceans-llm/issues/568)

## Current state

- [Model Routing and API Behavior: LiteLLM model metadata](../configuration/model-routing-and-api-behavior.md#litellm-model-metadata-get-model_groupinfo)
- [Identity and Access: API key scopes](../access/identity-and-access.md#api-key-scopes)
- [ADR: Enriched model list and client hints](2026-09-24-models-list-client-hints.md), which this ADR supersedes for omp

## Context

omp (Oh My Pi) with `discovery.type: proxy` reads only `id`, `name`, `supported_endpoint_types`, and `context_length` from `GET /v1/models`. With omp 18.12.5, we saw these gaps on the live Oceans payload:

- GPT models got no reasoning effort. omp sets `supportsReasoningEffort: false` for every OpenAI wire in proxy mode, so it sent no effort for any level that the user selected.
- Every Oceans model cost $0 in omp, so budget and usage views were wrong.
- Claude models got the generic effort list, `minimal` to `xhigh`. That list includes `minimal` and omits `max`, so it does not match these models.
- omp does not map `openai-response`. A Responses-only model used the provider `api`, which is the wrong endpoint.
- omp ignored the fields that Oceans already sends, such as `capabilities.effort`, `client_hints`, `max_tokens`, and `architecture.input_modalities`.

omp also has `discovery.type: litellm`. That mode reads the LiteLLM proxy metadata endpoints. It gets per-token prices, limits, vision, tool, and reasoning flags, and it sends OpenAI-family models to `/v1/responses`.

## Decision

Serve `GET /model_group/info` in the LiteLLM format, and tell omp users to use `discovery.type: litellm`. Keep `/v1/models` unchanged for Claude Code and proxy-style clients.

### 1. Route and access

The route is `GET /model_group/info`, beside `/v1/models` and outside the inference router. LiteLLM serves it at the root, and omp removes `/v1` from `baseUrl` before it calls it.

The handler uses `authenticate_for_scope` with the `inference` scope, the same as `/v1/models`. A missing key gets `401`. A key without the scope gets `403` with code `insufficient_scope`.

### 2. Same models as `/v1/models`

`GatewayService::model_group_info_for_api_key` uses the same `ModelAccessService` visibility and `metadata_snapshot()` as `models_list_for_api_key`. The response therefore has exactly the models that `/v1/models` shows to the same key.

The eligibility, transport, thinking policy, ceiling, and summary calculation moved out of `list_models` into a shared helper. Both listings use it, so they cannot disagree.

### 3. Projection

`crates/gateway-service/src/model_metadata/litellm.rs` maps each model to one `ModelGroupInfo` row:

| Field | Rule |
| --- | --- |
| `model_group` | Gateway model key |
| `providers` | One lowercase vendor: the model family from the upstream model name, else its `vendor/` prefix, else the primary route's provider type, else `oceans-llm`. `openai` only when every eligible chat route serves Responses |
| `mode` | `chat`, or `embedding` when the model serves only embeddings |
| `max_input_tokens` | Input limit, else the context window |
| `max_output_tokens` | Output limit |
| Four cost fields | Primary route price in USD per million tokens, divided by 1,000,000. All four or none |
| `supports_function_calling` | `tools` capability |
| `supports_vision` | `vision` capability, always `true` or `false` |
| `supports_reasoning` | Catalog reasoning flag or thinking policy, not limited to Anthropic Messages routes |
| `supports_response_schema` | Structured output capability |
| `supported_openai_params` | Fixed base list, plus `tools`, `tool_choice`, `reasoning_effort`, `response_format`, and `temperature` when the model supports them |

The `providers` rule follows the way omp selects an API. omp sends every `providers: ["openai"]` model to `/v1/responses`. Route selection can use any eligible route, so one route without Responses would make some requests fail. A GPT model behind such a route gets the primary route's provider type instead, and omp sends it to chat completions.

omp calls more LiteLLM endpoints when a row has only some costs or no `supports_vision` value. The projection therefore always sets `supports_vision` and sends the four costs together.

## Why

The mock payload test with omp 18.12.5 showed the result of this change:

| | Claude Opus 5.5 | GPT-6.1 Sol |
| --- | --- | --- |
| Endpoint | `/v1/chat/completions` | `/v1/responses` |
| Effort sent | `reasoning_effort: high` | `reasoning: {effort: high, summary: auto}` |
| Effort levels | `low` to `max` | `low` to `max` |
| Cost, USD per million tokens | 4 / 20 / 0.2 / 5 | 2 / 10 / 0.1 / 2.5 |
| Context | 1,000,000 | 922,000 |

GPT models now get reasoning effort, prices are correct, and Claude models get the correct effort list. A new field on `/v1/models` could not fix these gaps, because omp proxy mode does not read more fields. A change to omp proxy mode is an upstream change that Oceans does not control. LiteLLM metadata is a format that omp already reads.

## Trade-offs

- **Claude uses chat completions.** No LiteLLM field selects Anthropic Messages, so omp sends Claude models to `/v1/chat/completions`. The gateway translates these requests. Users who need `/v1/messages` set `modelOverrides.<id>.api: anthropic-messages` in omp.
- **Aliases.** omp gets effort levels and display names from its own catalog, by model ID. Aliases such as `claude-sonnet-latest` and `gpt-sol-latest` do not match it. They get the generic `minimal` to `xhigh` list, and the display name is the ID. The Oceans reasoning ceiling cannot be sent in this format. Users can set `modelOverrides.<alias>.thinking` in omp.
- **Tiered prices.** The row has one price per token type. omp ignores tiered prices, for example the higher GPT price above 272k tokens.
- **Decision models.** `mode` has no value that excludes decision models, so omp lists them as chat models.
- **One endpoint.** The gateway serves only `/model_group/info`. omp does not need `/model/info`, `/v1/model/info`, `/v2/model/info`, or `/v1/model_group/info`.
- **Base URL.** The omp provider `baseUrl` must end in `/v1`. Without it, omp calls the unversioned chat routes.

## Follow-ups

- Propose an omp change so `mapLiteLLMRichEntry` reads an effort list from the metadata row. Aliases and ceilings would then get the correct levels without `modelOverrides`.
- Add `/model/info` when a named client needs it.
