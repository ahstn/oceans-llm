# ADR: Route-Level Effective Model Metadata Overrides

- Date: 2026-07-16
- Status: Accepted

## Current state

- [Configuration Reference](../configuration/configuration-reference.md#route-metadata-overrides)
- [Model Routing and API Behavior](../configuration/model-routing-and-api-behavior.md#effective-route-metadata)
- [Pricing Catalog and Accounting](../configuration/pricing-catalog-and-accounting.md#route-pricing-overrides)
- [Client Harness Configuration](../configuration/client-harness-configuration.md)

## Context

The external pricing catalog describes upstream list prices and advertised model limits. It cannot represent deployment policy such as negotiated rates, self-hosted costs, reseller pricing, or a deliberately constrained context window. Editing downloaded catalog data would mix admin policy with external metadata and would be unsafe across refreshes.

Issues [#242](https://github.com/ahstn/oceans-llm/issues/242) and [#243](https://github.com/ahstn/oceans-llm/issues/243) require route-specific effective metadata that remains auditable through routing, accounting, admin APIs, generated client configuration, and MCP telemetry.

## Decision

### 1. Deployment-specific metadata is route-scoped

Each model route may declare:

- `context_window_tokens`, a positive effective context cap
- `pricing_override`, with required input/output rates and optional cache-read/cache-write rates in USD per million tokens

The typed values are stored on `model_routes`, independently of downloaded catalog rows. Different routes to the same upstream model may therefore expose different deployment contracts.

### 2. Configured pricing takes precedence over catalog pricing

When a selected route has `pricing_override`, request accounting uses that complete override and does not consult the catalog. Optional cache rates remain absent when omitted; they do not inherit catalog cache rates. Routes without an override retain the existing exact catalog lookup and unpriced-reason behavior.

Catalog refresh only manages catalog cache and effective-dated catalog rows. It cannot mutate route overrides.

### 3. Pricing configuration uses exact quoted decimal strings

Rates are non-negative `Money4` values expressed as quoted YAML strings with at most four fractional digits. YAML numeric scalars are rejected instead of being coerced through floating-point representation. Zero is a valid authoritative rate.

Input and output token subtotals use integer fixed-point arithmetic and round half up to the nearest `$0.0001` before they are added. Cache rates are persisted and exposed to clients, but cache token counters are not charged until the accounting model explicitly supports them.

### 4. Spend events snapshot selected route pricing and provenance

Every priced usage event retains:

- the selected route id
- the concrete input/output/cache rates used
- `configured_override` provenance for configured pricing
- no catalog row, provider, model, ETag, or fetched-at identity when catalog pricing was bypassed

This snapshot is immutable. Later route edits and catalog refreshes cannot rewrite historical charges, reporting, or budget consumption.

### 5. Effective context is conservative

The effective route context is the configured cap when catalog context is absent or greater than the configured value. A known smaller catalog context remains authoritative. Startup rejects a configured cap that exceeds the catalog context after startup catalog reconciliation; later refreshes warn about newly discovered conflicts and continue publishing the smaller catalog limit.

The effective context caps catalog input and output dimensions conservatively. If any selectable route lacks a known dimension, the logical-model aggregate remains unknown rather than advertising an unsafe maximum.

This limit is advertised metadata in the current implementation. The gateway does not count request tokens or reject an oversized request before provider dispatch.

### 6. Effective metadata has explicit provenance

Admin model data exposes effective pricing and context sources as `catalog` or `configured_override`, including catalog source metadata when applicable. Generated client configurations consume effective rates and conservative logical-model limits. MCP token-overhead telemetry records the selected route's effective context.

Catalog modalities remain optional in the effective metadata model. Absence means no catalog record was resolved; it is not represented by an empty modality list.

## Consequences

Benefits:

- negotiated and private-provider traffic participates in deterministic spend accounting and hard budgets
- external catalog refresh remains isolated from admin policy
- historical spend stays explainable after both configuration and catalog changes
- generated clients and MCP telemetry use the same deployment-specific context contract
- aliases and multi-route logical models retain conservative aggregate limits

Trade-offs:

- pricing values must be quoted in YAML, which is stricter than ordinary string coercion
- route metadata adds storage and admin-contract fields across both database backends
- a newly reduced catalog context discovered after startup produces a warning and a conservative effective limit; admins must correct configuration before the next restart
- cache rates are visible before cache token usage participates in gateway-side charging

## Follow-up work

- Add canonical request-time token-count preflight enforcement only after a tokenizer contract exists.
- Charge cache-read/cache-write usage only after provider counters are normalized into explicit ledger dimensions.
- Add admin mutation APIs if route configuration becomes editable outside declarative seed configuration.

## Attribution

This ADR was prepared through collaborative human + AI implementation/design work.
