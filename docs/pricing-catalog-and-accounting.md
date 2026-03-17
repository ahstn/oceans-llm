# Pricing Catalog and Accounting

`Owns`: pricing catalog sources, effective-dated pricing rows, pricing coverage limits, and unpriced-accounting behavior.
`Depends on`: [configuration-reference.md](configuration-reference.md), [data-relationships.md](data-relationships.md)
`See also`: [budgets-and-spending.md](budgets-and-spending.md), [model-routing-and-api-behavior.md](model-routing-and-api-behavior.md), [adr/2026-03-06-hybrid-pricing-catalog.md](adr/2026-03-06-hybrid-pricing-catalog.md)

This page explains how the gateway turns provider usage into durable pricing records and why some successful requests are intentionally not charged.

## Source of Truth

- Pricing resolution and refresh logic: [../crates/gateway-service/src/pricing_catalog.rs](../crates/gateway-service/src/pricing_catalog.rs)
- Spend ledger writes: [../crates/gateway-service/src/service.rs](../crates/gateway-service/src/service.rs)
- Cache and pricing-row persistence:
  - [../crates/gateway-store/src/libsql_store/pricing_catalog.rs](../crates/gateway-store/src/libsql_store/pricing_catalog.rs)
  - [../crates/gateway-store/src/postgres_store/pricing_catalog.rs](../crates/gateway-store/src/postgres_store/pricing_catalog.rs)
- Vendored fallback snapshot: [../crates/gateway-service/data/pricing_catalog_fallback.json](../crates/gateway-service/data/pricing_catalog_fallback.json)

## Catalog Layers

The runtime does not price directly from a live remote response on every request. It uses a layered model:

1. vendored normalized fallback snapshot in the repo
2. cached normalized remote snapshot in `pricing_catalog_cache`
3. effective-dated `model_pricing` rows used for historical pricing lookup at request time

That split is why past spend totals remain stable when the upstream catalog changes later.

## Upstream Source And Refresh

Current pricing input is normalized from `models.dev`.

Operational shape:

- the runtime can refresh pricing metadata from the upstream feed
- cached snapshots are persisted in `pricing_catalog_cache`
- the vendored fallback keeps the repo bootstrappable even when the remote source is unavailable

The cache is an input to pricing maintenance. The durable pricing source for historical charging is `model_pricing`.

## Historical Pricing Contract

Pricing is effective-dated.

At request time, the gateway resolves a pricing row and copies pricing provenance into `usage_cost_events`, including:

- `pricing_row_id`
- `pricing_provider_id`
- `pricing_model_id`
- copied rate fields
- pricing source metadata

This is the mechanism that keeps historical ledger math stable.

## Supported Pricing Paths

Current exact-only coverage is intentionally narrow:

- `openai_compat` requires a supported `pricing_provider_id`
- Vertex pricing is inferred from the upstream publisher prefix
- `google/...` maps to Google Vertex pricing
- `anthropic/...` maps to Anthropic-on-Vertex pricing

Known coverage constraint:

- Anthropic-on-Vertex pricing is only supported for `location=global`

## Why Requests Become Unpriced

A request can succeed and still become `unpriced`.

Current important causes include:

- missing provider pricing source
- unsupported `pricing_provider_id`
- unknown pricing model id
- unsupported Vertex publisher family
- unsupported Vertex location
- unsupported billing modifiers such as `service_tier` / `serviceTier`
- missing exact input or output rate coverage

This is fail-closed accounting behavior. The runtime prefers an explicit `unpriced` row over approximate billing.

## `usage_missing` vs `unpriced`

- `usage_missing`: provider usage could not be normalized
- `unpriced`: usage exists, but exact pricing could not be resolved safely

Both states are visible in reporting, but neither counts toward spend totals or hard-limit windows.

That means operators can see successful requests with zero charged spend without assuming the request was dropped.

## Relationship To Routing

Route choice affects accounting, not just provider execution.

Important examples:

- the chosen provider decides the pricing family
- the chosen upstream model decides the exact pricing lookup key
- route/request modifiers can intentionally make a request unpriced

For request resolution behavior, see [model-routing-and-api-behavior.md](model-routing-and-api-behavior.md).

## Relationship To Budgets

Budget enforcement only uses priced totals.

That means:

- `priced` and `legacy_estimated` rows count
- `unpriced` and `usage_missing` rows do not count

Budget-window and admin API behavior is owned by [budgets-and-spending.md](budgets-and-spending.md).
