# Provider API Compatibility

`See also`: [Configuration Reference](../configuration/configuration-reference.md), [Model Routing and APIs](../configuration/model-routing-and-api-behavior.md), [Request Lifecycle and Failure Modes](request-lifecycle-and-failure-modes.md), [Pricing Catalog and Accounting](../configuration/pricing-catalog-and-accounting.md), [Observability and Request Logs](../operations/observability-and-request-logs.md), [ADR: Route-Level Provider API Compatibility Profiles](../adr/2026-04-23-route-level-provider-api-compatibility-profiles.md), [OpenAI Responses API Family Boundary](../adr/2026-04-23-openai-responses-api-family-boundary.md)

This page describes the live compatibility contract between the gateway's public OpenAI-shaped API and provider-specific upstream APIs.

## Current Public Surface

The gateway currently exposes:

- `GET /v1/models`
- `POST /v1/chat/completions`
- `POST /v1/messages`
- `POST /messages`
- `POST /v1/responses`
- `POST /v1/embeddings`
- `POST /api/v1/batches`

The Responses API is a first-class API family. It is not translated through Chat Completions.

## API-Family Matrix

| API family | Current gateway status | Adapter path | Compatibility policy |
| --- | --- | --- | --- |
| OpenAI Chat Completions | Supported for `openai_compat` providers | `crates/gateway-providers/src/openai_compat.rs` | Route-level `openai_compat` profile can declare request-shape quirks and streaming usage support. |
| OpenAI Responses API | Supported for `openai_compat` providers | `crates/gateway-providers/src/openai_compat.rs` | Uses a distinct typed request/core/provider boundary and preserves Responses event-stream semantics. |
| OpenAI Embeddings | Supported for `openai_compat` providers and native Vertex text-embedding routes | `crates/gateway-providers/src/openai_compat.rs`, `crates/gateway-providers/src/vertex/embeddings.rs` | OpenAI-compatible providers receive the OpenAI-shaped request. Vertex text embeddings use a provider-specific `:predict` mapper with explicit local validation. |
| Anthropic Messages | Supported for `/v1/messages` and `/messages` through the chat execution boundary | `crates/gateway/src/http/handlers.rs`, `crates/gateway-providers/src/anthropic_compat.rs`, `crates/gateway-providers/src/vertex/anthropic_request.rs` | Accepts Anthropic Messages request shape and returns Anthropic Messages response/SSE for chat-capable routes such as Anthropic-on-Vertex and `anthropic_compat` providers. |
| Durable batches | Supported for providers with a configured batch adapter | `crates/gateway/src/http/batches.rs` and provider-specific batch adapters | The outer `endpoint` selects Chat Completions, Responses, or Embeddings. Chat and Responses item bodies use the same model effort policy as synchronous requests and are validated before persistence. |
| Google Generative AI | Not implemented as a direct API-key provider path | Follow-up issue | Vertex Google transport exists; direct Google native API needs separate auth, request, and stream mapping. |
| Cross-provider multimodal files/images | Partial, provider-dependent | Follow-up issue | Needs explicit request body and accounting semantics across OpenAI-compatible, Vertex Google, Anthropic, and Google native APIs. |

## Provider Type Endpoint Matrix

This matrix is about current execution support, not provider marketing claims.

| Provider type | `/v1/chat/completions` | `/v1/responses` | `/v1/embeddings` |
| --- | --- | --- | --- |
| `openai_compat` | Supported. Chat Completions route profiles can rewrite known request-shape quirks. | Supported through the distinct Responses request/provider path. Chat Completions profile transforms do not apply. | Supported. No route compatibility transforms apply in this slice. |
| `gcp_cloud_run_openai_compat` | Supported through the OpenAI-compatible adapter with Cloud Run ID-token auth. | Supported when the deployed service exposes an OpenAI-compatible Responses endpoint. Chat Completions profile transforms do not apply. | Supported when the deployed service exposes an OpenAI-compatible embeddings endpoint. |
| `anthropic_compat` | Supported. Translates Chat Completions to native Anthropic `/v1/messages`, handling JSON and SSE streaming, tool calls, and thinking. | Not implemented; keep route `responses: false`. | Not implemented; keep route `embeddings: false`. |
| `gcp_vertex` with `google/*` upstream models | Supported for the current Vertex chat path when route capabilities allow it. Tested Gemini models support function tools. The gateway maps remote `gs://` and public HTTPS image, video, and generic-file inputs to Vertex `fileData`; signed URL queries pass to Vertex unchanged and are sanitized in retained request logs. This PR's deterministic tests cover gateway serialization and response normalization, not a live Vertex remote-media request. | Not implemented; keep route `responses: false`. | Supported only for explicit text-embedding routes using `google/gemini-embedding-001`, `google/gemini-embedding-2`, `google/text-embedding-005`, or `google/text-multilingual-embedding-002` with `embeddings: true`. Google chat and multimodal routes should keep `embeddings: false`. |
| `gcp_vertex` with `anthropic/*` upstream models | Supported for Chat Completions and Anthropic Messages when route capabilities allow it. Tool use is supported for text/tool workflows. | Not implemented; keep route `responses: false`. | Not applicable. |
| `aws_bedrock` | Supported through explicit `compatibility.aws_bedrock.api_style`: Runtime Converse, Runtime Anthropic InvokeModel, Runtime OpenAI Chat, Mantle OpenAI Chat, or Mantle Anthropic Messages. Streaming uses the configured style's stream contract. | Supported for `api_style: mantle_openai_responses` with an OpenAI base path such as `/openai/v1`. This is the Bedrock-supported Responses subset, not full direct-OpenAI hosted-tool parity. | Not implemented; keep route `embeddings: false`. |

Route capability flags are still useful when a provider implementation does not support a public API family. They make failures happen at the gateway edge instead of later inside the provider adapter.

## Route Compatibility Metadata

Provider compatibility is route metadata, not provider metadata.

Rationale:

- one provider endpoint can front several upstream model families
- two routes to the same provider can need different transforms
- compatibility transforms must travel with the selected route and be visible in config, storage, and tests

Route compatibility is persisted in `model_routes.compatibility_json` and seeded from config under:

```yaml
models:
  - id: fast
    routes:
      - provider: openrouter
        upstream_model: openai/gpt-4o-mini
        compatibility:
          openai_compat:
            supports_store: false
            max_tokens_field: max_tokens
            developer_role: system
            reasoning_effort: omit
            supports_stream_usage: true
            empty_tools: omit
```

Bedrock routes use an explicit AWS Bedrock profile:

```yaml
models:
  - id: gpt-55-bedrock
    routes:
      - provider: bedrock-mantle-openai
        upstream_model: openai.gpt-5.5
        capabilities:
          chat_completions: false
          responses: true
          stream: true
          embeddings: false
          json_schema: true
        extra_headers:
          OpenAI-Project: proj_123
        compatibility:
          aws_bedrock:
            api_style: mantle_openai_responses
            openai_base_path: /openai/v1
```

`api_style` values are `runtime_converse`, `runtime_anthropic_invoke`, `runtime_openai_chat`, `mantle_openai_responses`, `mantle_openai_chat`, and `mantle_anthropic_messages`. OpenAI-shaped styles require `openai_base_path`. Only `mantle_openai_responses` routes can enable `responses` and `json_schema`; those routes must disable `chat_completions`.

Runtime Converse compatibility also accepts optional `supports_strict_tools`. When absent, transparent model IDs use model-family detection; set it explicitly for opaque application-inference-profile IDs or ARNs so Claude Opus 4.7/4.8 routes omit the unsupported `strict` field while supported models retain it.

## Effective Capabilities

Effective capability is the intersection of configured route metadata and provider runtime support.

- Route `capabilities` declares what the route should be allowed to attempt.
- Provider implementations still enforce what they can actually execute.
- Capability defaults are permissive, so routes for partial providers should set unsupported API families to `false`.

For example, a Vertex Google chat route should normally set `responses: false` and `embeddings: false`. A separate Vertex text-embedding route can set `embeddings: true` only when its `upstream_model` is one of the supported Google embedding models. Otherwise the route may look viable from config alone and still fail when provider capability checks reject the unsupported API family or model.

For Cloud Run OpenAI-compatible routes, set capabilities to the endpoints exposed by the deployed service. The gateway adapter can construct Chat Completions, Responses, embeddings, and streams through the OpenAI-compatible path, but a vLLM deployment might only enable some of those endpoints.

### Hosted Responses Tools

Route `capabilities.tools` is a coarse gateway gate. It means a route can attempt tool-bearing requests at all; it does not claim support for every OpenAI Responses hosted tool type.

Hosted tools such as `image_generation` are provider- and deployment-specific. Function, custom, namespace, tool-search, and MCP workflows can be available on a route even when an OpenAI-hosted tool is not.

For `aws_bedrock` routes using `api_style: mantle_openai_responses`, Bedrock exposes an OpenAI-compatible Responses subset. Bedrock Mantle GPT-5.5 does not support the OpenAI-hosted `image_generation` tool even though direct OpenAI GPT-5.5 can. Oceans strips opportunistic `image_generation` tool declarations for ordinary Bedrock-backed coding workflows and fails locally when a request explicitly requires image generation, so callers receive a deterministic gateway 400 instead of an upstream Bedrock validation error.

If Oceans later routes between providers based on hosted-tool support, that should become an explicit capability dimension instead of overloading `tools`.

## Cloud Run OpenAI-Compatible Auth

`gcp_cloud_run_openai_compat` is an auth-specific provider boundary around the OpenAI-compatible adapter.

- `auth.mode: adc` mints Cloud Run ID tokens from metadata-server credentials on Google Cloud, or from service-account ADC files with a signed JWT assertion that includes `target_audience`.
- `auth.mode: service_account` reads a mounted service-account JSON file and exchanges a signed JWT assertion for a Google-issued ID token whose audience is the Cloud Run service.
- `auth.mode: bearer` sends static bearer material and does not refresh; reserve it for constrained debugging.
- `audience` defaults to the HTTPS service origin from `base_url`, and can be set explicitly for Cloud Run custom audiences.
- `auth_header` defaults to `authorization`; set `x_serverless_authorization` when Cloud Run should consume the ID token without replacing application-level `Authorization`.

The request/response body contract remains OpenAI-compatible. vLLM/Gemma fields such as `chat_template_kwargs.enable_thinking` and `skip_special_tokens` belong in route `extra_body`.

## Google Vertex Anthropic Claude

Vertex-hosted Claude models are selected when the Vertex `upstream_model` starts with `anthropic/`. The model ID after the slash is used in the Vertex endpoint path:

- non-streaming Chat Completions use `rawPredict`
- streaming Chat Completions use `streamRawPredict`
- `model` is not forwarded in the JSON body
- `anthropic_version: "vertex-2023-10-16"` is included in the JSON body

Anthropic-on-Vertex uses the Anthropic Messages body shape, so the gateway applies the same Claude request policy used for native Anthropic-style routes while preserving Vertex transport rules.

Claude thinking compatibility is model-aware:

| Model family | Gateway behavior |
| --- | --- |
| Claude Fable 5.1 | Adaptive thinking is always on. Request-level `reasoning_effort`, `reasoning.effort`, or `output_config.effort` maps to adaptive thinking plus `output_config.effort`. Provider support for beta per-message effort is separate from request-level mapping; see [Google Vertex AI](../providers/gcp-vertex.md#per-message-effort). |
| Claude Opus 4.7 and later | `reasoning_effort` or `reasoning.effort` maps to `thinking: { "type": "adaptive" }` plus `output_config.effort`; manual `thinking.type: "enabled"` and `budget_tokens` are rejected. Non-default `temperature`, `top_p`, and `top_k` are rejected; default `temperature: 1` and `top_p: 1` are omitted. |
| Claude Opus 4.6 and Claude Sonnet 4.6 | `reasoning_effort` maps to adaptive thinking and `output_config.effort`. Caller-supplied manual budgets remain pass-through because Anthropic still accepts them, though they are deprecated upstream. |
| Claude Mythos Preview | `reasoning_effort` maps to `output_config.effort`; `thinking.type: "disabled"` is rejected. |
| Claude Opus 4.5 | Adaptive thinking is rejected. `reasoning_effort` maps to `output_config.effort` only when the request also includes a manual thinking budget. |
| Claude Sonnet/Haiku 4.5 and older Claude models | Adaptive thinking is rejected. These models require an explicit manual budget from `reasoning.budget_tokens`, `reasoning_budget_tokens`, `thinking_budget_tokens`, or caller-supplied `thinking.type: "enabled"` with `budget_tokens`; the gateway does not add `output_config.effort`. |

Provider-specific Anthropic fields remain available where they do not conflict with normalized compatibility behavior. If `reasoning_effort` disagrees with `reasoning.effort`, `output_config.effort`, caller-supplied `thinking`, or a manual budget, the request fails locally with a deterministic gateway error.

Model reasoning-effort ceilings are enforced before provider mapping. Every explicit effort field must satisfy the effective alias-chain policy even when two fields agree; compatibility transforms do not clamp or bypass the ceiling. See [Model Routing and APIs](../configuration/model-routing-and-api-behavior.md#enforce-reasoning-effort-ceilings) for the accepted order and API-specific paths.

Chat Completions response policy matches the Bedrock Claude policy. Native Anthropic `thinking` and `redacted_thinking` blocks are never concatenated into `choices[*].message.content`. Streaming `thinking_delta` and `signature_delta` events are never emitted as `delta.content`. The gateway preserves these blocks under `choices[*].message.provider_metadata.gcp_vertex.reasoning` and `choices[*].delta.provider_metadata.gcp_vertex.reasoning`.

Provider metadata preservation is not yet request-side replay. The current Vertex Anthropic mapper does not rehydrate preserved `thinking`, `signature`, or `redacted_thinking` blocks into later assistant content when callers send tool results. Tool-use continuations that require exact thinking block round-trip are tracked by [issue #140](https://github.com/ahstn/oceans-llm/issues/140).

Vertex Claude route capabilities should stay aligned with tested gateway behavior, not only upstream model capability. Function tools, tool-result continuations, and streaming tool-use deltas are supported for Anthropic-on-Vertex text/tool workflows. Image/document content blocks remain unsupported in the current mapper and should keep `vision: false`; broader multimodal matrices remain tracked by [issue #91](https://github.com/ahstn/oceans-llm/issues/91) and [issue #93](https://github.com/ahstn/oceans-llm/issues/93).

Vertex Google publisher routes remain separate from Anthropic-on-Vertex. `google/*` upstream models use Vertex `generateContent` and `streamGenerateContent`; Anthropic Messages fields such as `thinking`, `output_config`, and `anthropic_version` do not apply to those routes.

## Google Vertex Text Embeddings

Native Vertex text embeddings are available through the public OpenAI-compatible `POST /v1/embeddings` endpoint for these Google publisher models:

- `google/gemini-embedding-001`
- `google/gemini-embedding-2`
- `google/text-embedding-005`
- `google/text-multilingual-embedding-002`

The route should be embedding-only unless you have separately tested another API family for the same upstream model:

```yaml
capabilities:
  chat_completions: false
  responses: false
  embeddings: true
  stream: false
  tools: false
  vision: false
  json_schema: false
```

Compatibility contract:

| Public field or behavior | Vertex mapping or outcome |
| --- | --- |
| `input: "text"` | One Vertex text embedding request and one OpenAI-compatible `data[0]` embedding. `google/gemini-embedding-2` uses Vertex `:embedContent`; the other supported models use Vertex `:predict`. |
| `input: ["a", "b"]` | Independent embedding operations with OpenAI-compatible indexes preserved in request order. |
| Token arrays, nested arrays, non-string values, empty arrays, empty strings, multimodal/base64 payloads | Rejected locally with `invalid_request`. |
| `dimensions` | `parameters.outputDimensionality` for `:predict` models; `embedContentConfig.outputDimensionality` for `google/gemini-embedding-2`; must be positive and within the model's supported maximum. |
| `output_dimensionality` / `outputDimensionality` | Aliases for `dimensions`; conflicts are rejected. |
| `encoding_format` omitted or `float` | Accepted; response embeddings are float vectors. |
| `encoding_format: "base64"` | Rejected locally. |
| `task_type` | `instances[].task_type` for `:predict` models; rejected for `google/gemini-embedding-2`, which expects task instructions in the input text. |
| `input_type` | Alias for `task_type` on `:predict` models; conflicts are rejected. Rejected for `google/gemini-embedding-2`. |
| `title` | Accepted only for retrieval-document embeddings on `:predict` models. Rejected for `google/gemini-embedding-2`. |
| `auto_truncate` / `autoTruncate` | `parameters.autoTruncate` for `:predict` models; conflicts are rejected. Rejected for `google/gemini-embedding-2`. |

Responses are normalized to the OpenAI embeddings list shape: `object: "list"`, ordered `data[]`, `data[].object: "embedding"`, `data[].index`, float vectors, `model`, and `usage` when Vertex returns real token counts. Missing token counts become `usage_missing`; exact catalog misses become `unpriced`. Neither state consumes budget.

Anthropic-on-Vertex routes and Google chat/multimodal models do not implement embeddings.


## AWS Bedrock Runtime Anthropic Claude

For Bedrock, this foundation guarantees config load, validation, seeding, registration, deterministic region, endpoint kind, timeout, display, auth metadata, explicit Runtime and Mantle API style selection, and request/stream normalization for supported route styles. It also supports IAM/SigV4 signing for the `default_chain` and `static_credentials` auth modes. Runtime providers sign with service `bedrock`; Mantle providers sign with service `bedrock-mantle`. Bedrock `upstream_model` values should match the model identity accepted by the configured endpoint and API style.

Bedrock Runtime Anthropic `InvokeModel` is selected by `compatibility.aws_bedrock.api_style: runtime_anthropic_invoke`. Non-streaming Chat Completions for those routes use Bedrock Runtime `InvokeModel` (`/model/{modelId}/invoke`) with Anthropic's native Messages body instead of the generic Converse body.

The native Bedrock Anthropic body always includes:

- `anthropic_version: bedrock-2023-05-31`
- combined `system` and `developer` text as Anthropic `system`
- `messages` with Anthropic `text`, `image`, `tool_use`, and `tool_result` content blocks
- `max_tokens` from `max_tokens` or `max_completion_tokens`
- `temperature`, `top_p`, `top_k`, and `stop_sequences`, subject to Claude Opus 4.7+ sampling restrictions
- function tools as Anthropic custom tools with `input_schema`
- `tool_choice` mapped from OpenAI `auto`, `required`, and named function choices

The implementation rejects missing `max_tokens` for native Claude invocation because Bedrock marks it required. It also rejects OpenAI-only controls such as penalties, `n`, `seed`, `parallel_tool_calls`, and `response_format`. JSON schema mode should stay disabled in route capabilities unless a route explicitly uses Bedrock/Anthropic-specific `output_config` through provider overrides and accepts the non-OpenAI contract.

Claude thinking compatibility is model-aware:

| Model family | Gateway behavior |
| --- | --- |
| Claude Opus 4.7 and later | `reasoning_effort` or `reasoning.effort` maps to `thinking: { "type": "adaptive" }` plus `output_config.effort`; manual `thinking.type: "enabled"` and `budget_tokens` are rejected. Non-default `temperature`, `top_p`, and `top_k` are rejected; default `temperature: 1` and `top_p: 1` are omitted. |
| Claude Opus 4.6 and Claude Sonnet 4.6 | `reasoning_effort` or `reasoning.effort` maps to adaptive thinking and `output_config.effort`. Caller-supplied manual `thinking.type: "enabled"` with `budget_tokens` remains pass-through because Anthropic still accepts it. |
| Claude Mythos Preview | `reasoning_effort` maps to adaptive thinking and `output_config.effort`; `thinking.type: "disabled"` is rejected. |
| Claude Opus 4.5 | Adaptive thinking is rejected. `reasoning_effort` maps to Bedrock's beta `output_config.effort` for native Messages invocation and adds `anthropic_beta: ["effort-2025-11-24"]`. If a manual budget is also supplied through `reasoning.budget_tokens`, `reasoning_budget_tokens`, `thinking_budget_tokens`, or caller-supplied `thinking.type: "enabled"` with `budget_tokens`, the gateway sends manual `thinking.type: "enabled"` as well. |
| Claude Sonnet/Haiku 4.5 and older Claude models | Adaptive thinking is rejected. These models do not receive `output_config.effort`; they require an explicit manual budget from `reasoning.budget_tokens`, `reasoning_budget_tokens`, `thinking_budget_tokens`, or caller-supplied `thinking.type: "enabled"` with `budget_tokens`, and the gateway then sends manual `thinking.type: "enabled"`. |

Provider-specific fields remain available where they do not conflict with normalized compatibility behavior. `anthropic_beta`, `context_management`, `container`, and `metadata` are copied through. `thinking` and `output_config` are copied through first, then normalized OpenAI-shaped reasoning fields are applied. If `reasoning_effort` disagrees with `reasoning.effort`, `output_config.effort`, caller-supplied `thinking`, or a manual budget, the request fails locally with a deterministic gateway error instead of leaking incompatible OpenAI-only fields upstream. Route `extra_body` is still a final raw override for admin-controlled experiments.

For Bedrock Converse and ConverseStream, Claude thinking controls are written to `additionalModelRequestFields.thinking`. Adaptive models receive `type: "adaptive"` and `effort` inside that object. Manual-budget models receive `type: "enabled"` and `budget_tokens` inside that object. Existing unrelated `additionalModelRequestFields` keys are preserved, while conflicting `thinking` values are rejected locally.

Vision is supported only for Bedrock-compatible base64 image payloads. Remote image URLs are rejected because Bedrock Anthropic Messages requires base64 image sources. Tools and tool-result turns are supported for Claude 3+ models, subject to the model's Bedrock feature availability.

Converse tool results accept text, JSON, base64 images, and Bedrock document formats (`pdf`, `csv`, `doc`, `docx`, `xls`, `xlsx`, `html`, `md`, and `txt`). The same image/document conversion is used for ordinary user content. Document names are stripped of extensions and normalized to Bedrock's allowed characters. Claude Opus 4.7 and 4.8 tool specifications omit `strict`, which those Bedrock models reject; other models retain an explicit `strict` value.

Request-scoped Converse and ConverseStream requests may include `requestMetadata`, `performanceConfig`, `guardrailConfig`, and `additionalModelResponseFieldPaths` (or their snake_case aliases). Oceans validates AWS limits, enum values, guardrail shapes, and RFC 6901 response paths before dispatch. `streamProcessingMode` is accepted only for ConverseStream. Route `extra_body` is merged afterward and remains the final admin-controlled override.

Chat Completions response policy for Anthropic thinking is deliberately conservative. Native Anthropic `thinking` and `redacted_thinking` blocks, plus Bedrock Converse `reasoningContent` text, signatures, and redacted data, are never concatenated into `choices[*].message.content` or streamed as `delta.content`. The visible Chat Completions content remains answer text and tool calls only. Reasoning state that providers require for debugging or tool-use continuity is preserved under `choices[*].message.provider_metadata.aws_bedrock.reasoning` for non-streaming responses, and under `choices[*].delta.provider_metadata.aws_bedrock.reasoning` for ConverseStream chunks. The public Anthropic Messages route keeps the same non-leaking split when it uses chat-backed provider execution.

Anthropic documents that Claude 4 models can return summarized thinking, encrypted signatures, and `redacted_thinking` blocks. Claude Opus 4.7 defaults thinking display to `omitted`, so a stream can open an empty thinking block, emit only a signature delta, and then begin normal text. Bedrock Converse represents equivalent state as `reasoningContent`, including `reasoningText.text`, `reasoningText.signature`, and redacted content. The gateway preserves those fields as provider metadata and treats billed output token counts as provider usage until exact reasoning accounting is implemented.

Provider metadata preservation is not yet request-side replay. The current Bedrock Anthropic mappers do not rehydrate preserved `thinking`, `signature`, or `redacted_thinking` blocks into later assistant content when callers send tool results. Tool-use continuations that require exact thinking block round-trip are tracked by [issue #140](https://github.com/ahstn/oceans-llm/issues/140). Exact cache, reasoning, and modality token accounting remains tracked by [issue #92](https://github.com/ahstn/oceans-llm/issues/92).

Streaming boundary: Runtime Anthropic `InvokeModel` does not provide the route's streaming path. Configure `api_style: runtime_converse` for Bedrock Runtime ConverseStream, or `api_style: mantle_anthropic_messages` for Mantle Anthropic Messages SSE.

## OpenAI-Compatible Profile Fields

These profile transforms apply to Chat Completions request-shape quirks unless explicitly stated. Responses requests use the same route/provider selection path, but they are not patched with Chat Completions compatibility shims such as `stream_options.include_usage`.

`openai_compat.supports_store`

- default: `true`
- when `false`, outbound Chat Completions requests remove `store`

`openai_compat.max_tokens_field`

- default: `max_completion_tokens`
- `max_tokens` rewrites `max_completion_tokens` to `max_tokens`

`openai_compat.developer_role`

- default: `developer`
- `system` rewrites outbound `developer` messages to `system`

`openai_compat.reasoning_effort`

- default: `passthrough`
- `omit` removes `reasoning_effort`
- `reasoning_object` rewrites `reasoning_effort: "high"` to `reasoning: { "effort": "high" }`

`openai_compat.supports_stream_usage`

- default: `false`
- when `true`, streaming Chat Completions requests include `stream_options.include_usage = true`

`openai_compat.empty_tools`

- default: `preserve`
- `omit` removes an explicit empty `tools` array from both Chat Completions and Responses requests; neutral `tool_choice` values (`auto`, `none`, or `null`) are removed with it, while `required` and named choices are rejected locally
- `preserve_with_tool_history` removes an empty array unless function-tool history is present, for LiteLLM/Anthropic proxy compatibility; preserved arrays retain `tool_choice`

## OpenRouter Routing Policy

OpenRouter is configured as `type: openai_compat` for transport, but OpenRouter provider-selection policy is a separate route-level compatibility block:

```yaml
compatibility:
  openrouter:
    provider:
      zdr: true
      only: [openai, anthropic]
      ignore: [deepinfra]
      order: [openai, anthropic]
      preferred_max_latency:
        p90: 2.5
      max_price:
        prompt: 1.0
        completion: 2.0
```

The gateway serializes that block into the upstream Chat Completions request body as `provider`. For example:

```json
{
  "provider": {
    "zdr": true,
    "only": ["openai"],
    "ignore": ["deepinfra"],
    "order": ["openai", "anthropic"],
    "preferred_max_latency": { "p90": 2.5 },
    "max_price": {
      "prompt": 1.0,
      "completion": 2.0
    }
  }
}
```

`zdr` restricts OpenRouter routing to zero-data-retention endpoints. `preferred_max_latency` is preference-shaped and supports a number or `p50`, `p75`, `p90`, and `p99` percentile cutoffs in seconds. `max_price` is a hard ceiling and supports `prompt`, `completion`, `request`, and `image` dimensions. Provider slug validation remains OpenRouter's responsibility; Oceans validates shape, empty values, duplicate slugs, contradictory `only`/`ignore` entries, and raw `extra_body.provider` conflicts.

This policy is OpenRouter upstream behavior. It does not add Oceans-side multi-route fallback.

## Stream Normalization

The Chat Completions stream adapter keeps the SSE transcript OpenAI-shaped while normalizing common provider variants:

- appends one final `data: [DONE]` only after an upstream `[DONE]` marker or a chunk with a non-empty string `finish_reason`
- promotes `choices[*].usage` to top-level `usage` when top-level usage is absent
- preserves final usage-only chunks
- maps `delta.reasoning_content` and `delta.reasoning_text` into `delta.reasoning` when no canonical reasoning field exists
- emits structured SSE error chunks for malformed streams, top-level errors, or EOF before terminal semantics, and never appends `[DONE]` after failure

This is intentionally narrower than full tool-call streaming normalization. Tool-call streaming needs a richer gateway event model and is tracked separately.

The Responses stream adapter is separate. It parses SSE frames for transport safety, preserves `event: response.*` names and JSON payloads, accepts `response.completed` and `response.incomplete` as successful terminal states, and surfaces `response.failed`, top-level errors, conflicting status/type fields, Chat Completions protocol mismatches, and premature EOF without synthesizing `[DONE]`. One final `data: [DONE]` is appended only after a valid completed or incomplete terminal event.

Bedrock chat streaming is a separate transport adapter because Bedrock Runtime does not return SSE for ConverseStream. It decodes AWS Smithy/EventStream frames, reads string headers such as `:message-type`, `:event-type`, and `:exception-type`, and normalizes ConverseStream events into Chat Completions SSE chunks. `messageStart` emits the assistant role, `contentBlockDelta` emits text, function-tool argument deltas, or provider reasoning metadata deltas, `messageStop` emits the terminal finish reason, and `metadata.usage` emits an OpenAI-shaped usage chunk when present. EventStream exception frames and malformed or incomplete frames emit structured SSE error chunks and do not receive a final `[DONE]`.

The current Bedrock frame parser validates frame lengths, header boundaries, supported header encodings, JSON payload shape, and clean finalization. It recognizes the prelude CRC and message CRC fields but does not validate CRC checksums in this slice. Provider-native `InvokeModelWithResponseStream` mappings, including Anthropic-specific native streaming payloads, remain separate provider-family work tracked by [issue #139](https://github.com/ahstn/oceans-llm/issues/139).

## Cross-Provider Replay Identifiers

The gateway preserves provider-native IDs that already satisfy the target API. Invalid or overlong Bedrock tool-use/result IDs are replaced with the same deterministic, bounded `tool_<SHA-256>` value on both sides of the pair. OpenAI Responses replay preserves valid native `fc_`, `rs_`, and `msg_` item IDs, hashes foreign or invalid item IDs into the corresponding bounded namespace, and applies the same deterministic normalization to matching `function_call` and `function_call_output` `call_id` values. Missing optional Responses item IDs remain missing. This normalization is request-local and stateless; it does not claim that an ID belongs to a stored upstream response.

## Accounting Boundary

Compatibility profiles can make usage more likely to appear in a standard place, but they do not change accounting semantics.

Current durable accounting only relies on:

- `prompt_tokens`
- `completion_tokens`
- `total_tokens`

Responses usage is normalized from `usage.input_tokens`, `usage.output_tokens`, and `usage.total_tokens` into the gateway's prompt/completion/total accounting columns. Streaming Responses usage is read from completed response events with `response.usage`.

Vertex text embeddings normalize real provider token counts into prompt/input token usage: `predictions[].embeddings.statistics.token_count` for Vertex `:predict` text-embedding models and `usageMetadata.promptTokenCount` for `google/gemini-embedding-2`. The gateway does not infer tokens from character or byte counts. Provider-specific cache, reasoning, image, audio, and modality counters remain follow-up work. Until those semantics are explicit, successful requests may still become `usage_missing` or `unpriced`.

## Research References

The route-profile design follows the same broad lesson visible in mature adapter stacks: API-family differences are real interfaces, not provider-name strings.

- Vercel AI SDK keeps distinct provider packages for OpenAI, OpenAI-compatible, Anthropic, Google Generative AI, and Google Vertex under [`packages/`](https://github.com/vercel/ai/tree/main/packages).
- The OpenAI-compatible package exposes streaming usage as an explicit provider option rather than assuming every compatible server behaves the same.
- Mario Zechner's provider notes and `pi-mono` OpenAI completions adapter are useful examples of agent-facing compatibility pressure: [post](https://mariozechner.at/posts/2025-11-30-pi-coding-agent/) and [source](https://github.com/badlogic/pi-mono/blob/main/packages/ai/src/providers/openai-completions.ts).

## Follow-Up Scope

These items are intentionally outside this first slice:

- provider compatibility umbrella: [issue #53](https://github.com/ahstn/oceans-llm/issues/53)
- broader native Anthropic Messages parity beyond chat-backed Vertex Claude routing: [issue #89](https://github.com/ahstn/oceans-llm/issues/89)
- direct Google Generative AI provider/API-key path: [issue #90](https://github.com/ahstn/oceans-llm/issues/90)
- cross-provider tool-call streaming normalization fixtures: [issue #91](https://github.com/ahstn/oceans-llm/issues/91)
- cache, reasoning, and modality token accounting: [issue #92](https://github.com/ahstn/oceans-llm/issues/92)
- multimodal image/file compatibility across provider families: [issue #93](https://github.com/ahstn/oceans-llm/issues/93)
- Bedrock Runtime Anthropic streaming over `InvokeModelWithResponseStream`: [issue #139](https://github.com/ahstn/oceans-llm/issues/139)
- Anthropic thinking block replay for tool-use continuations: [issue #140](https://github.com/ahstn/oceans-llm/issues/140)
- Vertex Claude multimodal parity: [issue #141](https://github.com/ahstn/oceans-llm/issues/141)
- route readiness diagnostics: [issue #98](https://github.com/ahstn/oceans-llm/issues/98)
