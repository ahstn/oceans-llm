# Pricing Catalog and Accounting

`See also`: [Configuration Reference](configuration-reference.md), [Budgets](../access/budgets.md), [Provider API Compatibility](../reference/provider-api-compatibility.md), [Request Lifecycle and Failure Modes](../reference/request-lifecycle-and-failure-modes.md)

This page explains how Oceans LLM turns provider usage into spend records, why some successful requests are visible but not charged, and what admins should check when model prices look stale or missing.

## What Admins Should Expect

When the provider reports what it billed for a request, such as OpenRouter's `usage.cost`, Oceans LLM charges that amount; see [Provider-Reported Cost](#provider-reported-cost). Otherwise it charges only when it has both:

- provider usage: input token counts and output token counts
- an exact pricing match for the selected provider, model, location, and supported billing shape

When both are present, the request is recorded as `priced` and counts toward spend reports and hard or soft budget windows.

When either side is missing, the request still appears in reporting, but it does not consume budget:

- `usage_missing`: the provider response did not include usable usage counters
- `unpriced`: usage exists, but Oceans could not resolve an exact price safely

This is intentional. Oceans avoids approximate charging because approximate charges can make budget enforcement and audits misleading.

## Pricing Catalog Refresh

Oceans merges model prices, limits, and feature flags from three catalog sources:

| Source | What it adds |
| --- | --- |
| [models.dev](https://models.dev/api.json) | The main source for native providers, including `github-copilot` |
| [OpenRouter](https://openrouter.ai/api/v1/models?output_modalities=all) | The `openrouter` catalog, including image-only models and image output rates |
| [LiteLLM](https://github.com/BerriAI/litellm/blob/main/model_prices_and_context_window.json) | Limits, flags, API endpoints, and prices for exact OpenAI, GitHub Copilot, Bedrock, and Vertex model IDs |

For each model, each field comes from the first source that has a value: OpenRouter (only for `openrouter` models), then models.dev, then LiteLLM. An explicit `false` or zero counts as a value. Prices are one whole schedule from one source: the first source that has both an input and an output rate. Oceans never mixes rates from two sources. When sources disagree, `GET /v1/model-metadata` shows the conflict, but Oceans does not pick the smaller value. See [Model metadata discovery](../reference/request-lifecycle-and-failure-modes.md#model-metadata-discovery).

Each source refreshes when its snapshot is older than 6 hours. models.dev and LiteLLM send an ETag, so an unchanged catalog returns `304 Not Modified`. OpenRouter sends no ETag, so Oceans compares a checksum of the catalog instead. A catalog request times out after 5 seconds to connect or 30 seconds in total, and a response larger than the source's size limit is rejected. A source that fails keeps its last snapshot, and the other sources still refresh.

Admins can force a refresh of all three sources from the Models page with **Refresh pricing**. The message names each source and shows `updated`, `unchanged`, or `failed`. A failed source keeps its last snapshot. `POST /api/v1/admin/models/pricing-catalog/refresh` returns the same result per source, with the fetch time of the snapshot in use and a short error code such as `request_failed` or `response_too_large`.

After any source changes, Oceans rebuilds the merged catalog and reconciles effective pricing rows once for models with exact coverage. Rows that did not change are not rewritten. The Models page then reloads so newly priced models can show their rates.

The gateway also keeps a vendored fallback snapshot of each source, so a new environment can start when the remote catalogs are unavailable. The fallbacks are a bootstrap and outage safety net, not a replacement for refreshing current pricing in a deployed environment. Maintainers refresh them with `mise run //crates/gateway:sync-pricing-catalog`; the release task runs it.

Before the gateway starts serving requests, it refreshes stale sources, then materializes effective pricing rows from each source's cached snapshot or, when a source has no cached snapshot, from its vendored fallback. A source that cannot be reached does not stop startup.

Refreshing the catalog does not rewrite old request charges. Historical spend keeps the rate that was resolved when the request was recorded.

## How Pricing Is Chosen

The runtime uses three layers:

1. a vendored normalized fallback snapshot per source
2. a cached normalized remote snapshot per source, merged into one catalog
3. effective-dated pricing rows used for request-time lookup

The effective-dated rows are what matter for durable accounting. They let Oceans keep older spend stable after an upstream catalog changes.

Startup, scheduled refreshes, and manual refreshes own catalog reconciliation. Request-time accounting first uses a cost the provider reports in its usage. Without one, it checks the selected route for an admin-authored pricing override. Without an override, it looks up the persisted effective catalog row for the selected provider, model, location, and billing shape. It does not fetch a catalog source or reconcile pricing rows while handling a request.

Oceans copies the selected rates and provenance onto the spend event at request time. Catalog-priced events include the pricing provider and model. They also include the source that supplied the price, its ETag or checksum, and its fetch time. Configured-price events identify `configured_override` and omit catalog-only identity and generation fields. They also keep the selected route id. Provider-reported events identify `provider_reported` and store no rates. Historical spend remains explainable and immutable after catalog or route config changes.

## Context Tiers

Some models cost more when the prompt is long. For example, a model can charge a higher rate when the prompt is above 272,000 tokens. The catalog stores these rates as context tiers. Each tier has a threshold and its own input, output, cache read, and cache write rates.

Oceans bills a tier with these rules:

- A tier applies when the prompt tokens, cached tokens included, are greater than its threshold. A prompt of exactly the threshold uses the lower rates.
- When more than one tier applies, the tier with the highest threshold is used.
- The tier rates apply to the whole request, not only to the tokens above the threshold.
- When a tier does not set a rate, such as cache read, the base rate for that bucket is used.
- A provider-reported cost and a route `pricing_override` take precedence over the catalog, tiers included. An override has no tiers.
- Each item in a batch selects its tier from its own prompt tokens.

The spend event stores the threshold of the tier it used in `pricing_tier_above_input_tokens`. The value is empty for base rates, overrides, and provider-reported cost.

Tiers come from models.dev `tiers` entries of type `context`, and from LiteLLM `*_above_<N>k_tokens` prices. Oceans does not make a tier from the models.dev `context_over_200k` condition, because that condition often has the wrong threshold. Other price dimensions, such as priority, flex, and batch tiers, are not billed.

On the Models page, a model with tiers shows its base input and output rates as `from $X`. The model info dialog shows the base rates and one **Above N tokens** block for each tier.

## Provider-Reported Cost

OpenRouter reports what it billed for a request in `usage.cost`. When the usage has a `cost`, Oceans records it as the request cost with `pricing_source: provider_reported`. A reported cost takes precedence over route pricing overrides and catalog rates, so a negotiated rate or markup in `pricing_override` does not apply to responses that report a cost.

On bring-your-own-key requests (`usage.is_byok: true`), `cost` is only OpenRouter's fee, and the upstream provider bills `cost_details.upstream_inference_cost` to your key. Oceans records the sum of both. It reads the reported amounts as exact decimals, adds them, and rounds the total half up once to the nearest `$0.0001`.

A reported cost is charged even when the usage has no token counts. A malformed or negative cost, or a BYOK report without `cost_details.upstream_inference_cost`, is ignored with a warning, and the request is priced from token rates as usual.

## Route Pricing Overrides

Use `models[*].routes[*].pricing_override` when a contract, reseller, self-hosted deployment, or private provider has an authoritative rate that differs from or is absent in the external catalog:

```yaml
models:
  - id: contracted-model
    routes:
      - provider: private-provider
        upstream_model: upstream-model
        pricing_override:
          input_usd_per_million_tokens: "1.2500"
          output_usd_per_million_tokens: "5.0000"
          cache_read_usd_per_million_tokens: "0.1250"
          cache_write_usd_per_million_tokens: "1.5000"
```

Input and output rates are required when the block is present. Cache read, cache write, and image output rates are optional. Omitted optional rates remain absent rather than inheriting catalog values. Rates are exact non-negative Money4 strings; `"0"` and `"0.0000"` are valid authoritative prices.

Each token subtotal is calculated with integer fixed-point arithmetic from the per-million rate, rounded half up to the nearest `$0.0001`, then added to the request cost. Floating-point rate values are rejected at configuration load so binary floating-point conversion cannot change an admin-authored rate.

An override takes precedence over catalog lookup for its route, including when a conflicting catalog row exists or no catalog row can be resolved. A [provider-reported cost](#provider-reported-cost) takes precedence over the override. Catalog refresh does not modify overrides or historical spend events.

### Image Output Rate

Image models count the image tokens they generate as completion tokens and report them in `completion_tokens_details.image_tokens`, or `output_tokens_details.image_tokens` on Responses. Set `image_output_usd_per_million_tokens` to charge those tokens at the image rate and the remaining completion tokens at the output rate:

```yaml
pricing_override:
  input_usd_per_million_tokens: "0.2500"
  output_usd_per_million_tokens: "1.5000"
  image_output_usd_per_million_tokens: "30.0000"
```

Without an override, Oceans uses the catalog image output rate when the catalog has one. OpenRouter and LiteLLM publish image output rates; models.dev does not. Without an image rate, every completion token, image tokens included, is charged at the output rate. The spend event stores the image token count and the image rate it used. Batch pricing snapshots keep the image rate and apply the same split before the batch discount. An image token count that is not a whole number from zero to the completion tokens is ignored with a warning, and every completion token is charged at the output rate.

## Configure Routes For Chargeable Traffic

Route configuration affects pricing. The selected route tells Oceans which provider family and upstream model should be used for the pricing lookup.

Current exact catalog coverage, used only when a route has no pricing override, is intentionally narrow:

- `openai_compat` and `anthropic_compat` routes need a supported `pricing_provider_id`: `openai`, `openrouter`, `opencode`, `github-copilot`, `amazon-bedrock`, `google-vertex`, or `google-vertex-anthropic`
- OpenRouter `openai_compat` routes should use `pricing_provider_id: openrouter`
- native `typesafe` routes need a supported `pricing_provider_id` and should use a route pricing override when their upstream alias has no exact catalog identity
- OpenCode Zen `anthropic_compat` routes should use `pricing_provider_id: opencode`
- GitHub Copilot routes should use `pricing_provider_id: github-copilot`, which matches Copilot model IDs such as `claude-haiku-4.5`
- AWS Bedrock routes are priced as `amazon-bedrock` from the upstream model ID
- Vertex routes are priced from the upstream publisher prefix
- `google/...` maps to Google Vertex pricing
- `anthropic/...` maps to Anthropic-on-Vertex pricing

Anthropic-on-Vertex pricing is supported only for `location=global`.

The catalog lists most Anthropic-on-Vertex models under a versioned id, such as `claude-fable-5-1@default` or `claude-haiku-4-5@20251001`. Oceans looks up the exact upstream model first. If an unversioned id such as `anthropic/claude-fable-5-1` has no exact entry, Oceans tries `<model>@default`. Dated releases without a `@default` entry, such as `claude-sonnet-4-5`, must name the dated id (`anthropic/claude-sonnet-4-5@20250929`) or use a pricing override.

If a route changes provider, upstream model, location, or a billing modifier such as `service_tier`, review pricing behavior before relying on budget enforcement for that traffic.

## When Requests Are Not Charged

A successful provider response can still become `unpriced`.

Common causes are:

- no supported pricing source for the provider
- missing or unsupported `pricing_provider_id`
- unknown pricing model id
- unsupported Vertex publisher family
- unsupported Vertex location
- unsupported billing modifier such as `service_tier`
- missing exact input or output token rate

`usage_missing` is different. It means Oceans could not normalize usable provider usage at all. For example, a provider may return a successful response without final token counts.

Both states remain visible in reports and request logs, but neither state counts toward spend totals or hard-limit windows.

## Decisions Usage

System One Decisions responses report `input_tokens` and `output_tokens`. Oceans normalizes them to prompt/input and completion/output ledger fields and derives a total when both values are present.

OpenAI Decisions responses also report `input_tokens_details.cached_tokens`, `input_tokens_details.cache_write_tokens`, and `output_tokens_details.reasoning_tokens`. Cached input is priced through the same inclusive cache buckets as the Responses API, and reasoning tokens stay within completion tokens. Oceans always prices the upstream's own usage report, so cached tokens are counted even when a System One caller receives the response.

OpenAI bills Decisions on input only. When a model such as `gpt-6-luna` has no exact catalog entry, set a route `pricing_override` with OpenAI's published input and cached-input rates and a zero output rate:

```yaml
pricing_override:
  input_usd_per_million_tokens: "<OpenAI input rate>"
  cache_read_usd_per_million_tokens: "<OpenAI cached input rate>"
  output_usd_per_million_tokens: "0"
```

The vendored OpenRouter catalog includes `typesafe/jev-1.13` at $0.042 input and $0 output per million tokens with a 262,144-token context. A native TypeSafe route commonly uses `jev-latest`, which is not the same catalog key. Configure the known rates on that route instead of relying on alias inference.

## Vertex Text Embeddings

Native Vertex text embeddings use the same exact pricing resolver as other Vertex traffic:

| Upstream model | Pricing provider | Pricing model |
| --- | --- | --- |
| `google/gemini-embedding-001` | `google-vertex` | `gemini-embedding-001` |
| `google/gemini-embedding-2` | `google-vertex` | `gemini-embedding-2` |
| `google/text-embedding-005` | `google-vertex` | `text-embedding-005` |
| `google/text-multilingual-embedding-002` | `google-vertex` | `text-multilingual-embedding-002` |

Oceans charges only from real provider token usage. Vertex `:predict` text embeddings expose token usage through `predictions[].embeddings.statistics.token_count`; `google/gemini-embedding-2` exposes it through `usageMetadata.promptTokenCount` on `:embedContent`. Oceans aggregates those values for array input and records them as prompt/input tokens.

Oceans does not infer embedding token counts from characters, bytes, vector dimensions, or input count.

Embedding pricing in this slice is input-token based. `dimensions` and its `output_dimensionality` or `outputDimensionality` aliases affect the provider request and resulting vectors, but they do not select a different pricing key or billing modifier. For `:predict` models, `task_type`, `input_type`, `title`, and `auto_truncate` are request-shaping fields, not pricing keys.

If a future provider rate differentiates one of those dimensions, Oceans must model that modifier explicitly before charging for it.

## Streaming Usage

Some OpenAI-compatible providers only return streaming usage when the request includes `stream_options.include_usage = true`.

Routes can opt into that request shape with `compatibility.openai_compat.supports_stream_usage`. This improves usage capture for providers that support it, but it is not a billing guarantee:

- providers may still omit final usage
- provider-specific counters may not fit Oceans accounting
- successful requests can still become `usage_missing` or `unpriced`

This compatibility option applies to Chat Completions streams. Responses streams use the Responses event model and read usage from completed response events.

## Cache-Aware Token Accounting

Oceans normalizes cache usage when a supported response supplies enough evidence to form disjoint input buckets. The ledger retains the raw provider usage and stores:

- uncached input tokens;
- cache-read tokens;
- cache-write tokens.

OpenAI Responses `input_tokens_details` and equivalent root provider counters use inclusive input totals. Bedrock Converse and Anthropic counters use exclusive uncached-input totals, so Oceans adds cache-read and cache-write tokens when it derives the logical prompt total. Missing cache evidence remains `NULL`; reports show the aggregate as unavailable if any included row lacks all three normalized buckets.

Cache-aware pricing requires the standard input and output rates and each positive cache bucket's route rate. Malformed counters use aggregate input/output pricing as a `legacy_estimated` safety fallback and retain an `invalid_cache_token_usage` reason. Unsupported mixed TTL write classes remain unpriced because one cache-write rate cannot price them without loss.

The catalog and provider responses can contain more billing signals than Oceans charges today.

| Catalog, override, or provider signal | Current accounting status |
| --- | --- |
| prompt/input tokens | charged from the effective input rate |
| completion/output tokens | charged from the effective output rate |
| context tiers | the whole request is charged at the tier's rates when the prompt is above the tier threshold |
| total tokens | stored for reporting and validation context |
| cache read/write rates | stored on spend events, propagated to generated client metadata, and used for positive normalized cache buckets |
| cache read/write token counts | retained raw and normalized for supported response shapes |
| image output tokens | charged from the route's image output rate, else the catalog image output rate; otherwise charged with the other completion tokens at the output rate |
| provider-reported cost (`usage.cost`, plus `cost_details.upstream_inference_cost` for BYOK) | charged as the request cost, ahead of overrides and catalog rates |
| reasoning tokens or traces | not charged separately yet |
| image input, audio, and file modality counters | not charged separately yet |

AWS Bedrock Anthropic Claude responses preserve the raw Anthropic usage object under `usage.provider_usage`, including cache counters such as `cache_read_input_tokens` and `cache_creation_input_tokens` when Bedrock returns them. Oceans normalizes these totals when one write-price class is sufficient. Mixed 5-minute and 1-hour cache writes remain unpriced until the ledger and pricing catalog represent each class separately. Bedrock Claude thinking and Converse reasoning blocks are preserved as provider metadata on Chat Completions messages or stream deltas, but they are not priced as separate ledger dimensions.

TTL-specific cache-write accounting, hidden thinking costs, and reasoning-specific counters remain future work tracked in [issue #92](https://github.com/ahstn/oceans-llm/issues/92) and [issue #266](https://github.com/ahstn/oceans-llm/issues/266).

## Budgets And Reporting

Budget enforcement uses priced totals only.

- `priced` and `legacy_estimated` rows count toward spend totals and budget windows.
- `unpriced` and `usage_missing` rows stay visible but do not consume hard or soft budgets.

For budget setup, alerting, and budget precedence, see [Budgets](../access/budgets.md).

For the full request path and failure behavior, see [Request Lifecycle and Failure Modes](../reference/request-lifecycle-and-failure-modes.md).
