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

Oceans uses pricing metadata from `models.dev` when it is available. Admins can refresh that metadata from the Models page with **Refresh pricing**.

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

Startup, scheduled refreshes, and manual refreshes own catalog reconciliation. Request-time accounting only looks up the persisted effective row for the selected provider, model, location, and billing shape. It does not fetch `models.dev`, read a catalog snapshot, or reconcile pricing rows while handling a request.

At request time, Oceans records the selected pricing provenance with the spend event, including the pricing provider, pricing model, copied rate fields, and pricing source metadata. That makes later reports explainable even if the external catalog has changed.

## Configure Routes For Chargeable Traffic

Route configuration affects pricing. The selected route tells Oceans which provider family and upstream model should be used for the pricing lookup.

Current exact pricing coverage is intentionally narrow:

- `openai_compat` routes need a supported `pricing_provider_id`
- OpenRouter `openai_compat` routes should use `pricing_provider_id: openrouter`
- Vertex routes are priced from the upstream publisher prefix
- `google/...` maps to Google Vertex pricing
- `anthropic/...` maps to Anthropic-on-Vertex pricing

Anthropic-on-Vertex pricing is supported only for `location=global`.

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

## Stored But Not Charged Yet

The catalog and provider responses can contain more billing signals than Oceans charges today.

| Catalog or provider signal | Current accounting status |
| --- | --- |
| prompt/input tokens | charged when exact pricing resolves |
| completion/output tokens | charged when exact pricing resolves |
| total tokens | stored for reporting and validation context |
| cache reads/writes | not charged yet |
| reasoning tokens or traces | not charged separately yet |
| image, audio, and file modality counters | not charged yet |

The current accounting model is limited to prompt/input tokens, completion/output tokens, and total tokens.

AWS Bedrock Anthropic Claude responses preserve the raw Anthropic usage object under `usage.provider_usage`, including cache counters such as `cache_read_input_tokens` and `cache_creation_input_tokens` when Bedrock returns them. Bedrock Claude thinking and Converse reasoning blocks are preserved as provider metadata on Chat Completions messages or stream deltas, but they are not priced as separate ledger dimensions.

Cache read/write discounts, hidden thinking costs, and reasoning-specific counters remain future accounting work tracked in [issue #92](https://github.com/ahstn/oceans-llm/issues/92).

## Budgets And Reporting

Budget enforcement uses priced totals only.

- `priced` and `legacy_estimated` rows count toward spend totals and budget windows.
- `unpriced` and `usage_missing` rows stay visible but do not consume hard or soft budgets.

For budget setup, alerting, and budget precedence, see [Budgets](../access/budgets.md).

For the full request path and failure behavior, see [Request Lifecycle and Failure Modes](../reference/request-lifecycle-and-failure-modes.md).
