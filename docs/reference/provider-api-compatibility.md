# Provider API Compatibility

`See also`: [Configuration Reference](../configuration/configuration-reference.md), [Model Routing and API Behavior](../configuration/model-routing-and-api-behavior.md), [Request Lifecycle and Failure Modes](request-lifecycle-and-failure-modes.md), [Pricing Catalog and Accounting](../configuration/pricing-catalog-and-accounting.md), [Observability and Request Logs](../operations/observability-and-request-logs.md), [ADR: Route-Level Provider API Compatibility Profiles](../adr/2026-04-23-route-level-provider-api-compatibility-profiles.md), [OpenAI Responses API Family Boundary](../adr/2026-04-23-openai-responses-api-family-boundary.md)

This page describes the live compatibility contract between the gateway's public OpenAI-shaped API and provider-specific upstream APIs.

## Current Public Surface

The gateway currently exposes:

- `GET /v1/models`
- `POST /v1/chat/completions`
- `POST /v1/responses`
- `POST /v1/embeddings`

The Responses API is a first-class API family. It is not translated through Chat Completions.

## API-Family Matrix

| API family | Current gateway status | Adapter path | Compatibility policy |
| --- | --- | --- | --- |
| OpenAI Chat Completions | Supported for `openai_compat` providers | `crates/gateway-providers/src/openai_compat.rs` | Route-level `openai_compat` profile can declare request-shape quirks and streaming usage support. |
| OpenAI Responses API | Supported for `openai_compat` providers | `crates/gateway-providers/src/openai_compat.rs` | Uses a distinct typed request/core/provider boundary and preserves Responses event-stream semantics. |
| OpenAI Embeddings | Supported for `openai_compat` providers | `crates/gateway-providers/src/openai_compat.rs` | Uses the same route/provider resolution path; no compatibility transforms are applied in this slice. |
| Anthropic Messages | Not implemented as a native public API | Follow-up issue | Vertex Anthropic transport exists, but native Messages semantics need explicit mapping and tests. |
| Google Generative AI | Not implemented as a direct API-key provider path | Follow-up issue | Vertex Google transport exists; direct Google native API needs separate auth, request, and stream mapping. |
| Cross-provider multimodal files/images | Partial, provider-dependent | Follow-up issue | Needs explicit request body and accounting semantics across OpenAI-compatible, Vertex Google, Anthropic, and Google native APIs. |

## Provider Type Endpoint Matrix

This matrix is about current execution support, not provider marketing claims.

| Provider type | `/v1/chat/completions` | `/v1/responses` | `/v1/embeddings` |
| --- | --- | --- | --- |
| `openai_compat` | Supported. Chat Completions route profiles can rewrite known request-shape quirks. | Supported through the distinct Responses request/provider path. Chat Completions profile transforms do not apply. | Supported. No route compatibility transforms apply in this slice. |
| `gcp_vertex` with `google/*` upstream models | Supported for the current Vertex chat path when route capabilities allow it. | Not implemented; keep route `responses: false`. | Not implemented in this slice; keep route `embeddings: false`. |
| `gcp_vertex` with `anthropic/*` upstream models | Supported for the current Vertex chat path when route capabilities allow it. | Not implemented; keep route `responses: false`. | Not applicable. |
| `aws_bedrock` | Supported for Bedrock Converse chat. Anthropic Claude upstream model IDs use Bedrock `InvokeModel` with the native Anthropic Messages payload for non-streaming Chat Completions. Streaming chat uses ConverseStream and normalizes AWS Smithy/EventStream frames into OpenAI-compatible SSE; it is not native Anthropic Messages streaming. | Not implemented; keep route `responses: false`. | Not implemented; keep route `embeddings: false`. |

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
```

## Effective Capabilities

Effective capability is the intersection of configured route metadata and provider runtime support.

- Route `capabilities` declares what the route should be allowed to attempt.
- Provider implementations still enforce what they can actually execute.
- Capability defaults are permissive, so routes for partial providers should set unsupported API families to `false`.

For example, a Vertex Google chat route should normally set `responses: false` and `embeddings: false` until those provider paths are implemented. Otherwise the route may look viable from config alone and still fail when the provider adapter rejects the unsupported API family.

For Bedrock, this foundation guarantees config load, validation, seeding, registration, deterministic region, endpoint, timeout, display, auth metadata, Converse chat execution, Claude Messages invocation, and ConverseStream chat streaming for bearer-token auth. It also supports IAM/SigV4 signing for the `default_chain` and `static_credentials` auth modes. Bedrock `upstream_model` values should match Bedrock Runtime model identity: base model IDs, inference profile IDs, or Bedrock ARNs accepted by the target Bedrock Runtime operation. AWS documents `InvokeModel` as requiring `bedrock:InvokeModel`; streamed invocation and Converse access require the corresponding Bedrock runtime permissions.

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
| Claude Opus 4.7 and later | `reasoning_effort` or `reasoning.effort` maps to `thinking: { "type": "adaptive" }` plus `output_config.effort`; manual `thinking.type: "enabled"` and `budget_tokens` are rejected. Non-default `temperature`, `top_p`, and `top_k` are rejected; default `temperature: 1` and `top_p: 1` are omitted. |
| Claude Opus 4.6 and Claude Sonnet 4.6 | `reasoning_effort` maps to adaptive thinking and `output_config.effort`. Caller-supplied manual budgets remain pass-through because Anthropic still accepts them, though they are deprecated upstream. |
| Claude Mythos Preview | `reasoning_effort` maps to `output_config.effort`; `thinking.type: "disabled"` is rejected. |
| Claude Opus 4.5 | Adaptive thinking is rejected. `reasoning_effort` maps to `output_config.effort` only when the request also includes a manual thinking budget. |
| Claude Sonnet/Haiku 4.5 and older Claude models | Adaptive thinking is rejected. These models require an explicit manual budget from `reasoning.budget_tokens`, `reasoning_budget_tokens`, `thinking_budget_tokens`, or caller-supplied `thinking.type: "enabled"` with `budget_tokens`; the gateway does not add `output_config.effort`. |

Provider-specific Anthropic fields remain available where they do not conflict with normalized compatibility behavior. If `reasoning_effort` disagrees with `reasoning.effort`, `output_config.effort`, caller-supplied `thinking`, or a manual budget, the request fails locally with a deterministic gateway error.

Chat Completions response policy matches the Bedrock Claude policy. Native Anthropic `thinking` and `redacted_thinking` blocks are never concatenated into `choices[*].message.content`. Streaming `thinking_delta` and `signature_delta` events are never emitted as `delta.content`. The gateway preserves these blocks under `choices[*].message.provider_metadata.gcp_vertex.reasoning` and `choices[*].delta.provider_metadata.gcp_vertex.reasoning`.

Provider metadata preservation is not yet request-side replay. The current Vertex Anthropic mapper does not rehydrate preserved `thinking`, `signature`, or `redacted_thinking` blocks into later assistant content when callers send tool results. Tool-use continuations that require exact thinking block round-trip are tracked by [issue #140](https://github.com/ahstn/oceans-llm/issues/140).

Vertex Claude route capabilities should stay aligned with tested gateway behavior, not only upstream model capability. Function tools, tool-result continuations, image/document content blocks, and related stream behavior for Anthropic-on-Vertex are tracked by [issue #141](https://github.com/ahstn/oceans-llm/issues/141). The broader cross-provider tool and multimodal matrices remain tracked by [issue #91](https://github.com/ahstn/oceans-llm/issues/91) and [issue #93](https://github.com/ahstn/oceans-llm/issues/93).

Vertex Google publisher routes remain separate from Anthropic-on-Vertex. `google/*` upstream models use Vertex `generateContent` and `streamGenerateContent`; Anthropic Messages fields such as `thinking`, `output_config`, and `anthropic_version` do not apply to those routes.

## AWS Bedrock Anthropic Claude

Bedrock-hosted Claude models are selected when the Bedrock `upstream_model` contains `anthropic.claude`, including regional inference profile IDs such as `us.anthropic.claude-3-5-sonnet-20241022-v2:0`. Non-streaming Chat Completions for those routes use Bedrock Runtime `InvokeModel` (`/model/{modelId}/invoke`) with Anthropic's native Messages body instead of the generic Converse body.

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

Provider-specific fields remain available where they do not conflict with normalized compatibility behavior. `anthropic_beta`, `context_management`, `container`, and `metadata` are copied through. `thinking` and `output_config` are copied through first, then normalized OpenAI-shaped reasoning fields are applied. If `reasoning_effort` disagrees with `reasoning.effort`, `output_config.effort`, caller-supplied `thinking`, or a manual budget, the request fails locally with a deterministic gateway error instead of leaking incompatible OpenAI-only fields upstream. Route `extra_body` is still a final raw override for operator-controlled experiments.

For Bedrock Converse and ConverseStream, Claude thinking controls are written to `additionalModelRequestFields.thinking`. Adaptive models receive `type: "adaptive"` and `effort` inside that object. Manual-budget models receive `type: "enabled"` and `budget_tokens` inside that object. Existing unrelated `additionalModelRequestFields` keys are preserved, while conflicting `thinking` values are rejected locally.

Vision is supported only for Bedrock-compatible base64 image payloads. Remote image URLs are rejected because Bedrock Anthropic Messages requires base64 image sources. Tools and tool-result turns are supported for Claude 3+ models, subject to the model's Bedrock feature availability.

Chat Completions response policy for Anthropic thinking is deliberately conservative. Native Anthropic `thinking` and `redacted_thinking` blocks, plus Bedrock Converse `reasoningContent` text, signatures, and redacted data, are never concatenated into `choices[*].message.content` or streamed as `delta.content`. The visible Chat Completions content remains answer text and tool calls only. Reasoning state that providers require for debugging or tool-use continuity is preserved under `choices[*].message.provider_metadata.aws_bedrock.reasoning` for non-streaming responses, and under `choices[*].delta.provider_metadata.aws_bedrock.reasoning` for ConverseStream chunks. Direct Anthropic Messages routes should follow the same split when added: hidden or summarized thinking metadata may be preserved explicitly, but it must not leak through ordinary Chat Completions text fields.

Anthropic documents that Claude 4 models can return summarized thinking, encrypted signatures, and `redacted_thinking` blocks. Claude Opus 4.7 defaults thinking display to `omitted`, so a stream can open an empty thinking block, emit only a signature delta, and then begin normal text. Bedrock Converse represents equivalent state as `reasoningContent`, including `reasoningText.text`, `reasoningText.signature`, and redacted content. The gateway preserves those fields as provider metadata and treats billed output token counts as provider usage until exact reasoning accounting is implemented.

Provider metadata preservation is not yet request-side replay. The current Bedrock Anthropic mappers do not rehydrate preserved `thinking`, `signature`, or `redacted_thinking` blocks into later assistant content when callers send tool results. Tool-use continuations that require exact thinking block round-trip are tracked by [issue #140](https://github.com/ahstn/oceans-llm/issues/140). Exact cache, reasoning, and modality token accounting remains tracked by [issue #92](https://github.com/ahstn/oceans-llm/issues/92).

Streaming boundary: native Anthropic Messages streaming over `InvokeModelWithResponseStream` is not implemented in this slice. If a Bedrock route enables streaming, it is using the generic Bedrock Converse stream adapter from [issue #128](https://github.com/ahstn/oceans-llm/issues/128), not the native Anthropic Messages stream event contract. Native Bedrock Anthropic Messages streaming is tracked by [issue #139](https://github.com/ahstn/oceans-llm/issues/139).

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

## Stream Normalization

The Chat Completions stream adapter keeps the SSE transcript OpenAI-shaped while normalizing common provider variants:

- appends one final `data: [DONE]` when the upstream omits it after valid payload events
- promotes `choices[*].usage` to top-level `usage` when top-level usage is absent
- preserves final usage-only chunks
- maps `delta.reasoning_content` and `delta.reasoning_text` into `delta.reasoning` when no canonical reasoning field exists
- emits structured SSE error chunks for malformed or incomplete streams instead of pretending the stream completed normally

This is intentionally narrower than full tool-call streaming normalization. Tool-call streaming needs a richer gateway event model and is tracked separately.

The Responses stream adapter is separate. It parses SSE frames for transport safety, preserves `event: response.*` names and JSON payloads, surfaces malformed or incomplete streams as structured SSE error chunks, and appends one final `data: [DONE]` only after a successful upstream stream that omitted it.

Bedrock chat streaming is a separate transport adapter because Bedrock Runtime does not return SSE for ConverseStream. It decodes AWS Smithy/EventStream frames, reads string headers such as `:message-type`, `:event-type`, and `:exception-type`, and normalizes ConverseStream events into Chat Completions SSE chunks. `messageStart` emits the assistant role, `contentBlockDelta` emits text, function-tool argument deltas, or provider reasoning metadata deltas, `messageStop` emits the terminal finish reason, and `metadata.usage` emits an OpenAI-shaped usage chunk when present. EventStream exception frames and malformed or incomplete frames emit structured SSE error chunks and do not receive a final `[DONE]`.

The current Bedrock frame parser validates frame lengths, header boundaries, supported header encodings, JSON payload shape, and clean finalization. It recognizes the prelude CRC and message CRC fields but does not validate CRC checksums in this slice. Provider-native `InvokeModelWithResponseStream` mappings, including Anthropic-specific native streaming payloads, remain separate provider-family work tracked by [issue #139](https://github.com/ahstn/oceans-llm/issues/139).

## Accounting Boundary

Compatibility profiles can make usage more likely to appear in a standard place, but they do not change accounting semantics.

Current durable accounting only relies on:

- `prompt_tokens`
- `completion_tokens`
- `total_tokens`

Responses usage is normalized from `usage.input_tokens`, `usage.output_tokens`, and `usage.total_tokens` into the gateway's prompt/completion/total accounting columns. Streaming Responses usage is read from completed response events with `response.usage`.

Provider-specific cache, reasoning, image, audio, and modality counters remain follow-up work. Until those semantics are explicit, successful requests may still become `usage_missing` or `unpriced`.

## Research References

The route-profile design follows the same broad lesson visible in mature adapter stacks: API-family differences are real interfaces, not provider-name strings.

- Vercel AI SDK keeps distinct provider packages for OpenAI, OpenAI-compatible, Anthropic, Google Generative AI, and Google Vertex under [`packages/`](https://github.com/vercel/ai/tree/main/packages).
- The OpenAI-compatible package exposes streaming usage as an explicit provider option rather than assuming every compatible server behaves the same.
- Mario Zechner's provider notes and `pi-mono` OpenAI completions adapter are useful examples of agent-facing compatibility pressure: [post](https://mariozechner.at/posts/2025-11-30-pi-coding-agent/) and [source](https://github.com/badlogic/pi-mono/blob/main/packages/ai/src/providers/openai-completions.ts).

## Follow-Up Scope

These items are intentionally outside this first slice:

- provider compatibility umbrella: [issue #53](https://github.com/ahstn/oceans-llm/issues/53)
- native Anthropic Messages public/API-family mapping: [issue #89](https://github.com/ahstn/oceans-llm/issues/89)
- direct Google Generative AI provider/API-key path: [issue #90](https://github.com/ahstn/oceans-llm/issues/90)
- cross-provider tool-call streaming normalization fixtures: [issue #91](https://github.com/ahstn/oceans-llm/issues/91)
- cache, reasoning, and modality token accounting: [issue #92](https://github.com/ahstn/oceans-llm/issues/92)
- multimodal image/file compatibility across provider families: [issue #93](https://github.com/ahstn/oceans-llm/issues/93)
- Vertex embeddings provider support: [issue #103](https://github.com/ahstn/oceans-llm/issues/103)
- Bedrock native Anthropic Messages streaming over `InvokeModelWithResponseStream`: [issue #139](https://github.com/ahstn/oceans-llm/issues/139)
- Anthropic thinking block replay for tool-use continuations: [issue #140](https://github.com/ahstn/oceans-llm/issues/140)
- Vertex Claude tool and multimodal parity: [issue #141](https://github.com/ahstn/oceans-llm/issues/141)
- route readiness diagnostics: [issue #98](https://github.com/ahstn/oceans-llm/issues/98)
