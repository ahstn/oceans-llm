# OpenRouter

`See also`: [Configuration Reference](../configuration/configuration-reference.md), [Model Routing and APIs](../configuration/model-routing-and-api-behavior.md), [Provider API Compatibility](../reference/provider-api-compatibility.md), [Pricing Catalog and Accounting](../configuration/pricing-catalog-and-accounting.md)

This page shows how to set up OpenRouter routes.

## Current Runtime Boundary

Configure OpenRouter with the generic `openai_compat` provider type. Its HTTP API uses the OpenAI format. Put its provider policy in route data under `compatibility.openrouter`. Do not hide that policy in `extra_body.provider`.

Use generic `openai_compat` without `compatibility.openrouter` for other OpenAI-compatible endpoints. Use `compatibility.openrouter` only for routes that call `https://openrouter.ai/api/v1`. These routes can use OpenRouter's upstream routing controls.

## Provider

```yaml
providers:
  - id: openrouter
    type: openai_compat
    base_url: https://openrouter.ai/api/v1
    pricing_provider_id: openrouter
    auth:
      kind: bearer
      token: env.OPENROUTER_API_KEY
    display:
      label: OpenRouter
      icon_key: openrouter
```

## Route Policy

OpenRouter can route one model across upstream providers. This is an OpenRouter feature, not Oceans route fallback. Oceans still selects one gateway route first.

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

- `zdr`: limits routing to OpenRouter endpoints with Zero Data Retention.
- `only`: provider slugs OpenRouter may use.
- `ignore`: provider slugs OpenRouter must skip.
- `order`: preferred provider slug order. OpenRouter turns off its default load balancing when the policy sets an order.
- `preferred_max_latency`: a preference, not a hard limit. Use a number or `p50`, `p75`, `p90`, and `p99` cutoffs in seconds.
- `max_price`: a hard ceiling. Supported dimensions are `prompt`, `completion`, `request`, and `image`.

Provider slugs must match OpenRouter's provider names. Oceans checks the data shape and conflicts. It does not maintain an OpenRouter provider catalog.
