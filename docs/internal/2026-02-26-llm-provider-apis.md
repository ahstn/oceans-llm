## Framing: separate **provider account**, **protocol**, and **model identity**

Your examples (Vertex offering Gemini + Claude; OpenCode Zen offering GPT + Gemini) are exactly why you don’t want a 1:1 mapping of:

> “provider == model vendor API”

Instead, you want three orthogonal concepts:

* **Provider account / connection**: “Where do I send HTTP requests and how do I authenticate?”

  * Examples: `vertex-prod-uscentral1`, `openrouter-prod`, `opencode-zen-personal`
* **Protocol (API family)**: “What request/response schema + streaming semantics does this endpoint speak?”

  * Examples: `openai_chat_completions`, `openai_responses` (later), `anthropic_messages`, `google_generateContent` (Gemini native)
* **Model identity**: “What model name does *that endpoint* expect?”

  * Examples: `google/gemini-2.0-flash-001` via Vertex OpenAI endpoint, `claude-opus-4-6` via Anthropic Messages, `gemini-2.5-flash` via Gemini native

This separation is the foundation for reuse in Rust *and* for tolerating subtle provider differences.

---

## What the example APIs tell us (and what to design for)

### 1) OpenAI Chat Completions: “OpenAI-ish JSON + SSE deltas”

Key traits you need to support if you go OpenAI-compatible as your primary gateway surface:

* **Roles & content parts**

  * Chat Completions supports multiple roles, including **`developer`** (newer guidance, especially for newer/reasoning models) and `system` as older-style instructions. ([OpenAI Developers][1])
* **Token limits**

  * `max_completion_tokens` exists and `max_tokens` is deprecated (and incompatible with some model families). ([OpenAI Developers][1])
* **Structured outputs**

  * `response_format` supports plain text and JSON schema constrained output (`json_schema`) as an option. ([OpenAI Developers][1])
* **Streaming**

  * Streaming is via **Server-Sent Events** (`stream: true`), with optional `stream_options` like `include_usage` (final usage chunk may not arrive if interrupted). ([OpenAI Developers][1])

Implication for your Rust contracts:

* “Message content” must be **multi-part** (text + images + files) and not just a string.
* Your streaming design must handle “delta-style” updates + optional usage chunks.

---

### 2) OpenAI “Responses API” is becoming the primary interface

OpenAI’s own guide now recommends the **Responses API** over Chat Completions, especially for reasoning models. ([OpenAI Developers][2])

Implication:

* Even if MVP starts with `/v1/chat/completions`, don’t hardwire your internal contract to Chat Completions quirks.
* Design an internal **conversation + tool calls** contract that can map to both Chat Completions **and** Responses later.

(You can treat this as: `protocol = openai_chat_completions` today; add `protocol = openai_responses` later.)

---

### 3) Anthropic Messages: similar intent, different “shape”

From Anthropic’s “Create a Message” reference:

* Requires `max_tokens` and `messages` where roles are fundamentally `user` / `assistant` (system instructions are *not* modeled as a `system` role in the same way as OpenAI). Also, consecutive turns can be combined by the API. ([Claude][3])
* Model listing is `GET /v1/models` and supports pagination; auth uses `X-Api-Key` plus required `anthropic-version`, and there’s an `anthropic-beta` header for feature flags. ([Claude][4])

Implication:

* Your internal “chat request” model should represent:

  * **system/developer instructions** as a first-class field (not necessarily a message with role = system)
  * messages as a sequence of typed blocks
* You also need a concept of **per-request extra headers** (e.g., `anthropic-beta`) that can be attached at the *model target* level, not only provider-wide.

---

### 4) Vertex AI can be treated as **two different protocols**

Google’s Vertex Gemini docs show two different ways to call models:

1. **Native Gemini/Vertex inference API** (`generateContent`, `streamGenerateContent`, etc.)
2. **OpenAI-compatible endpoint** under:

   * `https://{location}-aiplatform.googleapis.com/v1/projects/{project_id}/locations/{location}/endpoints/openapi`
   * used with an OAuth access token as the “api_key” for the OpenAI client
   * `chat.completions.create(..., stream=True)` works in the example ([Google Cloud Documentation][5])

Implication:

* For MVP and reuse, Vertex can often be modeled as:

  * `provider_connection(protocol=openai_chat_completions, auth=gcp_oauth_token, base_url=.../endpoints/openapi)`
* Later, you can add a `vertex_gemini_native` adapter to access Gemini-native features that OpenAI-compat might not expose cleanly.

---

### 5) OpenCode Zen and “third-party aggregators” are usually OpenAI-compatible, but incomplete

OpenCode Zen is positioned as a curated provider that issues an API key. ([opencode.ai][6])
Community issues show it being called via an OpenAI-style endpoint like:

* `https://opencode.ai/zen/v1/chat/completions` ([GitHub][7])
  …and that it historically may **not expose `/v1/models`**, which breaks auto-discovery in OpenAI-compatible clients. ([GitHub][8])

Implication:

* Treat “OpenAI-compatible” as a **protocol family**, not a guarantee that every endpoint/field exists.
* Your architecture needs:

  * **capability flags per provider+model target** (e.g., supports_models_list = false)
  * graceful degradation or explicit errors

---

### 6) OpenRouter (and others) may support *multiple protocols*

OpenRouter documents an Anthropic Messages “create messages” endpoint that “uses the Anthropic Messages API format.” ([OpenRouter][9])

Implication:

* A single “provider” brand may expose multiple protocols; you should represent this either as:

  * multiple `provider_connection`s (one per protocol), or
  * one provider with multiple named endpoints internally

For MVP simplicity: multiple provider connections.

---

## Recommended Rust architecture for shared contracts + subtle modifications

### A) Use a **protocol-first adapter layer**

Instead of one adapter per “provider brand”, implement adapters per **protocol**:

* `OpenAiChatCompletionsAdapter`
* `AnthropicMessagesAdapter`
* `GeminiGenerateContentAdapter` (later)
* `OpenAiResponsesAdapter` (later)

Then let each provider connection pick a protocol:

* Vertex OpenAI endpoint → `OpenAiChatCompletionsAdapter`
* OpenCode Zen → `OpenAiChatCompletionsAdapter`
* Anthropic first-party → `AnthropicMessagesAdapter`
* OpenRouter Anthropic endpoint → `AnthropicMessagesAdapter`

This maximizes code reuse.

---

### B) Canonical internal contract: a **superset** of common semantics

Design a `core` module that is *not* OpenAI-specific nor Anthropic-specific.

Key idea: model “chat” as **messages + typed content parts + tool calls**, plus a flexible extension map.

Sketch:

```rust
/// Canonical input used by routing + adapters.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CoreChatRequest {
    /// Gateway model alias requested by the client (before routing),
    /// or resolved upstream model after routing (depending on stage).
    pub model: String,

    /// Highest-priority instructions (maps to OpenAI developer/system, Anthropic system, etc).
    pub instructions: Option<Vec<CoreContentPart>>,

    pub messages: Vec<CoreMessage>,

    pub tools: Vec<CoreTool>,
    pub tool_choice: Option<CoreToolChoice>,

    pub generation: CoreGenerationParams,

    /// Gateway-controlled metadata (authz subject, request id, etc).
    pub meta: CoreRequestMeta,

    /// Escape hatch for vendor/proxy-specific parameters (controlled/whitelisted).
    #[serde(default)]
    pub extensions: std::collections::BTreeMap<String, serde_json::Value>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CoreMessage {
    pub role: CoreRole,
    pub content: Vec<CoreContentPart>,
    /// For tool loops / multi-agent:
    pub name: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum CoreRole { Developer, System, User, Assistant, Tool }

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum CoreContentPart {
    Text { text: String },
    ImageUrl { url: String, mime: Option<String> },
    FileRef { uri: String, mime: Option<String> },

    /// Normalized tool calling
    ToolCall { id: String, name: String, arguments_json: serde_json::Value },
    ToolResult { id: String, content: String },

    /// Optional: “thinking/reasoning” blocks (preserve if needed)
    Reasoning { text: String },
}
```

Why this works:

* OpenAI supports multipart content and different roles. ([OpenAI Developers][1])
* Anthropic uses message blocks and treats system separately (your `instructions` maps naturally). ([Claude][3])
* Gemini/Vertex can map to “role + parts” easily (native and OpenAI-compat variants). ([Google Cloud Documentation][5])

---

### C) Provider connection vs gateway model vs model target

Model the “routing object” explicitly:

* **ProviderConnection**: credentials + base_url + protocol + defaults
* **GatewayModel**: “name clients use” (alias) + tags
* **ModelTarget**: concrete upstream target (provider + upstream_model_id) + overrides

This matches how aggregators work in practice.

```rust
pub struct ProviderConnection {
    pub id: String,
    pub protocol: Protocol,
    pub base_url: url::Url,
    pub auth: ProviderAuth,
    pub default_headers: http::HeaderMap,
    pub defaults: ProviderDefaults,
    pub quirks: ProviderQuirks,
}

pub struct ModelTarget {
    pub provider_id: String,
    pub upstream_model: String,
    pub weight: u32,
    pub param_overrides: serde_json::Map<String, serde_json::Value>,
    pub header_overrides: http::HeaderMap,

    /// Critical: capability overrides per (provider, model)
    pub capabilities: CapabilitySet,
}
```

This is how you represent:

* `vertex-prod` + `google/gemini-...`
* `vertex-prod` + `anthropic/claude-...` (if exposed via some endpoint)
* `opencode-zen` + `gpt-...`

---

## Handling subtle differences without forking code everywhere

### 1) Capability-driven validation + “drop/convert/error” policies

A lot of differences are just: **provider supports a subset** of fields.

Examples from sources:

* OpenAI Chat Completions has `max_completion_tokens` and deprecates `max_tokens`. ([OpenAI Developers][1])
* Some providers (like OpenCode Zen historically) may not implement `/v1/models`. ([GitHub][8])
* Streaming options like `include_usage` exist for OpenAI SSE streams. ([OpenAI Developers][1])
* Anthropic adds beta behavior via `anthropic-beta` headers. ([Claude][10])

So define:

```rust
#[derive(Clone, Debug)]
pub struct CapabilitySet {
    pub chat_completions: bool,
    pub responses_api: bool,
    pub embeddings: bool,
    pub models_list: bool,

    pub streaming: bool,
    pub tools: bool,
    pub vision: bool,
    pub json_schema: bool,

    pub supports_developer_role: bool,
    pub supports_stream_usage_chunk: bool,
}
```

Then implement an “enforcer”:

* If request asks for `response_format=json_schema` but `json_schema=false`:

  * either error (strict mode)
  * or convert to “best effort” (inject instruction “output JSON”, etc.) with a warning span

This is the same *shape* as LiteLLM’s “supported OpenAI params per provider” approach (it maintains a supported parameter list and adapts accordingly). ([GitHub][11])

---

### 2) A small, composable “transform pipeline” (middleware for requests/responses)

Instead of per-provider forks, build an internal pipeline like:

1. **Normalize incoming API → CoreChatRequest**
2. **Validate + coerce (capabilities)**
3. **Resolve route → ModelTarget**
4. **Apply target overrides**
5. **Convert Core → Upstream protocol request**
6. **Apply protocol-level quirks**
7. **Execute**
8. **Decode upstream response/stream → CoreChatResponse/CoreStreamEvent**
9. **Encode Core → outgoing API**

The key to subtle modifications is steps **4** and **6**.

Design “quirks” as mostly data-driven transforms, e.g.:

* role mapping:

  * `developer → system` for providers that don’t support developer role
* parameter mapping:

  * `max_completion_tokens → max_tokens` for older OpenAI-compatible backends
* drop fields:

  * drop `stream_options.include_obfuscation` if upstream rejects it
* inject headers:

  * add `anthropic-beta: ...` for certain targets
* path overrides:

  * some OpenAI-compatible providers want `/api/v1` instead of `/v1`

This can be represented as:

```rust
pub trait RequestTransform: Send + Sync {
    fn apply_openai_body(&self, body: &mut serde_json::Value);
    fn apply_headers(&self, headers: &mut http::HeaderMap);
}
```

and you attach a `Vec<Box<dyn RequestTransform>>` to a connection/target.

---

### 3) Streaming: unify via a **CoreStreamEvent**, then render to client protocol

OpenAI streaming emits “delta chunks” and ends with a `[DONE]` sentinel, and has optional final usage chunk. ([OpenAI Developers][1])
Anthropic streaming is event-based with different semantics (and beta features can change streaming behavior). ([Claude][10])

To avoid N×M complexity, do:

* Each upstream adapter parses its stream into an internal stream of:

```rust
pub enum CoreStreamEvent {
    MessageStart { id: String },
    TextDelta { text: String },
    ToolCallDelta { id: String, name: Option<String>, arguments_fragment: String },
    Usage { input_tokens: u64, output_tokens: u64 },
    MessageStop { stop_reason: Option<String> },
}
```

Then:

* Your outgoing OpenAI-compatible endpoint turns those into `data: {...}` SSE frames
* Your outgoing Anthropic-compatible endpoint (if you add one later) turns those into Anthropic SSE events

This keeps each adapter’s streaming logic isolated and testable.

---

## Practical recommendations for your MVP given the provider mix

### MVP focus: implement **OpenAI-compatible protocol** first for max coverage

Because:

* Vertex explicitly supports OpenAI client usage via its OpenAI endpoint. ([Google Cloud Documentation][5])
* OpenCode Zen appears to be called via OpenAI-style chat completions endpoint. ([GitHub][7])
* Many aggregators and gateways use OpenAI-compat as the common denominator.

Then add:

* `AnthropicMessagesAdapter` second (to cover first-party Claude and any Anthropic-compatible proxies). ([Claude][4])

### Don’t assume `/v1/models` exists everywhere

OpenCode Zen has community requests to add `/v1/models` for discovery; treat model discovery as “best effort” and allow static config overrides. ([GitHub][8])

### Plan for “model requires Responses API, not Chat Completions”

Even some OpenAI ecosystem tooling documents that certain models may not support `/chat/completions` and require Responses instead. ([Zed][12])

So in your model metadata/capabilities:

* `capabilities.chat_completions = false`
* route the request to a protocol adapter that can serve it (later `openai_responses`)

---

## Example: how this looks in config terms (conceptual)

```yaml
providers:
  vertex_uscentral1_openai:
    protocol: openai_chat_completions
    base_url: "https://us-central1-aiplatform.googleapis.com/v1/projects/myproj/locations/us-central1/endpoints/openapi"
    auth:
      type: gcp_oauth
      # service account / ADC details live here
    defaults:
      timeout_ms: 60000
    quirks:
      # if needed
      drop_fields: ["stream_options.include_obfuscation"]

  anthropic_primary:
    protocol: anthropic_messages
    base_url: "https://api.anthropic.com"
    auth:
      type: header_api_key
      header: "X-Api-Key"
      from_env: "ANTHROPIC_API_KEY"
    defaults:
      headers:
        anthropic-version: "2023-06-01"

  opencode_zen:
    protocol: openai_chat_completions
    base_url: "https://opencode.ai/zen/v1"
    auth:
      type: bearer
      from_env: "OPENCODE_ZEN_API_KEY"
    capabilities:
      models_list: false  # until confirmed

models:
  gemini_fast:
    targets:
      - provider: vertex_uscentral1_openai
        upstream_model: "google/gemini-2.0-flash-001"
        capabilities: { streaming: true, tools: true, vision: true }
  claude_smart:
    targets:
      - provider: anthropic_primary
        upstream_model: "claude-opus-4-6"
        header_overrides:
          anthropic-beta: "interleaved-thinking-2025-05-14"
```

This demonstrates:

* Same gateway can expose `gemini_fast` and `claude_smart`
* Provider connections are “how to call”
* Targets are “which model name + overrides”

---

## Sources

* OpenAI Chat Completions API reference (parameters, roles, streaming, response_format): ([OpenAI Developers][1])
* OpenAI Text generation guide (Responses API recommendation): ([OpenAI Developers][2])
* Anthropic API reference (Create a Message; List Models; beta headers): ([Claude][3])
* Google Cloud Vertex AI Gemini inference docs (OpenAI-compatible endpoint + streaming example): ([Google Cloud Documentation][5])
* OpenRouter Anthropic Messages reference (supports Anthropic Messages format): ([OpenRouter][9])
* OpenCode Zen docs (Zen overview/provider concept): ([opencode.ai][6])
* OpenCode Zen endpoint evidence + /v1/models gap (community issues): ([GitHub][7])
* LiteLLM Vertex/Gemini adapter code (illustrative “supported params / translation” pattern): ([GitHub][11])
* Zed “LLM providers” docs (note about some models not supporting chat completions): ([Zed][12])
* AWS Bedrock Anthropic messages overview (cross-provider “messages” standardization context): ([AWS Documentation][13])

[1]: https://developers.openai.com/api/reference/resources/chat/subresources/completions/methods/create "Create chat completion | OpenAI API Reference"
[2]: https://developers.openai.com/api/docs/guides/text "Text generation | OpenAI API"
[3]: https://platform.claude.com/docs/en/api/messages/create?utm_source=chatgpt.com "Create a Message - Claude API Reference"
[4]: https://platform.claude.com/docs/en/api/models/list "List Models - Claude API Reference"
[5]: https://docs.cloud.google.com/vertex-ai/generative-ai/docs/model-reference/inference "Generate content with the Gemini API in Vertex AI  |  Generative AI on Vertex AI  |  Google Cloud Documentation"
[6]: https://opencode.ai/docs/zen/?utm_source=chatgpt.com "Zen"
[7]: https://github.com/anomalyco/opencode/issues/8228?utm_source=chatgpt.com "OpenCode Zen Gemini Integration - 500 Internal Server Error"
[8]: https://github.com/anomalyco/opencode/issues/2901?utm_source=chatgpt.com "Add OpenAI-compatible /v1/models endpoint to OpenCode ..."
[9]: https://openrouter.ai/docs/api/api-reference/anthropic-messages/create-messages?explorer=true&utm_source=chatgpt.com "Create a message"
[10]: https://platform.claude.com/docs/en/api/beta-headers?utm_source=chatgpt.com "Beta headers - Claude API Docs"
[11]: https://github.com/BerriAI/litellm/blob/main/litellm/llms/vertex_ai/gemini/vertex_and_google_ai_studio_gemini.py "litellm/litellm/llms/vertex_ai/gemini/vertex_and_google_ai_studio_gemini.py at main · BerriAI/litellm · GitHub"
[12]: https://zed.dev/docs/ai/llm-providers?utm_source=chatgpt.com "LLM Providers - Use Your Own API Keys in Zed"
[13]: https://docs.aws.amazon.com/bedrock/latest/userguide/model-parameters-anthropic-claude-messages.html?utm_source=chatgpt.com "Anthropic Claude Messages API"

