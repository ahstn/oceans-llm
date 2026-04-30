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
| `aws_bedrock` | Supported for non-streaming Bedrock Converse when route capabilities allow it. Keep route `stream: false` until EventStream support lands. | Not implemented; keep route `responses: false`. | Not implemented; keep route `embeddings: false`. |

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

For Bedrock, this foundation guarantees config load, validation, seeding, registration, deterministic region, endpoint, timeout, display, auth metadata, and non-streaming Converse chat execution for bearer-token auth. Bedrock `upstream_model` values should match Bedrock Runtime model identity: base model IDs, inference profile IDs, or Bedrock ARNs accepted by the target Bedrock Runtime operation. AWS documents `InvokeModel` as requiring `bedrock:InvokeModel`; streamed invocation and Converse access require the corresponding Bedrock runtime permissions. IAM/SigV4 signing and EventStream chat streaming remain follow-up work.

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
- Bedrock EventStream chat streaming: [issue #128](https://github.com/ahstn/oceans-llm/issues/128)
- route readiness diagnostics: [issue #98](https://github.com/ahstn/oceans-llm/issues/98)
