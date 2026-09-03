# OpenCode Zen

`See also`: [Configuration Reference](../configuration/configuration-reference.md), [Model Routing and APIs](../configuration/model-routing-and-api-behavior.md), [Provider API Compatibility](../reference/provider-api-compatibility.md), [Pricing Catalog and Accounting](../configuration/pricing-catalog-and-accounting.md)

This page explains how to configure OpenCode Zen and its `claude-fable-5-1` model in Oceans.

## Provider Configuration

OpenCode Zen exposes Anthropic-compatible endpoints under `https://opencode.ai/zen/v1/messages`. Configure Zen in Oceans using the `anthropic_compat` provider type with `base_url: https://opencode.ai/zen`. The adapter automatically targets `/v1/messages`.

```yaml
providers:
  - id: opencode-zen
    type: anthropic_compat
    base_url: https://opencode.ai/zen
    pricing_provider_id: opencode
    auth:
      kind: x_api_key
      token: env.OPENCODE_API_KEY
    default_headers:
      anthropic-version: "2023-06-01"
    display:
      label: OpenCode Zen
      icon_key: anthropic

models:
  - id: claude-fable-5-1
    description: Claude Fable 5.1 through OpenCode Zen
    max_reasoning_effort: max
    routes:
      - provider: opencode-zen
        upstream_model: claude-fable-5-1
        context_window_tokens: 1000000
        pricing_override:
          input_usd_per_million_tokens: "10.0000"
          output_usd_per_million_tokens: "50.0000"
          cache_read_usd_per_million_tokens: "0.2500"
          cache_write_usd_per_million_tokens: "12.5000"
        capabilities:
          chat_completions: true
          responses: false
          embeddings: false
          stream: true
          tools: true
          vision: true
          json_schema: false
          developer_role: false
```

### Key Settings

- **`base_url`**: `https://opencode.ai/zen` (without `/v1/messages`). The adapter appends `/v1/messages`.
- **`auth`**: `kind: x_api_key` sends the token in the `x-api-key` header from `OPENCODE_API_KEY`.
- **`pricing_provider_id`**: `opencode`.
- **`default_headers`**: `anthropic-version: "2023-06-01"`.
- **`upstream_model`**: `claude-fable-5-1`.
- **Model limits**: 1,000,000 context tokens and 128,000 output tokens.
- **Thinking policy**: Adaptive thinking is always enabled for `claude-fable-5-1`. Supported effort levels are `low`, `medium`, `high`, `xhigh`, and `max`. Default is `high`. Manual token budgets are rejected.
- **Tools**: Tool use is supported for `auto`, `none`, or omitted `tool_choice`. Forced tool selection (`tool_choice.type: "tool"` or `"any"`) is rejected for Fable 5.1.
- **Capabilities**: `chat_completions: true`, `stream: true`, `tools: true`, `vision: true`. `responses: false`, `embeddings: false`, and `json_schema: false`.

## Local Pi Client Configuration

Pi connects to Oceans, not directly to Zen. In `~/.pi/agent/models.json`:

```json
{
  "providers": {
    "oceans-llm": {
      "baseUrl": "https://llm.example.com",
      "api": "anthropic-messages",
      "apiKey": "$OCEANS_LLM_API_KEY",
      "compat": {
        "forceAdaptiveThinking": true
      },
      "models": [
        {
          "id": "claude-fable-5-1",
          "name": "Claude Fable 5.1",
          "reasoning": true,
          "input": [
            "text",
            "image"
          ],
          "contextWindow": 200000,
          "maxTokens": 128000,
          "cost": {
            "input": 10.0,
            "output": 50.0,
            "cacheRead": 0.25,
            "cacheWrite": 12.5
          },
          "thinkingLevelMap": {
            "off": null,
            "minimal": null,
            "low": "low",
            "medium": "medium",
            "high": "high",
            "xhigh": "xhigh",
            "max": "max"
          }
        }
      ]
    }
  }
}
```

Set the environment variable and run Pi:

```sh
export OCEANS_LLM_API_KEY='<Oceans gateway API key>'
pi --provider oceans-llm --model claude-fable-5-1
```

Note:
- Oceans caps generated client context at 200,000 tokens for Pi.
- The two credentials are separated: `OPENCODE_API_KEY` stays on the gateway and authenticates Oceans to Zen; `OCEANS_LLM_API_KEY` stays on the client and authenticates Pi to Oceans.
