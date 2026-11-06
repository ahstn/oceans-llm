# Native Anthropic Messages transport

## Decision

Route public Anthropic Messages requests through explicit native provider methods whenever the selected route supports the Messages protocol. OpenRouter Claude routes automatically select `/api/v1/messages`, including `~anthropic/claude-opus-latest`. Chat Completions callers remain supported through a translation at the public API boundary.

Native Messages controls on rolling aliases, private deployments and unrecognized model versions are validated by the upstream. Known explicit model versions retain Oceans' local thinking rules. A rolling family name must never acquire the legacy manual-thinking restrictions from the classifier's fallback.

## Implementation

`ProviderClient` exposes native JSON and streaming methods, together with a route-dependent capability check. Direct Anthropic, Vertex Claude, Bedrock Runtime Anthropic Invoke, Bedrock Mantle Anthropic Messages, OpenRouter Claude, and Copilot routes configured with `chat_api: anthropic_messages` implement that boundary. Other routes keep the existing translation path. Native methods follow the trait's existing optional-operation convention for batch, token counting, images and Decisions. A separate capability trait would add another registry or accessor convention while still needing route-dependent checks.

Native requests keep top-level `system` separate from system turns inside `messages`. Message fields, content blocks, signatures and their order survive the core request representation. Cloud adapters apply the platform's endpoint, authentication, version and beta mapping. Platform-pinned headers replace forwarded values before authentication or signing; Mantle sends one pinned `anthropic-version`. Route overrides, model access, effort ceilings, budgets, guardrails and request logging still apply.

Prompt guardrails inspect plain-string `system` values without moving them into `messages`. Stream guardrails accept CR, LF, CRLF and mixed line endings. They inspect nonempty complete start-frame tool inputs independently from later JSON deltas, and transformed start inputs remain JSON objects. Empty opening objects keep their normal delta behavior. Original frame bytes and metadata survive unless a transform changes the event's data.

Native JSON and SSE no longer pass through a Chat Completions response projection. Oceans changes the public model identity to the requested gateway model, and retains native stop reasons, content, usage, extensions and unknown events. The stream changes only the initial model identity; subsequent frames pass through unchanged when guardrails do not transform them. No terminal event is synthesized for an interrupted native stream.

The existing stream collector detects EOF before `message_stop`, fails the response body and records a stream error. It still accounts for observed usage. This shares the logging parser rather than adding a second parser to the transport.

Native provider results carry response headers. Anthropic and OpenRouter extension namespaces, rate-limit headers, `retry-after`, `x-should-retry` and upstream `request-id` survive both successful responses and upstream HTTP errors. Oceans keeps its own `x-request-id`. Caller credentials, cookies and connection headers are excluded. Native upstream error bodies retain the wording clients use for capability-rejection recovery.

Accounting accepts native cache-exclusive token counts directly. Cache reads and writes contribute to the normalized prompt used for context-tier selection and billing. Translated Chat Completions streams include those counters in `total_tokens`, independent of the provider label. Native Messages callers still receive the upstream's original usage object. OpenRouter-reported costs remain authoritative for both public Messages and Chat Completions requests.

OpenRouter Chat Completions translation retains HTTP(S) image URLs as native URL sources; other Anthropic transports keep their existing image restrictions. Image translation preserves outer cache controls and TTLs across remote URL, data URL and base64 forms. Token-limit aliases are resolved within each precedence layer before the default is added. A route limit overrides the caller limit, and the native body contains only `max_tokens`.

OpenRouter Chat Completions JSON schemas become native `output_config.format`; native Messages output options remain opaque. One predicate in core's existing `route_capabilities` module selects OpenRouter native routes for both provider dispatch and advertised metadata, so these boundaries cannot drift. It belongs with shared compatibility inference, not model-thinking policy. Moving it into the provider implementation would require the service layer to depend on provider SDKs or duplicate the policy.

Unknown native models retain explicit manual thinking and budget aliases. OpenAI effort aliases do not add native effort to manual thinking; caller-supplied native effort remains unchanged. Conflicting controls still fail locally.

Valid mixed-TTL cache counts survive even when the current pricing schema cannot price the TTL classes. They still count toward token rate limits and ledger totals. The existing unpriced policy remains in place unless the provider supplies a valid cost; separate TTL rates and migrations are outside this transport change.

## Why

The previous native path reconstructed responses from Chat Completions. It discarded server-tool and future content blocks, reordered signed thinking, converted `pause_turn` to `end_turn`, and lost cache usage fields. Selecting an Anthropic-compatible upstream alone could not prevent those losses. The exact rolling Opus alias also fell through to `ManualOnly` in v0.47 and v0.48, rejecting valid adaptive thinking before dispatch.

See Anthropic's [gateway protocol](https://code.claude.com/docs/en/llm-gateway-protocol), [thinking restrictions](https://platform.claude.com/docs/en/build-with-claude/thinking-troubleshooting), and OpenRouter's [Messages endpoint](https://openrouter.ai/docs/api/api-reference/anthropic-messages/create-a-message).

## Trade-offs and verification

Upstream validation means a new alias or model option can work without an Oceans release. It also means model-specific errors for unknown identities arrive from the provider. Explicit gateway access, budget and effort-ceiling policies remain enforced locally.

Cloud providers retain their own endpoints: Vertex uses `rawPredict` and `streamRawPredict`, Bedrock Runtime uses InvokeModel, and Bedrock Mantle uses `/anthropic/v1/messages`. Bedrock Runtime Anthropic streaming remains unsupported. OpenRouter token counting and complete Claude Code workflow compatibility are separate integration contracts.

The verification gate requires native request/response/SSE captures across transports, rolling-alias parity, cached-usage accounting and native generated-tool guardrails. A bounded live request verifies the public native route and its Request Logs entry. The earlier [Anthropic-compatible provider decision](2026-09-03-anthropic-compatible-provider-and-opencode-zen.md) still governs Chat Completions translation.

## Amendment: opaque Chat continuation state (2026-11-05)

Chat clients also need complete native state for signed tool continuations. A reasoning-only projection cannot recover the original order of thinking, tools and provider extensions. Each native family therefore owns its replay envelope and validates it against the public Chat projection before restoration. A source discriminator prevents Vertex Google and Vertex Claude metadata from being confused. This is not a general transcript-repair layer.

Compaction deltas replace the completed summary and preserve nullable content and encrypted checkpoints according to the primary Anthropic contract. Both Chat stream adapters retain the block and later visible text for continuation. Invalid compaction values remain upstream failures rather than silently discarded replay.

Inline replay retains one ordered response-sized sequence. Terminal assembly moves that sequence into metadata rather than cloning it, releases parsed tool-input buffers, and discards failed snapshots. Disabling replay or truncating it would break continuation; a bounded or spooled wire contract is a separate design decision. Legacy reasoning fields and SSE serialization still require their own representation.

Output transforms invalidate replay state rather than expose an unmodified copy of redacted text or arguments. Truncated length/filter output can complete without a replay snapshot; malformed generation output fails while retaining observed usage. Native Messages and Responses remain separate transports. The [compatibility reference](../reference/provider-api-compatibility.md#native-chat-replay) defines the wire fields and limits.

The [pinned harness comparison](../internal/2026-11-05-provider-harness-comparison.md) records the accepted corrections, rejected claims and proof scope. Local provider fixtures prove wire behavior; they do not certify cloud credentials, model availability or a full agent session.
