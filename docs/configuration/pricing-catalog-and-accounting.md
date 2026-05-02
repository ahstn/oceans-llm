# Pricing Catalog and Accounting

`See also`: [Configuration Reference](configuration-reference.md), [Provider API Compatibility](../reference/provider-api-compatibility.md), [Data Relationships](../reference/data-relationships.md), [Request Lifecycle and Failure Modes](../reference/request-lifecycle-and-failure-modes.md), [Budgets and Spending](../operations/budgets-and-spending.md), [ADR: Hybrid Pricing Catalog from models.dev](../adr/2026-03-06-hybrid-pricing-catalog.md), [ADR: Route-Level Provider API Compatibility Profiles](../adr/2026-04-23-route-level-provider-api-compatibility-profiles.md)

This page explains how the gateway turns provider usage into durable pricing records and why some successful requests are intentionally not charged.

## Source of Truth

- pricing resolution and refresh logic:
  - [../crates/gateway-service/src/pricing_catalog.rs](../../crates/gateway-service/src/pricing_catalog.rs)
- spend ledger writes:
  - [../crates/gateway-service/src/service.rs](../../crates/gateway-service/src/service.rs)
- cache and pricing-row persistence:
  - [../crates/gateway-store/src/libsql_store/pricing_catalog.rs](../../crates/gateway-store/src/libsql_store/pricing_catalog.rs)
  - [../crates/gateway-store/src/postgres_store/pricing_catalog.rs](../../crates/gateway-store/src/postgres_store/pricing_catalog.rs)
- vendored fallback snapshot:
  - [../crates/gateway-service/data/pricing_catalog_fallback.json](../../crates/gateway-service/data/pricing_catalog_fallback.json)

## Catalog Layers

The runtime does not price directly from a live remote response on every request.

It uses three layers:

1. vendored normalized fallback snapshot in the repo
2. cached normalized remote snapshot in `pricing_catalog_cache`
3. effective-dated `model_pricing` rows used for historical lookup

That split keeps historical spend totals stable after an upstream catalog changes.

## Upstream Source and Refresh

Current normalized pricing input comes from `models.dev`.

Operational shape:

- the runtime can refresh pricing metadata from the upstream feed
- cached snapshots are persisted in `pricing_catalog_cache`
- the vendored fallback keeps the repo bootstrappable when the remote source is unavailable

The durable historical charging source is still `model_pricing`, not the cache row.

## Historical Pricing Contract

Pricing is effective-dated.

At request time, the gateway resolves one pricing row and copies provenance into `usage_cost_events`, including:

- `pricing_row_id`
- `pricing_provider_id`
- `pricing_model_id`
- copied rate fields
- pricing source metadata

## Supported Pricing Paths

Current exact-only coverage is intentionally narrow:

- `openai_compat` requires a supported `pricing_provider_id`
- Vertex pricing is inferred from the upstream publisher prefix
- `google/...` maps to Google Vertex pricing
- `anthropic/...` maps to Anthropic-on-Vertex pricing

Known coverage constraint:

- Anthropic-on-Vertex pricing is only supported for `location=global`

## Why Requests Become `unpriced`

A request can succeed and still become `unpriced`.

Common causes:

- missing provider pricing source
- unsupported `pricing_provider_id`
- unknown pricing model id
- unsupported Vertex publisher family
- unsupported Vertex location
- unsupported billing modifiers such as `service_tier`
- missing exact input or output rate coverage

This is fail-closed accounting behavior. Approximate billing is intentionally avoided in this slice.

## `usage_missing` Versus `unpriced`

- `usage_missing`
  - provider usage could not be normalized
- `unpriced`
  - usage exists, but exact pricing could not be resolved safely

Both states stay visible in reporting, but neither counts toward spend totals or hard-limit windows.

## Streaming Usage and Compatibility

Some OpenAI-compatible providers only emit streaming usage when the request includes `stream_options.include_usage = true`.

Routes can opt into that request shape with `compatibility.openai_compat.supports_stream_usage`. This improves usage capture for providers that support it, but it is not a billing guarantee:

- providers may omit final usage despite the option
- provider-specific usage counters may not fit the gateway accounting model
- successful requests can still become `usage_missing` or `unpriced`

This compatibility option is Chat Completions-specific. Responses streams use the Responses event model and read usage from completed response events with `response.usage`.

The accounting model remains limited to prompt/input tokens, completion/output tokens, and total tokens in this slice.

## Stored But Not Charged Yet

The pricing catalog can preserve more rate metadata than the runtime charges today.

| Catalog or provider signal | Current accounting status |
| --- | --- |
| prompt/input tokens | charged when exact pricing resolves |
| completion/output tokens | charged when exact pricing resolves |
| total tokens | stored for reporting and validation context |
| cache reads/writes | not charged yet |
| reasoning tokens or traces | not charged separately yet |
| image, audio, and file modality counters | not charged yet |

That distinction keeps the ledger conservative. The gateway should not infer spend for provider-specific counters until the pricing and request semantics are explicit. Richer token and cache accounting is tracked in [issue #92](https://github.com/ahstn/oceans-llm/issues/92).

AWS Bedrock Anthropic Claude responses preserve the raw Anthropic usage object under `usage.provider_usage`, including cache counters such as `cache_read_input_tokens` and `cache_creation_input_tokens` when Bedrock returns them. Bedrock Claude thinking and Converse reasoning blocks are preserved as provider metadata on Chat Completions messages or stream deltas, but they are not priced as separate ledger dimensions. Durable accounting still uses only normalized `prompt_tokens`, `completion_tokens`, and `total_tokens`; cache read/write discounts, hidden thinking costs, and reasoning-specific counters remain aligned with [issue #92](https://github.com/ahstn/oceans-llm/issues/92).

## Relationship to Request Flow

Route choice affects accounting, not only provider execution.

- the chosen provider decides the pricing family
- the chosen upstream model decides the exact lookup key
- route or request modifiers can make a request become `unpriced`

Use [request-lifecycle-and-failure-modes.md](../reference/request-lifecycle-and-failure-modes.md) for the full cause-and-effect path.

## Relationship to Budgets

Budget enforcement only uses priced totals.

- `priced` and `legacy_estimated` rows count
- `unpriced` and `usage_missing` rows do not count

Use [budgets-and-spending.md](../operations/budgets-and-spending.md) for budget windows and spend APIs.
