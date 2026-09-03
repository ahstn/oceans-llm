# Google Vertex AI

`See also`: [Configuration Reference](../configuration/configuration-reference.md), [Model Routing and APIs](../configuration/model-routing-and-api-behavior.md), [Provider API Compatibility](../reference/provider-api-compatibility.md), [Pricing Catalog and Accounting](../configuration/pricing-catalog-and-accounting.md)

This page owns provider-specific configuration examples for Google Vertex AI routes.

## Current Runtime Boundary

The gateway uses one `gcp_vertex` provider type for multiple Vertex publisher families:

- `google/*` chat upstream models use Vertex `generateContent` and `streamGenerateContent`
- supported `google/*` text-embedding upstream models use Vertex `:predict` through the public `/v1/embeddings` path
- `anthropic/*` upstream models use Anthropic-on-Vertex `rawPredict` and `streamRawPredict`
- `/v1/responses` is not implemented for `gcp_vertex` routes in this slice

Vertex routes require Google Cloud authentication with the `https://www.googleapis.com/auth/cloud-platform` scope. The provider supports Application Default Credentials, service-account JSON from a mounted path, and static bearer tokens for constrained environments.

The `auth.mode: service_account` examples on this page are upstream Google Cloud credentials used by the gateway when it calls Vertex. They are not gateway service accounts, do not grant callers access to `/v1/*`, and do not participate in gateway team service-account management.

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

`api_host` is optional. When omitted, the gateway derives it from `location`: `global` uses `aiplatform.googleapis.com`, the multi-region `us` and `eu` locations use `aiplatform.us.rep.googleapis.com` / `aiplatform.eu.rep.googleapis.com`, and any other region uses `{region}-aiplatform.googleapis.com` (for example `us-east5-aiplatform.googleapis.com`). Set `api_host` explicitly only to override that host, such as for a private endpoint. Anthropic-on-Vertex pricing is currently supported only for `location: global`.

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

For service-account JSON:

- provision the Google service account in the target project
- grant the least-privilege Vertex AI permissions needed for the configured models
- mount the JSON as a file and point `credentials_path` at that mounted path
- rotate the JSON or move to ADC/workload identity outside the gateway, then restart or reload the gateway path that reads it

Do not put the JSON document itself in `gateway.yaml`. Use a mounted secret path or a runtime identity mechanism such as ADC.

## Model Identity

Use publisher-qualified `upstream_model` values:

- Google models: `google/<model-id>`
- Anthropic models: `anthropic/<model-id>`

The publisher prefix selects the request mapper and pricing family. The model ID after the slash is passed to the Vertex endpoint path.

Examples verified against Anthropic's [effort reference](https://platform.claude.com/docs/en/build-with-claude/effort), [Claude Fable 5.1 overview](https://platform.claude.com/docs/en/models/fable-5-1/overview), and Google Cloud docs on 2026-09-02:

| Use case | Gateway model id | Vertex `upstream_model` | Notes |
| --- | --- | --- | --- |
| Demanding reasoning and long-horizon agents | `claude-fable-5.1` | `anthropic/claude-fable-5-1` | Claude Fable 5.1 has a 1M-token context window, adaptive thinking that is always on, and a default effort of `high`. |
| General high-capability Claude | `claude-opus-vertex` | `anthropic/claude-opus-4-7` | Claude Opus 4.7 is available through Anthropic-on-Vertex and supports adaptive thinking. |
| Claude coding and agent workloads | `claude-sonnet-vertex` | `anthropic/claude-sonnet-4-6` | Claude Sonnet 4.6 supports adaptive thinking with effort. |
| Older pinned Claude | `claude-sonnet-45-vertex` | `anthropic/claude-sonnet-4-5@20250929` | Versioned Anthropic model IDs use the `@YYYYMMDD` suffix on Vertex. |
| Gemini chat | `gemini-flash-vertex` | `google/gemini-2.0-flash` | Uses the Vertex Google publisher request shape. |
| Gemini embeddings | `gemini-embedding-vertex` | `google/gemini-embedding-001` | Uses Vertex text embeddings `:predict` through `/v1/embeddings`. |
| Gemini Embedding 2 | `gemini-embedding-2-vertex` | `google/gemini-embedding-2` | Uses Vertex `:embedContent` for text-only OpenAI-compatible embeddings. |
| Vertex text embeddings | `text-embedding-vertex` | `google/text-embedding-005` | Older text embedding model using the Vertex text-embedding `:predict` contract. |
| Vertex multilingual embeddings | `text-multilingual-embedding-vertex` | `google/text-multilingual-embedding-002` | Multilingual text embedding model using the Vertex text-embedding `:predict` contract. |

Google documents that Claude model availability varies by endpoint and region. Prefer `global` when your residency policy allows it; use `us`, `eu`, or a regional location when you need a geography-specific processing boundary.

## Claude Example

Anthropic-on-Vertex uses the Anthropic Messages body shape with Vertex transport requirements:

- the model stays in the endpoint path, not the JSON request body
- the body includes `anthropic_version: "vertex-2023-10-16"`
- non-streaming requests use `rawPredict`
- streaming requests use `streamRawPredict`

```yaml
models:
  - id: claude-fable-5.1
    description: Claude Fable 5.1 on Google Vertex AI
    tags: [vertex, claude, reasoning]
    max_reasoning_effort: high
    routes:
      - provider: vertex-global
        upstream_model: anthropic/claude-fable-5-1
        context_window_tokens: 1000000
        capabilities:
          chat_completions: true
          responses: false
          embeddings: false
          stream: true
          tools: true
          vision: false
          json_schema: false
```

Native Claude invocation requires `max_tokens`. If callers omit it, the gateway currently supplies `max_tokens: 1024` for Anthropic-on-Vertex routes.

Anthropic-on-Vertex routes can enable `tools: true` when the upstream Claude model supports tool use. The gateway maps OpenAI Chat Completions function tools, assistant `tool_calls`, tool-result continuations, and streaming tool-use deltas to and from the Anthropic Messages shape used by Vertex. Keep `vision: false` unless you have tested image/document content blocks for the exact route; the Anthropic-on-Vertex mapper still rejects non-text content blocks in this slice.

### Claude Thinking Compatibility

For Anthropic-on-Vertex, OpenAI-shaped `reasoning_effort` maps to Anthropic Messages `output_config.effort` without forwarding the OpenAI-only field. The gateway also applies model-aware thinking policy before sending the Vertex request.

Adaptive example for Claude Fable 5.1:

```json
{
  "anthropic_version": "vertex-2023-10-16",
  "max_tokens": 16000,
  "thinking": {
    "type": "adaptive"
  },
  "output_config": {
    "effort": "high"
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
  "model": "claude-fable-5.1",
  "max_tokens": 16000,
  "reasoning_effort": "high",
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
| Claude Fable 5.1 | Adaptive thinking is always on. Request-level `reasoning_effort`, `reasoning.effort`, or `output_config.effort` maps to adaptive thinking plus `output_config.effort`. The checked-in example caps explicit effort at `high`, so `xhigh` and `max` are rejected by the gateway before Vertex routing. |
| Claude Opus 4.7 and later | `reasoning_effort` or `reasoning.effort` maps to `thinking: { "type": "adaptive" }` plus `output_config.effort`. Manual `thinking.type: "enabled"` and `budget_tokens` are rejected. Non-default `temperature`, `top_p`, and `top_k` are rejected; default `temperature: 1` and `top_p: 1` are omitted. |
| Claude Opus 4.6 and Claude Sonnet 4.6 | `reasoning_effort` maps to adaptive thinking and `output_config.effort`. Caller-supplied manual budgets remain pass-through because Anthropic still accepts them, but they are deprecated upstream. |
| Claude Mythos Preview | Adaptive thinking is the default when `thinking` is unset. `reasoning_effort` maps to `output_config.effort`; `thinking.type: "disabled"` is rejected. |
| Claude Opus 4.5 | Adaptive thinking is rejected. `reasoning_effort` maps to `output_config.effort` only when a manual thinking budget is also supplied. |
| Claude Sonnet/Haiku 4.5 and older Claude models | Adaptive thinking is rejected. These models require an explicit manual budget from `reasoning.budget_tokens`, `reasoning_budget_tokens`, `thinking_budget_tokens`, or caller-supplied `thinking.type: "enabled"` with `budget_tokens`; the gateway does not add `output_config.effort`. |

Anthropic `stop_reason` values map to OpenAI `finish_reason`: `end_turn` and `stop_sequence` to `stop`, `max_tokens` to `length`, `tool_use` to `tool_calls`, and `refusal` to `content_filter`. This mapping is shared with the `anthropic_compat` provider.

#### Per-message effort

Anthropic documents [per-message effort](https://platform.claude.com/docs/en/build-with-claude/effort#change-effort-mid-conversation-beta) for Claude Fable 5.1 as a beta. The request includes `anthropic-beta: mid-conversation-output-config-2026-07-01` and an effort-only system message; the new value applies to the next user turn:

```json
{
  "output_config": {
    "effort": "high"
  },
  "messages": [
    {
      "role": "system",
      "content": [],
      "output_config": {
        "effort": "low"
      }
    },
    {
      "role": "user",
      "content": "Summarize the plan in one sentence."
    }
  ]
}
```

The gateway checks both `output_config.effort` values independently against the effective model ceiling. With `max_reasoning_effort: high`, the values above pass; `xhigh`, `max`, unknown strings, and malformed values fail before route selection.

Per-message effort is beta, and availability of the header and message shape can differ by provider API. Verify Anthropic-on-Vertex support before adding the beta header to a Vertex route. The gateway validates a nested effort value wherever it is present, but policy validation does not make an unsupported provider feature available.

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

Google publisher routes use Vertex `generateContent` and `streamGenerateContent?alt=sse`. Streamed output is folded into Chat Completions chunks; the `finish_reason` chunk is emitted once at end of stream so late `usageMetadata` is never dropped. Every Gemini stream ends with a candidate `finishReason` (or a prompt block with no candidates). If the upstream connection closes before that frame arrives, the gateway emits an error chunk (`google_stream_premature_eof`) instead of `finish_reason: "stop"` and `[DONE]`, so clients do not accept truncated output as complete.

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
          tools: true
          vision: true
          json_schema: false
```

Vertex Google multimodal inputs map remote media to Vertex `fileData`; the gateway does not download or probe the media.

Google Gemini routes support OpenAI Chat Completions function tools, assistant `tool_calls`, tool-result continuations (`role: tool`), and streaming function-call deltas. Named tool choice (`tool_choice: {"type": "function", "function": {"name": "..."}}`), `tool_choice: "required"` / `"any"`, `tool_choice: "none"`, and `tool_choice: "auto"` map to Gemini `toolConfig.functionCallingConfig.mode` and `allowedFunctionNames`. Thought signatures returned by Gemini 3 / thinking-capable models are preserved and relayed across tool continuations.

### Gemini Reasoning and Sampling

OpenAI-shaped `reasoning_effort` (or `reasoning.effort`) maps to `generationConfig.thinkingConfig` and is not forwarded. The wire shape follows the model generation:

| Model | `reasoning_effort` mapping |
| --- | --- |
| Gemini 3.7 Flash and later (any tier) | `thinkingLevel`: `minimal`/`low` -> `LOW`, `medium` -> `MEDIUM`, `high`/`xhigh`/`max` -> `HIGH`. `MINIMAL` is not offered by these models. |
| Gemini 3.0 to 3.6 Flash / Flash-Lite | `thinkingLevel` with all four levels, `minimal` -> `MINIMAL`. |
| Gemini 3.x Pro | `thinkingLevel` `LOW` or `HIGH` only; `minimal` collapses to `LOW`, `medium` to `HIGH`. |
| Gemini 2.5 | `thinkingBudget` per effort tier. `none` sends `0` on Flash / Flash-Lite; 2.5 Pro cannot disable thinking and gets the 128-token floor. |
| Gemini 2.0 and older, or an unrecognised Gemini id | No thinking. Any `reasoning_effort` (including `none`) is rejected with `400 invalid_request_error` so a misconfigured client is visible instead of silently paying for a request it did not intend. Omit the field for these models. |

Gemini 3.x cannot turn thinking off. `reasoning_effort: "none"` (or `"off"`) sends the lowest `thinkingLevel` the model accepts (`MINIMAL` before 3.7, `LOW` from 3.7 and on Pro) together with `includeThoughts: false`. That hides the thought text from the response; the model still thinks at that level and bills those tokens as `reasoning_tokens`.

Gemini 3.6 and later deprecate the classic sampling parameters. The gateway follows the [3.6](https://docs.cloud.google.com/gemini-enterprise-agent-platform/models/guides/gemini-3-6-flash) and [3.7](https://docs.cloud.google.com/gemini-enterprise-agent-platform/models/guides/gemini-3-7-flash) model guides:

- `temperature`, `top_p`, and `top_k` are ignored upstream, so the gateway drops them instead of forwarding.
- `presence_penalty`, `frequency_penalty`, and `n` cause an upstream API error. The gateway accepts the no-op defaults (`0`, `0`, `1`) and rejects any other value with `400 invalid_request_error` naming the native field.
- The rules run on the fully merged `generationConfig`, so they also cover a caller-supplied native `generationConfig` and a route's `extra_body`.
- Gemini 3.5 and older keep the full sampling surface (`temperature`, `topP`, `topK`, `presencePenalty`, `frequencyPenalty`, `candidateCount`).

A caller-supplied native `generationConfig` (or `generation_config`) is deep-merged over the mapped OpenAI fields.

### Gemini Remote Media

Chat Completions accepts these typed content shapes:

- `image_url` and `input_image` for images
- `video_url` and `input_video` for videos
- `file` for generic Vertex-supported media, including video

Media URLs can use `gs://` or `https://`. An HTTPS URL must be publicly readable by Vertex. Plain HTTP and local schemes such as `file://` are rejected. The gateway forwards an accepted URI unchanged, including its signed query string.

Use `mime_type` as the canonical MIME field. The compatibility aliases `media_type` and `mediaType` are also accepted. If more than one field is present, all values must match. Image content requires an `image/*` value, and video content requires a `video/*` value. Generic `file` content can use any MIME type that the selected Gemini model supports. When no MIME field is present, the gateway infers common image, audio, document, and video types from the parsed URL path, without its query string. A missing or unknown MIME type causes a local validation error.

Request logging keeps the media scheme, host, and path but replaces any image, video, or generic-file query string with `?<redacted>`. This protects signed URL credentials in retained request payloads. Redaction does not change the URI sent to Vertex.

Vertex media limits depend on the selected Gemini model and endpoint. Confirm the supported formats, file sizes, media counts, video duration, and VPC Service Controls restrictions in the current [Vertex Gemini inference reference](https://cloud.google.com/vertex-ai/generative-ai/docs/model-reference/inference) and [video understanding documentation](https://cloud.google.com/vertex-ai/generative-ai/docs/multimodal/video-understanding). The gateway does not fetch media or preflight these upstream limits.

## Text Embeddings Example

Native Vertex text embeddings are exposed through the OpenAI-compatible gateway endpoint:

```text
POST /v1/embeddings
```

Use an embedding-only route. Do not make a Gemini chat route embedding-capable just because it also uses the `google/*` publisher prefix.

```yaml
models:
  - id: gemini-embedding
    description: Gemini embeddings on Vertex AI
    tags: [vertex, embeddings]
    routes:
      - provider: vertex-global
        upstream_model: google/gemini-embedding-001
        capabilities:
          chat_completions: false
          responses: false
          embeddings: true
          stream: false
          tools: false
          vision: false
          json_schema: false
```

Supported native Vertex text-embedding upstream models:

| Upstream model | Default/maximum output dimensions | Notes |
| --- | ---: | --- |
| `google/gemini-embedding-001` | 3072 | Supports lower `dimensions` values through Vertex `outputDimensionality`. Vertex accepts one input text per `:predict` request for this model, so the gateway sends one request per input and preserves OpenAI array order. |
| `google/gemini-embedding-2` | 3072 | Uses Vertex `:embedContent`. The gateway supports text-only OpenAI-compatible embeddings; image, audio, video, and PDF multimodal inputs remain unsupported on `/v1/embeddings`. `task_type`, `input_type`, `title`, and `auto_truncate` are not accepted for this model; put task instructions in the input text. |
| `google/text-embedding-005` | 768 | Uses the same Vertex `:predict` text-embedding contract. Array input is batched up to 250 instances per request. |
| `google/text-multilingual-embedding-002` | 768 | Uses the same Vertex `:predict` text-embedding contract. Array input is batched up to 250 instances per request. |

Batched `:predict` requests also respect the Vertex 20,000-token aggregate limit. The gateway has no Gemini tokenizer, so batches are sized with an upper bound of one token per UTF-8 byte; the tokenizer's byte fallback means no input can tokenize to more tokens than bytes. Prose batches come out smaller than strictly necessary, which only costs extra `:predict` calls. A single input larger than the whole budget is still sent on its own and left for Vertex to truncate or reject according to `auto_truncate`.

Request example:

```bash
curl "$OCEANS_BASE_URL/v1/embeddings" \
  -H "Authorization: Bearer $OCEANS_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gemini-embedding",
    "input": ["search query", "document text"],
    "dimensions": 768,
    "task_type": "SEMANTIC_SIMILARITY",
    "encoding_format": "float"
  }'
```

OpenAI SDK example:

```python
from openai import OpenAI

client = OpenAI(base_url="https://gateway.example.com/v1", api_key="...")

response = client.embeddings.create(
    model="gemini-embedding",
    input=["search query", "document text"],
    dimensions=768,
    extra_body={
        "task_type": "SEMANTIC_SIMILARITY",
        "auto_truncate": False,
    },
)
```

Parameter mapping:

| Public request field | Vertex field | Gateway behavior |
| --- | --- | --- |
| `input: "text"` | `instances[].content` for `:predict`; `content.parts[].text` for `google/gemini-embedding-2` `:embedContent` | Returns one embedding with `index: 0`. Empty strings are rejected locally. |
| `input: ["a", "b"]` | batched `instances[]` for `:predict` models that allow it; one request per input for `google/gemini-embedding-001` and `:embedContent` | Returns one embedding per input in original order. A response whose prediction count differs from the batch is rejected. Empty arrays, nested arrays, token arrays, non-string values, and multimodal payloads are rejected locally. |
| `dimensions` | `parameters.outputDimensionality` for `:predict`; `embedContentConfig.outputDimensionality` for `google/gemini-embedding-2` | Must be a positive integer within the supported model maximum. |
| `output_dimensionality` / `outputDimensionality` | Same as `dimensions` | Provider-specific aliases; conflicting aliases are rejected locally. |
| `encoding_format: "float"` or omitted | n/a | Accepted. `base64` is rejected locally. |
| `task_type` | `instances[].task_type` for `:predict` models only | Must be one of Google's supported task enum values. Rejected for `google/gemini-embedding-2`; put task instructions in the input text. |
| `input_type` | Alias for `task_type` for `:predict` models only | Conflicts are rejected. Rejected for `google/gemini-embedding-2`. |
| `title` | `instances[].title` for `:predict` models only | Accepted only for retrieval-document embeddings. Rejected for `google/gemini-embedding-2`. |
| `auto_truncate` / `autoTruncate` | `parameters.autoTruncate` for `:predict` models only | Boolean. When `false`, overlong input is left for Vertex to reject instead of truncating. Rejected for `google/gemini-embedding-2`. |

Allowed task types are `RETRIEVAL_QUERY`, `RETRIEVAL_DOCUMENT`, `SEMANTIC_SIMILARITY`, `CLASSIFICATION`, `CLUSTERING`, `QUESTION_ANSWERING`, `FACT_VERIFICATION`, and `CODE_RETRIEVAL_QUERY`.

Usage, pricing, and budgets:

- The gateway uses real Vertex token counts only: `predictions[].embeddings.statistics.token_count` for `:predict` models and `usageMetadata.promptTokenCount` for `google/gemini-embedding-2`. It does not convert character counts or byte counts into tokens.
- When token counts and exact pricing are available, embedding spend is charged through the same user, service-account, and user-model budgets as other gateway traffic.
- If Vertex omits token counts, the ledger row is `usage_missing`; if exact catalog pricing is unavailable, the row is `unpriced`. Both remain visible in reporting but do not consume budgets.

Troubleshooting:

| Symptom | Check |
| --- | --- |
| `/v1/embeddings` returns a capability or invalid-request error | Confirm the selected route has `embeddings: true` and uses one of the supported embedding upstream models, not a Gemini chat model. |
| `encoding_format` fails | Use `float`; native Vertex `base64` encoding is not implemented. |
| Token-array or nested-array input fails | Send text strings. OpenAI token arrays cannot be translated safely to Vertex text content. |
| Spend row is `usage_missing` | Vertex did not return usable token counts, so the request is visible but not budget-consuming. |
| Spend row is `unpriced` | The pricing catalog did not have an exact supported price for the selected Vertex model/location. |

## Operational Notes

- Keep `responses: false` on all Vertex routes. Keep `embeddings: false` on Vertex chat routes and enable `embeddings: true` only on explicit `google/gemini-embedding-001`, `google/gemini-embedding-2`, `google/text-embedding-005`, or `google/text-multilingual-embedding-002` routes.
- Use `upstream_model: anthropic/<model-id>` for Claude and `upstream_model: google/<model-id>` for Gemini; unqualified model IDs fail at the gateway edge.
- Vertex AI limits Anthropic request payloads to 30 MB. Large documents and many images can hit that byte limit before the model token limit.
- Keep `json_schema: false` unless a route has explicit provider-specific overrides and tests.
- Use `extra_body` only for additive provider fields you have tested for the exact publisher and model family.
- Anthropic-on-Vertex routes may set `tools: true` for tested Claude tool-use models. Keep `vision: false` unless you have gateway fixtures for multimodal Anthropic content blocks. Upstream Claude model capability is not enough by itself; route capability flags should reflect the gateway mapper and tests.
- Check Anthropic and Google Cloud model pages before adding a new Claude route; model IDs, endpoint availability, context windows, and retirement dates vary by model and location.

## Model Armor

Gateway guardrails call the standalone Model Armor `sanitizeUserPrompt` and `sanitizeModelResponse` methods. This is separate from Vertex model routing. A Model Armor template can protect a route on Google Cloud, AWS, OpenAI, or another provider because evaluation occurs at the gateway boundary.

The gateway references existing prompt and response templates. It does not create, update, or delete Model Armor resources. The runtime identity needs `modelarmor.templates.useToSanitizeUserPrompt` or `modelarmor.templates.useToSanitizeModelResponse` on each template. For production, supply the OAuth access token through a protected `file./path` secret reference. The gateway reads the file before each Model Armor evaluation, so an external credential process can rotate the token. An `env.NAME` reference is also supported, but it does not refresh while the process runs.

See [Gateway Guardrails](../operations/gateway-guardrails.md) for phase selection, failure behavior, configuration, rollout, and incident handling.

## Validation

Validate documentation-only edits with `mise run //docs:build`. For runtime Vertex adapter changes, run `cargo test -p gateway-providers vertex::tests` and `cargo clippy -p gateway-providers --all-targets -- -D warnings`.
