# Anthropic thinking compatibility research

Reviewed 2026-10-21 using Exa against Anthropic's API documentation and official Python SDK types. This is maintainer research, not a claim that every Claude Code workflow has been verified. The gateway's published contract is [Provider API Compatibility](../reference/provider-api-compatibility.md).

The reported request is valid for Claude Opus 5.5:

```json
{
  "thinking": { "type": "adaptive", "display": "summarized" },
  "output_config": { "effort": "high" }
}
```

It belongs on `POST /v1/messages`. `thinking.display` controls response visibility and does not specify a token budget. Rejecting that field as an unsupported budget is a gateway validation error. See the [Messages API](https://platform.claude.com/docs/en/api/messages) and [thinking configuration](https://platform.claude.com/docs/en/build-with-claude/thinking#configuring-thinking).

## Native request contract

| Field | Accepted form | Meaning |
| --- | --- | --- |
| `thinking.type` | `adaptive`, `enabled`, `disabled`, `between_tools` | Supported choices depend on the model. |
| `thinking.display` | `summarized`, `omitted`, null, or absent | Valid with adaptive or manual enabled thinking. Omission preserves the model's default. |
| `thinking.budget_tokens` | Integer at least 1,024 with `type: enabled` | Manual thinking budget; unsupported on adaptive-only models. |
| `output_config.effort` | `low`, `medium`, `high`, `xhigh`, `max`, null, or absent | Controls the whole response, including thinking when active. Availability of individual levels depends on the model. |

The [Messages schema](https://platform.claude.com/docs/en/api/messages), [adaptive SDK type](https://raw.githubusercontent.com/anthropics/anthropic-sdk-python/main/src/anthropic/types/thinking_config_adaptive_param.py), [enabled SDK type](https://raw.githubusercontent.com/anthropics/anthropic-sdk-python/main/src/anthropic/types/thinking_config_enabled_param.py), and [output SDK type](https://raw.githubusercontent.com/anthropics/anthropic-sdk-python/main/src/anthropic/types/output_config_param.py) agree on these shapes. Neither the GA nor [beta output type](https://raw.githubusercontent.com/anthropics/anthropic-sdk-python/main/src/anthropic/types/beta/beta_output_config_param.py) documents numeric effort or the strings `none`, `auto`, or `adaptive` as effort values.

Nullability applies to the inner `effort` and `display` fields. The top-level `thinking` and `output_config` fields are optional objects, without a documented null variant in the [request SDK type](https://raw.githubusercontent.com/anthropics/anthropic-sdk-python/main/src/anthropic/types/message_create_params.py). Passing those whole objects as null is not established as Anthropic-compatible behavior by these sources.

An omitted effort is valid and must remain omitted. The default is `medium` on Opus 5.5 and Haiku 5.5, and generally `high` elsewhere. Effort also works when thinking is disabled on a model that permits it. “No effort supplied” therefore does not mean “thinking disabled.” Injecting `high` changes the behavior and cost of an Opus 5.5 request. See [Effort](https://platform.claude.com/docs/en/build-with-claude/effort) and [Steering thinking](https://platform.claude.com/docs/en/build-with-claude/thinking-steering-and-cost).

The manual budget normally must be less than `max_tokens`; manual interleaved thinking is the documented exception. `max_tokens` is the hard output ceiling. Adaptive thinking does not need a synthesized budget. See [Extended thinking](https://platform.claude.com/docs/en/build-with-claude/extended-thinking#budget-rules-and-tuning).

## Model distinctions

| Model family | Omitted thinking | Explicit thinking modes | Effort levels |
| --- | --- | --- | --- |
| Opus 5.5 | Adaptive | Adaptive | All five; default medium |
| Opus 5 | Adaptive | Adaptive; disabled at high or below | All five; default high |
| Opus 4.7 / 4.8 | Off | Adaptive, disabled | All five; default high |
| Sonnet 5.5 | Adaptive | Adaptive; between_tools at high or below | All five; default high |
| Sonnet 5 | Adaptive | Adaptive, disabled | All five; default high |
| Haiku 5.5 | Adaptive | Adaptive; disabled at high or below | All five; default medium |
| Opus 4.6 / Sonnet 4.6 | Off | Adaptive, disabled, enabled (deprecated) | Low, medium, high, max |
| Opus 4.5 | Off | Enabled, disabled | Low, medium, high |
| Sonnet 4.5 / Haiku 4.5 | Off | Enabled, disabled | Unsupported |
| Fable 5 / 5.1, Mythos 5 / 5.1 | Adaptive | Adaptive | All five; default high |
| Mythos Preview | Adaptive | Adaptive, enabled | Low, medium, high, max |

Sources: [thinking support matrix](https://platform.claude.com/docs/en/build-with-claude/thinking-troubleshooting#thinking-support-defaults-and-rejected-configurations-by-model), [effort availability](https://platform.claude.com/docs/en/build-with-claude/effort#effort-levels).

In the gateway's [model classifier](../../crates/gateway-core/src/claude_models.rs), `AdaptiveOnly` means manual thinking budgets are unsupported. Separate model classifications determine whether disabling is allowed and which forced tool choices are supported. Those restrictions differ within the group. Qualified provider model IDs retain the same classification without treating unrelated suffixes as known model versions.

## Native fields and compatibility aliases

Native Messages fields and OpenAI-shaped aliases have different responsibilities:

- A native `output_config.effort` is forwarded without adding a thinking mode. Adaptive or enabled thinking retains its display setting, and absence or null of effort retains its meaning. Thinking-off modes normalize to the model's supported type-only form.
- `reasoning_effort` and `reasoning.effort` are gateway compatibility aliases. They request the existing model-aware translation to Anthropic controls. Manual-only models still need an explicit budget for that translation.
- Explicit alias/native disagreements remain errors. Model or alias reasoning-effort ceilings remain enforced before provider mapping; omission does not invent an effort value to validate against the ceiling.
- An invalid display value should report a display validation error. A budget error requires an actual manual budget or unsupported manual configuration.
- The gateway retains its model-specific thinking-off rules: always-on models reject disabling, Sonnet 5.5 maps `disabled` to `between_tools`, and models with a thinking-off effort ceiling reject `xhigh` and `max` in that mode. Other upstream model and beta restrictions remain authoritative; omitted effort never acquires another model's defaults.

The owning implementation is [Anthropic thinking compatibility](../../crates/gateway-providers/src/anthropic/thinking.rs). The [routing contract](../configuration/model-routing-and-api-behavior.md#enforce-reasoning-effort-ceilings) owns model and alias ceilings.

## Thinking output and beta extensions

`display: omitted` returns thinking blocks with an empty `thinking` string and a signature. Streaming includes an empty thinking delta followed by the signature delta. Preserve these blocks, signatures, and their order during tool continuations. Adaptive mode can produce no thinking block at any effort. Display affects visibility, not billed reasoning or whether thinking is enabled. See [Thinking](https://platform.claude.com/docs/en/build-with-claude/thinking#controlling-thinking-display).

Current documented extensions include:

- `display: updates`, with beta `thinking-display-updates-2026-08-18`. Reasoning text remains hidden while progress updates can be visible. Without the beta value, upstream rejects it like an unknown display value.
- `thinking.block_binding`, with beta `thinking-binding-controls-2026-08-01`, for supported models' handling of signed thinking after conversation-prefix changes.
- Per-message effort, with beta `per-message-effort-2026-08-01`, on supported models and platforms. It uses `output_config.effort` on a special system message in `messages`; its location determines when it takes effect.

See [progress updates](https://platform.claude.com/docs/en/build-with-claude/thinking#progress-updates-between-tool-calls), [signature validation](https://platform.claude.com/docs/en/build-with-claude/thinking-troubleshooting#a-400-error-says-a-thinking-block-signature-is-invalid), and [per-message effort](https://platform.claude.com/docs/en/build-with-claude/effort#per-message-effort-beta). Native adaptive objects must preserve extension fields for upstream validation. `between_tools` is different: the upstream accepts no extra thinking fields with that type.

The fetched SDK display docstring still states a universal summarized default. The current model-specific documentation overrides that description: Opus 5.5 defaults to omitted display.

## Claude Code auto mode and verification scope

[Claude Code auto mode](https://code.claude.com/docs/en/auto-mode-config) is a tool-permission classifier workflow. It is not an Anthropic effort enum. Its requests must be allowed to omit effort; that omission alone cannot prove the complete workflow is compatible.

The compatibility review also identified surfaces beyond display validation: `safeguards` request configuration and `safeguard_results` responses must survive native transport mapping, tool-use IDs must remain unchanged, and per-message effort must remain attached to its message when a request contains system messages. The [gateway protocol guide](https://code.claude.com/docs/en/llm-gateway-protocol) requires open-ended native header/body forwarding, and the [classifier billing guide](https://code.claude.com/docs/en/auto-mode-classifier-billing) explains why dropping safeguard fields causes fallback to separately billed classifier requests. These need separate request and response assertions across the supported transports. The reviewed guides do not define the complete safeguard payload schema or its exact SSE event placement; fixture coverage must preserve opaque values without inventing that contract.

No real Claude Code auto-mode session was run as part of this research. Local fixtures, provider request captures, and bounded live requests should report their own coverage without claiming a complete Claude Code workflow. In particular, check both streaming and non-streaming requests with every effort, omitted effort, display modes, manual-budget rejection, and signed thinking continuity.

Documentation validation from the repository root: `mise run //docs:build`. Runtime changes additionally require the repository's `verify-oceans-admin` evidence workflow.
