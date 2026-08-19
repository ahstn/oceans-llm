# Pricing Catalog and Accounting

`See also`: [Configuration Reference](configuration-reference.md), [Budgets](../access/budgets.md), [Provider API Compatibility](../reference/provider-api-compatibility.md), [Request Lifecycle and Failure Modes](../reference/request-lifecycle-and-failure-modes.md)

This page explains how Oceans LLM turns provider usage into spend records, why some successful requests are visible but not charged, and what admins should check when model prices look stale or missing.

## What Admins Should Expect

Oceans LLM charges only when it has both:

- usage from the provider, such as input and output token counts
- an exact pricing match for the selected provider, model, location, and supported billing shape

When both are present, the request is recorded as `priced` and counts toward spend reports and hard or soft budget windows.

When either side is missing, the request still appears in reporting, but it does not consume budget:

- `usage_missing`: the provider response did not include usable usage counters
- `unpriced`: usage exists, but Oceans could not resolve an exact price safely

This is intentional. Oceans avoids approximate charging because approximate charges can make budget enforcement and audits misleading.

## Pricing Catalog Refresh

Oceans uses pricing metadata from `models.dev` when it is available. Platform admins can refresh that metadata from the Models page with **Refresh pricing**. The refresh endpoint is platform-admin-only.

A refresh updates the gateway's cached catalog snapshot and reconciles effective pricing rows for models with exact coverage. Scheduled refreshes run the same reconciliation workflow. The Models page then reloads so newly priced models can show input and output rates.

The gateway also keeps a vendored fallback catalog so a new environment can start when the remote catalog is unavailable. The fallback is a bootstrap and outage safety net, not a replacement for refreshing current pricing in a deployed environment.

Before the gateway starts serving requests, it materializes effective pricing rows from the cached snapshot or, when no cached snapshot is available, from the vendored fallback. This startup step ensures request accounting can use persisted pricing without depending on `models.dev` availability.

Refreshing the catalog does not rewrite old request charges. Historical spend keeps the rate that was resolved when the request was recorded.

## How Pricing Is Chosen

The runtime uses three layers:

1. a vendored normalized fallback snapshot
2. a cached normalized remote snapshot
3. effective-dated pricing rows used for request-time lookup

The effective-dated rows are what matter for durable accounting. They let Oceans keep older spend stable after an upstream catalog changes.

Startup, scheduled refreshes, and manual refreshes own catalog reconciliation. Request-time accounting first checks the selected route for an admin-authored pricing override. Without one, it looks up the persisted effective catalog row for the selected provider, model, location, and billing shape. It does not fetch `models.dev`, read a catalog snapshot, or reconcile pricing rows while handling a request.

At request time, Oceans copies the selected rates and provenance onto the spend event. Catalog-priced events include the pricing provider, pricing model, source, ETag, and fetched-at timestamp. Configured-price events identify `configured_override`, leave catalog-only identity and generation fields absent, and retain the selected route id. Historical spend therefore stays explainable and immutable after either catalog or route configuration changes.

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

Input and output rates are required when the block is present. Cache read and cache write rates are optional. Omitted cache rates remain absent rather than inheriting catalog values. Rates are exact non-negative Money4 strings; `"0"` and `"0.0000"` are valid authoritative prices.

Each input and output token subtotal is calculated with integer fixed-point arithmetic from the per-million rate, rounded half up to the nearest `$0.0001`, then added to the request cost. Floating-point rate values are rejected at configuration load so binary floating-point conversion cannot change an admin-authored rate.

An override takes precedence over catalog lookup for its route, including when a conflicting catalog row exists or no catalog row can be resolved. Catalog refresh does not modify overrides or historical spend events.

## Configure Routes For Chargeable Traffic

Route configuration affects pricing. The selected route tells Oceans which provider family and upstream model should be used for the pricing lookup.

Current exact catalog coverage, used only when a route has no pricing override, is intentionally narrow:

- `openai_compat` routes need a supported `pricing_provider_id`
- OpenRouter `openai_compat` routes should use `pricing_provider_id: openrouter`
- Vertex routes are priced from the upstream publisher prefix
- `google/...` maps to Google Vertex pricing
- `anthropic/...` maps to Anthropic-on-Vertex pricing
- `aws_bedrock` routes are priced against the `amazon-bedrock` provider family

Anthropic-on-Vertex pricing is supported only for `location=global`.

Bedrock pricing model ids are normalized from `upstream_model` before catalog lookup: ARN model ids resolve to their final path segment, `gpt-oss-120b` and `gpt-oss-20b` map to their catalog ids, and the default-version suffix `-v1:0` is stripped only for `claude-sonnet-4-6`, `claude-opus-4-6`, and `claude-opus-4-7`. Every other id resolves verbatim, so pinned versions outside those families need an exact catalog row to price.

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
| total tokens | stored for reporting and validation context |
| cache read/write rates | stored on spend events, propagated to generated client metadata, and used for positive normalized cache buckets |
| cache read/write token counts | retained raw and normalized for supported response shapes |
| reasoning tokens or traces | not charged separately yet |
| image, audio, and file modality counters | not charged yet |

AWS Bedrock Anthropic Claude responses preserve the raw Anthropic usage object under `usage.provider_usage`, including cache counters such as `cache_read_input_tokens` and `cache_creation_input_tokens` when Bedrock returns them. Oceans normalizes these totals when one write-price class is sufficient. Mixed 5-minute and 1-hour cache writes remain unpriced until the ledger and pricing catalog represent each class separately. Bedrock Claude thinking and Converse reasoning blocks are preserved as provider metadata on Chat Completions messages or stream deltas, but they are not priced as separate ledger dimensions.

TTL-specific cache-write accounting, hidden thinking costs, and reasoning-specific counters remain future work tracked in [issue #92](https://github.com/ahstn/oceans-llm/issues/92) and [issue #266](https://github.com/ahstn/oceans-llm/issues/266).

## Budgets And Reporting

Budget enforcement uses priced totals only.

- `priced` and `legacy_estimated` rows count toward spend totals and budget windows.
- `unpriced` and `usage_missing` rows stay visible but do not consume hard or soft budgets.

For budget setup, alerting, and budget precedence, see [Budgets](../access/budgets.md).

For the full request path and failure behavior, see [Request Lifecycle and Failure Modes](../reference/request-lifecycle-and-failure-modes.md).
