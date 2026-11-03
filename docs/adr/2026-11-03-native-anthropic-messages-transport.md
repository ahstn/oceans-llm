# Native Anthropic Messages transport

## Decision

Route public Anthropic Messages requests through explicit native provider methods whenever the selected route supports the Messages protocol. OpenRouter Claude routes automatically select `/api/v1/messages`, including `~anthropic/claude-opus-latest`. Chat Completions callers remain supported through a translation at the public API boundary.

Native Messages controls on rolling aliases, private deployments and unrecognized model versions are validated by the upstream. Known explicit model versions retain Oceans' local thinking rules. A rolling family name must never acquire the legacy manual-thinking restrictions from the classifier's fallback.

## Implementation

`ProviderClient` exposes native JSON and streaming methods, together with a route-dependent capability check. Direct Anthropic, Vertex Claude, Bedrock Runtime Anthropic Invoke, Bedrock Mantle Anthropic Messages, OpenRouter Claude, and Copilot routes configured with `chat_api: anthropic_messages` implement that boundary. Other routes keep the existing translation path.

Native requests keep top-level `system` separate from system turns inside `messages`. Message fields, content blocks, signatures and their order survive the core request representation. Cloud adapters apply the platform's endpoint, authentication, version and beta mapping. Route overrides, model access, effort ceilings, budgets, guardrails and request logging still apply.

Native JSON and SSE no longer pass through a Chat Completions response projection. Oceans changes the public model identity to the requested gateway model, and retains native stop reasons, content, usage, extensions and unknown events. The stream changes only the initial model identity; subsequent frames pass through unchanged when guardrails do not transform them. No terminal event is synthesized for an interrupted native stream.

Native provider results carry response headers. Anthropic and OpenRouter extension namespaces, rate-limit headers, `retry-after`, `x-should-retry` and upstream `request-id` survive both successful responses and upstream HTTP errors. Oceans keeps its own `x-request-id`. Caller credentials, cookies and connection headers are excluded. Native upstream error bodies retain the wording clients use for capability-rejection recovery.

Accounting accepts native cache-exclusive token counts directly. Cache reads and writes contribute to the normalized prompt used for context-tier selection and billing. The caller still receives the upstream's original usage object. OpenRouter-reported costs remain authoritative for both public Messages and Chat Completions requests.

## Why

The previous native path reconstructed responses from Chat Completions. It discarded server-tool and future content blocks, reordered signed thinking, converted `pause_turn` to `end_turn`, and lost cache usage fields. Selecting an Anthropic-compatible upstream alone could not prevent those losses. The exact rolling Opus alias also fell through to `ManualOnly` in v0.47 and v0.48, rejecting valid adaptive thinking before dispatch.

See Anthropic's [gateway protocol](https://code.claude.com/docs/en/llm-gateway-protocol), [thinking restrictions](https://platform.claude.com/docs/en/build-with-claude/thinking-troubleshooting), and OpenRouter's [Messages endpoint](https://openrouter.ai/docs/api/api-reference/anthropic-messages/create-a-message).

## Trade-offs and verification

Upstream validation means a new alias or model option can work without an Oceans release. It also means model-specific errors for unknown identities arrive from the provider. Explicit gateway access, budget and effort-ceiling policies remain enforced locally.

Cloud providers retain their own endpoints: Vertex uses `rawPredict` and `streamRawPredict`, Bedrock Runtime uses InvokeModel, and Bedrock Mantle uses `/anthropic/v1/messages`. Bedrock Runtime Anthropic streaming remains unsupported. OpenRouter token counting and complete Claude Code workflow compatibility are separate integration contracts.

The verification gate requires native request/response/SSE captures across transports, rolling-alias parity, cached-usage accounting and native generated-tool guardrails. A bounded live request verifies the public native route and its Request Logs entry. The earlier [Anthropic-compatible provider decision](2026-09-03-anthropic-compatible-provider-and-opencode-zen.md) still governs Chat Completions translation.
