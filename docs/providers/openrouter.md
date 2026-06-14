# OpenRouter

`See also`: [Configuration Reference](../configuration/configuration-reference.md), [Model Routing and API Behavior](../configuration/model-routing-and-api-behavior.md), [Provider API Compatibility](../reference/provider-api-compatibility.md), [Pricing Catalog and Accounting](../configuration/pricing-catalog-and-accounting.md)

This page owns provider-specific configuration examples for OpenRouter routes.

## Current Runtime Boundary

OpenRouter is configured with the generic `openai_compat` provider type because its HTTP API is OpenAI-compatible. OpenRouter-specific provider-selection policy is route metadata under `compatibility.openrouter`; do not hide that policy in `extra_body.provider`.

Use generic `openai_compat` without `compatibility.openrouter` for arbitrary OpenAI-compatible endpoints. Use `compatibility.openrouter` only for routes that call `https://openrouter.ai/api/v1` and need OpenRouter's upstream provider routing controls.

## Provider

```yaml
providers:
  - id: openrouter
    type: openai_compat
    base_url: https://openrouter.ai/api/v1
    pricing_provider_id: openai
    auth:
      kind: bearer
      token: env.OPENROUTER_API_KEY
    display:
      label: OpenRouter
      icon_key: openrouter
```

## Route Policy

OpenRouter routes one requested model across upstream provider endpoints. That is OpenRouter behavior, not Oceans multi-route fallback. Oceans still selects one gateway route before provider execution.

```yaml
models:
  - id: openrouter-fast-zdr
    routes:
      - provider: openrouter
        upstream_model: openai/gpt-4o-mini
        compatibility:
          openrouter:
            provider:
              zdr: true
              only: [openai, anthropic]
              ignore: [deepinfra]
              order: [openai, anthropic]
              preferred_max_latency:
                p90: 2.5
              max_price:
                prompt: 1.0
                completion: 2.0
```

Policy fields:

- `zdr`: restricts routing to OpenRouter endpoints with Zero Data Retention.
- `only`: provider slugs OpenRouter may use.
- `ignore`: provider slugs OpenRouter must skip.
- `order`: preferred provider slug order. OpenRouter disables its default load balancing when ordered provider preference is set.
- `preferred_max_latency`: a preference, not a hard exclusion. Use a number or `p50`, `p75`, `p90`, and `p99` cutoffs in seconds.
- `max_price`: a hard ceiling. Supported dimensions are `prompt`, `completion`, `request`, and `image`.

Provider slugs must match OpenRouter's provider names. Oceans validates shape and conflicts but does not maintain an OpenRouter provider catalog.
