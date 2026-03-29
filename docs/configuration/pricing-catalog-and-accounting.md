# Pricing Catalog and Accounting

`Owns`: pricing catalog source layers, effective-dated pricing rows, exact-only coverage limits, and `unpriced` accounting behavior.
`Depends on`: [configuration-reference.md](configuration-reference.md), [data-relationships.md](../reference/data-relationships.md)
`See also`: [request-lifecycle-and-failure-modes.md](../reference/request-lifecycle-and-failure-modes.md), [budgets-and-spending.md](../operations/budgets-and-spending.md), [adr/2026-03-06-hybrid-pricing-catalog.md](../adr/2026-03-06-hybrid-pricing-catalog.md)

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
