# Route Pricing and Context Overrides Interview

`See also`: [Model Routing and APIs](../configuration/model-routing-and-api-behavior.md), [Pricing Catalog and Accounting](../configuration/pricing-catalog-and-accounting.md), [ADR: Route-Level Effective Model Metadata Overrides](../adr/2026-07-16-route-level-effective-model-metadata-overrides.md)

- Date: 2026-07-16
- Status: Confirmed for implementation
- Issues: [#243](https://github.com/ahstn/oceans-llm/issues/243), [#242](https://github.com/ahstn/oceans-llm/issues/242), and absorbed [#237](https://github.com/ahstn/oceans-llm/issues/237)

## Purpose

This interview defines the implementation contract for route-level pricing overrides and context-window overrides. It also absorbs #237 because the confirmed pricing schema includes cache-write metadata that must reach generated Pi configuration.

The design keeps admin policy separate from the downloaded pricing catalog. It preserves exact fixed-point accounting, snapshots applied rates on usage events, and does not add approximate request token enforcement.

## Context Reviewed

The interview used the following sources:

- `README.md`
- GitHub issues #243, #242, and #237
- `crates/gateway/src/config.rs`
- `crates/gateway-core/src/domain.rs`
- `crates/gateway-core/src/traits.rs`
- `crates/gateway-service/src/pricing_catalog.rs`
- `crates/gateway-service/src/admin_models.rs`
- `crates/gateway-service/src/mcp_token_overhead.rs`
- `crates/gateway-service/src/service.rs`
- `crates/gateway-store/migrations/`
- libsql and PostgreSQL model-route and usage-event persistence
- `crates/gateway-client-config/`
- the admin models API and Models page
- pricing, routing, configuration, and client-harness documentation

## Existing Constraints

The current system establishes several constraints for this work:

- Route configuration is parsed in `gateway`, converted to `SeedModelRoute`, persisted by `gateway-store`, and loaded as `ModelRoute`.
- Catalog pricing is effective-dated and resolved by provider, upstream model, location, and billing shape.
- Usage events already snapshot catalog rate and provenance fields so catalog refresh cannot rewrite historical charges.
- Usage cost currently uses prompt and completion token totals only. Cache-read and cache-write rates exist in catalog records, but canonical cache-token accounting does not.
- Admin model data and generated client configurations currently use one display route.
- MCP overhead persistence already accepts `context_window_tokens`, but request handling currently passes `None`.
- `/v1/models` intentionally returns a narrow OpenAI-compatible model card.
- `GatewayConfig::from_path` is synchronous and cannot validate a configured context cap against persisted catalog state.

## Confirmed Configuration Contract

A route may declare an optional context cap and optional pricing override:

```yaml
models:
  - id: contracted-gpt
    routes:
      - provider: openai-prod
        upstream_model: gpt-5
        context_window_tokens: 128000
        pricing_override:
          input_usd_per_million_tokens: "1.2500"
          output_usd_per_million_tokens: "10.0000"
          cache_read_usd_per_million_tokens: "0.1250"
          cache_write_usd_per_million_tokens: "1.2500"
```

### Pricing validation

- `input_usd_per_million_tokens` and `output_usd_per_million_tokens` are required when `pricing_override` is present.
- Cache-read and cache-write rates are optional.
- Rates are exact decimal strings, not YAML floating-point numbers.
- Values use the existing `Money4` convention and allow at most four fractional digits.
- Zero is valid and represents an explicit free rate.
- Negative values, malformed decimals, overflow, missing base rates, and unknown override fields are rejected.
- An omitted cache rate remains absent. It does not fall back to the external catalog and is not treated as zero inside the effective route metadata.
- Existing provider-level `pricing_provider_id` requirements remain unchanged.

### Context validation

- `context_window_tokens` is an integer token count.
- It must be positive.
- YAML parsing and `seed-config` validate only local shape and ranges.
- Every serving startup validates persisted route caps after catalog materialization and before binding the HTTP server.
- Startup rejects a configured cap larger than a known catalog context limit.
- A configured cap is accepted when no catalog context is known.
- Explicit scheduling fields are not included. The seeded configuration is active until replaced.

## Effective Route Metadata

Effective metadata is resolved centrally in `gateway-service`. Catalog refresh and reconciliation remain separate responsibilities.

### Pricing precedence

A route pricing override is authoritative for the complete configured pricing shape:

1. If `pricing_override` exists, use its input, output, cache-read, and cache-write values.
2. Do not fill omitted cache fields from the catalog.
3. If no override exists, retain current exact catalog resolution and current unpriced reasons.

Catalog limits, modalities, and display metadata may still supplement a route whose token rates are configured. The catalog itself is never mutated to represent admin policy.

### Context precedence

For one route:

1. Resolve the known catalog context, input, and output limits when available.
2. If a configured context cap exists, the effective context is the smaller of the configured and catalog contexts.
3. If catalog context is unknown, the configured cap is effective.
4. Clamp known input and output limits to the effective context.
5. Preserve an unknown input or output dimension as unknown.

If a later catalog refresh lowers context below a previously valid configured cap, refresh succeeds. Effective metadata follows the smaller catalog limit and emits a warning. The next serving startup rejects the now-invalid configured cap unless configuration is corrected.

## Logical-Model Aggregation

Admin model data and generated client configuration describe a logical gateway model, while execution selects a concrete route. Aggregation therefore uses routes that can currently serve:

- enabled
- positive weight
- resolvable provider

Disabled, zero-weight, and missing-provider routes do not contribute.

### Limits

- Context, input, and output limits are aggregated independently.
- Each published dimension is the minimum across eligible routes.
- If any eligible route lacks a dimension, that aggregate dimension is `null`.
- Existing client behavior may fall back from an unknown input limit to the safe aggregate context.
- Aliases use the resolved execution model's routes.

This prevents generated clients from advertising a limit larger than any selectable healthy route.

### Pricing

- The singular model price remains the existing primary display route's effective price.
- `pricing_varies_by_route` is `true` when any eligible route differs in input, output, cache-read, or cache-write rate.
- Priced versus unpriced or absent values count as a difference.
- Equal amounts with different provenance do not set the variance flag.
- No maximum-rate pair or weighted average is synthesized because either could describe no actual route.

## Provenance

### Admin models API

The admin API exposes structured source objects for effective pricing and context metadata. Each source contains:

- source kind: `configured_override`, `catalog`, or `mixed` where aggregation requires it
- catalog source when applicable
- catalog ETag when applicable
- catalog fetched-at timestamp when applicable

The API also exposes cache-write pricing and `pricing_varies_by_route`.

No Models UI provenance changes are included. The generated admin OpenAPI artifact and TypeScript types remain synchronized.

### Usage events

Every new usage-cost event snapshots the selected `model_route_id` as a non-foreign-key value. A non-FK snapshot survives route deletion and reseeding.

For configured pricing:

- `pricing_source` is `configured_override`.
- `model_route_id` is populated.
- Input, output, cache-read, and cache-write rates are copied to the event.
- `pricing_row_id`, pricing provider/model identity, ETag, fetched-at, and last-updated remain `null`.
- Existing `provider_key`, `upstream_model`, and `model_id` fields remain populated as before.

For catalog pricing, cache-read and cache-write rates and `model_route_id` are also snapshotted with the existing catalog provenance.

Configuration changes affect only future events. Earlier event rates and computed costs remain unchanged.

## Accounting Semantics

- Configured pricing is checked before catalog resolution.
- An overridden route is priced when the catalog has a conflicting row or no matching row.
- Input and output rates remain required, so normal prompt/completion usage can be priced deterministically.
- Existing fixed-point cost calculation and per-dimension half-up rounding remain unchanged.
- Explicit zero rates produce a priced zero-cost event.
- Cache rates are persisted and propagated but do not affect `computed_cost_usd` in this slice.
- Cache-aware charging waits for a canonical provider-neutral cache-token contract.
- Budget enforcement consumes the computed configured input/output cost through the existing usage-event path.

## Client Configuration

Generated client configurations receive:

- model-level minimum context, input, and output limits
- primary-route effective input, output, cache-read, and cache-write rates

Issue #237 is included in this work:

- `ClientConfigInput` gains optional cache-write pricing.
- Pi emits the effective cache-write rate when present.
- Pi emits zero only when cache-write pricing is absent.
- OpenCode and other harness output remain unchanged unless their existing documented schema supports the field.

The public `/v1/models` response remains unchanged.

## MCP Token-Overhead Telemetry

MCP request telemetry resolves context from the route actually selected for that request. It persists the effective route context and computes `context_window_percent_bps` from that value.

When neither configured nor catalog context is available, the context and percentage remain absent. This work does not add request token counting or preflight rejection.

## Refresh and Startup Behavior

The required sequence for serving startup is:

1. Run migrations when enabled.
2. Seed configuration when enabled.
3. Materialize or refresh effective catalog rows.
4. Validate persisted configured context caps against known catalog contexts.
5. Fail before binding if validation finds a configured cap above a known catalog context.

Manual and background catalog refreshes do not fail solely because an upstream context limit shrank. Effective metadata clamps to the new catalog value and a warning identifies the stale admin cap.

The standalone `seed-config` command does not acquire a new network/catalog lifecycle. Catalog-dependent validation is guaranteed by serving startup, including startup with `--seed-config=false`.

## Persistence Plan

Matching libsql and PostgreSQL migrations will add storage for:

- route-level context cap
- route-level pricing override
- usage-event `model_route_id`
- usage-event cache-read and cache-write rate snapshots

Route override data remains separate from `model_pricing`. Catalog refresh cannot overwrite or delete configured overrides.

Both store implementations must update:

- seed insertion and replacement
- route reads and row mapping
- usage-event insertion and row mapping
- test fixtures and repository contract tests

## API and Documentation Scope

Update:

- the admin models response and generated OpenAPI/TypeScript artifacts
- configuration reference with exact YAML, units, and validation
- pricing and accounting documentation with precedence, provenance, rounding, cache limitations, and history behavior
- model routing documentation with route aggregation and metadata-only enforcement scope
- client-harness documentation for Pi cache-write behavior
- deploy gateway configuration with a representative override example

Do not change:

- the public `/v1/models` card shape
- the Models UI provenance presentation
- request-time token preflight
- downloaded catalog records
- provider pricing identity requirements

## Verification Contract

Implementation tests must cover:

- valid exact-string and zero pricing values
- malformed, negative, overflow, missing input/output, and unknown pricing fields
- positive context validation
- startup rejection above known catalog context
- acceptance when catalog context is unknown
- configured pricing precedence over conflicting and missing catalog rows
- omitted cache fields remaining absent
- route-level context clamping
- logical-model minimum aggregation and unknown propagation
- pricing variance across all four rate dimensions
- catalog shrink after startup clamping safely
- selected-route context in MCP percentage telemetry
- usage-event route ID, source, and all rate snapshots
- configured event catalog fields remaining null
- immutable historical events after configuration or catalog changes
- input/output cost calculation and budget consumption
- no-override catalog and unpriced behavior remaining unchanged
- admin source objects and generated contract artifacts
- Pi cache-write present and absent behavior
- unchanged OpenCode output

## Resolved Alternatives

The interview rejected the following alternatives:

- optional or partial base input/output overrides
- YAML floating-point rates
- catalog fallback for omitted cache rates
- cache-aware cost calculation without a canonical cache-token contract
- publishing maximum, averaged, or synthesized multi-route pricing
- optimistic context aggregation that ignores an unknown selectable route
- rejecting catalog refresh when upstream context shrinks
- explicit override effective-date scheduling
- relaxing provider pricing identity requirements
- exposing provenance only as booleans or raw source strings
- adding provenance UI changes in this slice
- extending `/v1/models`
- approximate request token preflight

## Outstanding Questions

None. The interview contract was explicitly confirmed before this record was saved.
