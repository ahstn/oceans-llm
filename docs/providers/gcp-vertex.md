# Google Vertex AI

`See also`: [Configuration Reference](../configuration/configuration-reference.md), [Model Routing and API Behavior](../configuration/model-routing-and-api-behavior.md), [Provider API Compatibility](../reference/provider-api-compatibility.md), [Pricing Catalog and Accounting](../configuration/pricing-catalog-and-accounting.md)

This page owns provider-specific configuration examples for Google Vertex AI routes.

## Current Runtime Boundary

The gateway uses one `gcp_vertex` provider type for multiple Vertex publisher families:

- `google/*` upstream models use Vertex `generateContent` and `streamGenerateContent`
- `anthropic/*` upstream models use Anthropic-on-Vertex `rawPredict` and `streamRawPredict`
- `/v1/responses` and `/v1/embeddings` are not implemented for `gcp_vertex` routes in this slice

Vertex routes require Google Cloud authentication with the `https://www.googleapis.com/auth/cloud-platform` scope. The provider supports Application Default Credentials, service-account JSON from a mounted path, and static bearer tokens for constrained environments.

## Provider

```yaml
providers:
  - id: vertex-global
    type: gcp_vertex
    project_id: env.GCP_PROJECT_ID
    location: global
    auth:
      mode: adc
    display:
      label: Google Vertex AI
      icon_key: vertexai
```

`api_host` is optional. When omitted, the gateway uses `aiplatform.googleapis.com`, which is the right default for the global endpoint. For Vertex multi-region endpoints, set `api_host` explicitly to `aiplatform.us.rep.googleapis.com` or `aiplatform.eu.rep.googleapis.com`. For a regional endpoint, set it to the regional Vertex host such as `us-east5-aiplatform.googleapis.com`. Anthropic-on-Vertex pricing is currently supported only for `location: global`.

Service-account and bearer examples:

```yaml
providers:
  - id: vertex-service-account
    type: gcp_vertex
    project_id: env.GCP_PROJECT_ID
    location: us
    api_host: aiplatform.us.rep.googleapis.com
    auth:
      mode: service_account
      credentials_path: /var/run/secrets/gcp/service-account.json

  - id: vertex-bearer
    type: gcp_vertex
    project_id: env.GCP_PROJECT_ID
    location: us-central1
    api_host: us-central1-aiplatform.googleapis.com
    auth:
      mode: bearer
      token: env.GCP_VERTEX_ACCESS_TOKEN
```

## Model Identity

Use publisher-qualified `upstream_model` values:

- Google models: `google/<model-id>`
- Anthropic models: `anthropic/<model-id>`

The publisher prefix selects the request mapper and pricing family. The model ID after the slash is passed to the Vertex endpoint path.

Examples verified against Anthropic and Google Cloud docs on 2026-05-01:

| Use case | Gateway model id | Vertex `upstream_model` | Notes |
| --- | --- | --- | --- |
| Latest high-capability Claude | `claude-opus-vertex` | `anthropic/claude-opus-4-7` | Claude Opus 4.7 is available through Anthropic-on-Vertex and supports adaptive thinking. |
| Claude coding and agent workloads | `claude-sonnet-vertex` | `anthropic/claude-sonnet-4-6` | Claude Sonnet 4.6 supports adaptive thinking with effort. |
| Older pinned Claude | `claude-sonnet-45-vertex` | `anthropic/claude-sonnet-4-5@20250929` | Versioned Anthropic model IDs use the `@YYYYMMDD` suffix on Vertex. |
| Gemini chat | `gemini-flash-vertex` | `google/gemini-2.0-flash` | Uses the Vertex Google publisher request shape. |

Google documents that Claude model availability varies by endpoint and region. Prefer `global` when your residency policy allows it; use `us`, `eu`, or a regional location when you need a geography-specific processing boundary.

## Claude Example

Anthropic-on-Vertex uses the Anthropic Messages body shape with Vertex transport requirements:

- the model stays in the endpoint path, not the JSON request body
- the body includes `anthropic_version: "vertex-2023-10-16"`
- non-streaming requests use `rawPredict`
- streaming requests use `streamRawPredict`

```yaml
models:
  - id: claude-opus-vertex
    description: Claude Opus on Google Vertex AI
    tags: [vertex, claude, reasoning]
    routes:
      - provider: vertex-global
        upstream_model: anthropic/claude-opus-4-7
        capabilities:
          chat_completions: true
          responses: false
          embeddings: false
          stream: true
          tools: false
          vision: false
          json_schema: false
```

Native Claude invocation requires `max_tokens`. If callers omit it, the gateway currently supplies `max_tokens: 1024` for Anthropic-on-Vertex routes.

Keep `tools: false` and `vision: false` for Anthropic-on-Vertex routes in this slice. Upstream Claude-on-Vertex uses an Anthropic Messages-like API and current model cards advertise broader agent, computer-use, image, and document capabilities for some Claude models, but the gateway's current Anthropic-on-Vertex mapper is intentionally narrower. Function tools, tool-result continuations, image blocks, document blocks, and their stream behavior remain capability-gated until request mapping and fixtures are added in [issue #141](https://github.com/ahstn/oceans-llm/issues/141).

### Claude Thinking Compatibility

For Anthropic-on-Vertex, OpenAI-shaped `reasoning_effort` maps to Anthropic Messages `output_config.effort` without forwarding the OpenAI-only field. The gateway also applies model-aware thinking policy before sending the Vertex request.

Adaptive example for Claude Opus 4.7:

```json
{
  "anthropic_version": "vertex-2023-10-16",
  "max_tokens": 16000,
  "thinking": {
    "type": "adaptive"
  },
  "output_config": {
    "effort": "xhigh"
  },
  "messages": [
    {
      "role": "user",
      "content": "Review this implementation plan."
    }
  ]
}
```

Gateway callers can request the same shape with OpenAI-compatible fields:

```json
{
  "model": "claude-opus-vertex",
  "max_tokens": 16000,
  "reasoning_effort": "xhigh",
  "messages": [
    {
      "role": "user",
      "content": "Review this implementation plan."
    }
  ]
}
```

The gateway sends `thinking: { "type": "adaptive" }` and `output_config.effort` upstream, and removes `reasoning_effort`.

Model behavior:

| Model family | Gateway behavior |
| --- | --- |
| Claude Opus 4.7 and later | `reasoning_effort` or `reasoning.effort` maps to `thinking: { "type": "adaptive" }` plus `output_config.effort`. Manual `thinking.type: "enabled"` and `budget_tokens` are rejected. Non-default `temperature`, `top_p`, and `top_k` are rejected; default `temperature: 1` and `top_p: 1` are omitted. |
| Claude Opus 4.6 and Claude Sonnet 4.6 | `reasoning_effort` maps to adaptive thinking and `output_config.effort`. Caller-supplied manual budgets remain pass-through because Anthropic still accepts them, but they are deprecated upstream. |
| Claude Mythos Preview | Adaptive thinking is the default when `thinking` is unset. `reasoning_effort` maps to `output_config.effort`; `thinking.type: "disabled"` is rejected. |
| Claude Opus 4.5 | Adaptive thinking is rejected. `reasoning_effort` maps to `output_config.effort` only when a manual thinking budget is also supplied. |
| Claude Sonnet/Haiku 4.5 and older Claude models | Adaptive thinking is rejected. These models require an explicit manual budget from `reasoning.budget_tokens`, `reasoning_budget_tokens`, `thinking_budget_tokens`, or caller-supplied `thinking.type: "enabled"` with `budget_tokens`; the gateway does not add `output_config.effort`. |

Manual budget example for an older Claude model:

```json
{
  "model": "claude-sonnet-45-vertex",
  "max_tokens": 8192,
  "reasoning": {
    "effort": "medium",
    "budget_tokens": 2048
  },
  "messages": [
    {
      "role": "user",
      "content": "Analyze this migration risk."
    }
  ]
}
```

For Claude Sonnet 4.5, the gateway sends manual `thinking.type: "enabled"` with `budget_tokens` and omits `output_config.effort`. For Claude Opus 4.5, it sends the manual budget and `output_config.effort`.

Chat Completions hides Claude thinking from normal `content` and `delta.content`. Native Anthropic `thinking`, `redacted_thinking`, `thinking_delta`, and `signature_delta` blocks are preserved under `provider_metadata.gcp_vertex.reasoning` for debugging and provider continuity. The gateway does not yet rehydrate that provider metadata into future Anthropic content blocks when callers send tool results. Anthropic documents that tool-use continuations with thinking may require complete unmodified thinking blocks, so gateway-managed replay remains tracked by [issue #140](https://github.com/ahstn/oceans-llm/issues/140).

## Gemini Example

Google publisher routes use Vertex `generateContent` and `streamGenerateContent`.

```yaml
models:
  - id: gemini-flash-vertex
    description: Gemini Flash on Google Vertex AI
    tags: [vertex, gemini]
    routes:
      - provider: vertex-global
        upstream_model: google/gemini-2.0-flash
        capabilities:
          chat_completions: true
          responses: false
          embeddings: false
          stream: true
          tools: false
          vision: true
          json_schema: false
```

Vertex Google multimodal inputs currently accept `gs://` image and file URIs through OpenAI-compatible typed content. Inline/base64 data and remote HTTP URLs are not supported in this gateway slice.

## Operational Notes

- Set `responses: false` and `embeddings: false` on Vertex routes until those API families exist in the provider adapter.
- Use `upstream_model: anthropic/<model-id>` for Claude and `upstream_model: google/<model-id>` for Gemini; unqualified model IDs fail at the gateway edge.
- Vertex AI limits Anthropic request payloads to 30 MB. Large documents and many images can hit that byte limit before the model token limit.
- Keep `json_schema: false` unless a route has explicit provider-specific overrides and tests.
- Use `extra_body` only for additive provider fields you have tested for the exact publisher and model family.
- Keep Anthropic-on-Vertex `tools: false` and `vision: false` unless you have gateway fixtures for that route. Upstream Claude model capability is not enough by itself; route capability flags should reflect the gateway mapper and tests.
- Check Anthropic and Google Cloud model pages before adding a new Claude route; model IDs, endpoint availability, context windows, and retirement dates vary by model and location.

## Validation

Validate documentation-only edits with `mise run docs-check`. For runtime Vertex adapter changes, run `cargo test -p gateway-providers vertex::tests` and `cargo clippy -p gateway-providers --all-targets -- -D warnings`.
