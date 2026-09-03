# Anthropic-Compatible Provider Adapter and OpenCode Zen Support

## Status

Accepted.

## Context

Oceans supported OpenAI-compatible providers (`openai_compat`), Vertex AI publisher endpoints (`gcp_vertex`), AWS Bedrock (`aws_bedrock`), and GitHub Copilot (`github_copilot`). However, third-party providers such as OpenCode Zen host Claude models (e.g., `claude-fable-5-1`) over native Anthropic Messages HTTP endpoints at `https://opencode.ai/zen/v1/messages`.

Configuring OpenCode Zen as `openai_compat` fails because `openai_compat` always targets `/chat/completions`. Furthermore, pricing for OpenCode Zen routes requires recognizing `opencode` as an accepted pricing provider family, and client configuration generators (such as Pi) need support for the `max` reasoning effort level on adaptive-thinking Claude models.

## Decision

1. Add a first-class provider type named `anthropic_compat`.
   - Reuses and extracts core Anthropic request, response, and SSE streaming normalization into `crates/gateway-providers/src/anthropic/`.
   - Calls `{base_url}/v1/messages` (appending `/v1/messages` to `base_url`).
   - Supports `x-api-key` (default) and `bearer` authentication.
   - Forwards arbitrary default headers such as `anthropic-version`.
   - Normalizes usage tokens including cache read and cache creation/write tokens.
   - Enforces adaptive thinking policy for `claude-fable-5-1`, defaulting to `high` and supporting `low`, `medium`, `high`, `xhigh`, and `max`.
   - Rejects manual token budgets and forced `tool_choice` for `claude-fable-5-1`.
   - Preserves native thinking blocks across turns.
   - Advertises chat completion and streaming support, while disallowing responses and embeddings.
2. Add `opencode` to `SUPPORTED_PRICING_PROVIDER_IDS` in `gateway-service::pricing_catalog::target`.
3. Update `gateway-client-config` Pi template to map the `max` thinking level to `"max"` in `thinkingLevelMap`, while preserving the 200,000 token client context cap.

## Implementation

- `crates/gateway-providers/src/anthropic/`: Shared Anthropic request mapping, response parsing, SSE stream conversion, and thinking policies.
- `crates/gateway-providers/src/anthropic_compat.rs`: `AnthropicCompatProvider` implementing `ProviderClient`.
- `crates/gateway-service/src/pricing_catalog/target.rs`: Includes `opencode` in supported pricing provider IDs.
- `crates/gateway-service/src/admin_models.rs`: Defines `anthropic_compat` route capabilities.
- `crates/gateway/src/config/providers.rs` & `crates/gateway/src/config.rs`: Parses, validates, and seeds `anthropic_compat` configurations.
- `crates/gateway/src/main.rs`: Registers `AnthropicCompatProvider` in the runtime provider registry.
- `crates/gateway-client-config/src/templates/pi.rs`: Emits `"max": "max"` in Pi's `thinkingLevelMap`.
- `docs/providers/opencode-zen.md`: Documentation and configuration examples for OpenCode Zen and Pi.

## Trade-Offs

- Sharing Anthropic request/response parsing between Vertex and `anthropic_compat` reduces duplication, while keeping transport mechanisms (OAuth for Vertex, API key/bearer for `anthropic_compat`) decoupled.
- `claude-fable-5-1` enforces adaptive thinking at the provider boundary to fail fast with structured errors when callers provide unsupported manual budgets or forced tool choices.
