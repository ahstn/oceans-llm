# ADR: Hybrid Pricing Catalog from models.dev

- Date: 2026-03-06
- Status: Accepted

## Context

We added budget and spend-accounting schema groundwork (`user_budgets`, `usage_cost_events`), but the gateway still lacked a reliable pricing source for live enforcement. Provider `/v1/models` endpoints were not sufficient because they do not expose stable billing data, and provider pricing pages are documented separately and can change independently of model discovery APIs.

We needed a pricing source that:

- works across the provider types we currently support,
- is structured enough to normalize into one internal schema,
- does not make every request depend on a live third-party pricing API,
- avoids opaque or approximate billing guesses,
- gives us a path to future spend accounting without schema churn.

## Decision

### 1. Use `models.dev` as the baseline pricing feed

We use `https://models.dev/api.json` as the upstream pricing input for this slice.

Why:
- it already projects provider/model pricing into a machine-readable format,
- it covers the providers we currently support (`openai`, `google-vertex`, `google-vertex-anthropic`),
- it is simpler and less brittle than scraping provider pricing pages.

### 2. Do not use `models.dev` as the only runtime dependency

We chose a hybrid model:

- a vendored normalized fallback snapshot committed in the repo,
- a persisted runtime cache in `pricing_catalog_cache`,
- conditional refresh against `models.dev/api.json` with `ETag`,
- best-effort revalidation every 15 minutes.

Why:
- request execution must continue if `models.dev` is unavailable,
- startup should not depend on a cold remote fetch once any usable snapshot exists,
- vendored fallback keeps the repo bootstrappable and reviewable.

### 3. Vendor our own normalized snapshot, not upstream TOML or a git submodule

We do not copy raw `models.dev` TOML files into this repo and do not use a git submodule. Instead, we generate a small repo-tracked JSON snapshot in our own schema for the supported providers.

Why:
- the gateway needs a stable internal format keyed to its own pricing resolution logic,
- coupling runtime behavior to the upstream repo layout would make upgrades fragile,
- a normalized snapshot is easier to diff, validate, and cache atomically.

### 4. Require explicit pricing identity for `openai_compat`

`openai_compat` provider config now requires `pricing_provider_id`, validated against the supported internal pricing provider ids.

Why:
- adapter type alone does not identify billing source,
- different OpenAI-compatible transports can represent different pricing catalogs,
- explicit mapping is safer than inference and fails closed when misconfigured.

### 5. Derive pricing identity for Vertex from publisher prefix

For `gcp_vertex`, pricing source is derived from `upstream_model`:

- `google/*` -> `google-vertex`
- `anthropic/*` -> `google-vertex-anthropic`

Why:
- Vertex is one transport layer serving multiple publishers,
- billing semantics depend on publisher family, not only transport type,
- exact upstream model ids remain the lookup key.

### 6. Resolve pricing only for exact supported billing paths

The gateway returns `unpriced` instead of approximating when billing inputs are incomplete or unsupported. Covered paths in this slice are exact model-id lookups for the supported providers with standard token pricing. Unsupported cases include:

- unknown `pricing_provider_id`,
- unknown model ids,
- unsupported billing modifiers such as `service_tier` / `serviceTier`,
- unsupported Vertex publisher families,
- non-global Vertex Anthropic locations.

Why:
- approximate pricing would make later budget enforcement unsafe,
- unsupported modifiers can materially change billing,
- failing closed keeps accounting integrity ahead of feature breadth.

### 7. Do not re-enable budget enforcement yet

Even with a pricing catalog in place, we still defer live budget enforcement and `usage_cost_events` writes.

Why:
- pricing coverage is intentionally exact-only and not yet complete for all billing variants,
- request-path spend attribution is still follow-up work,
- unpriced requests must not be blocked or charged.

## Consequences

Positive:
- the gateway now has a live, refreshable pricing source for supported providers,
- startup and request execution remain resilient when the remote catalog is unavailable,
- the internal pricing model is explicit and testable,
- future spend-ledger work can reuse the normalized catalog and cache table.

Tradeoffs:
- pricing freshness is bounded by the 15-minute refresh interval and remote availability,
- unsupported billing modifiers intentionally remain unpriced,
- vendored snapshot maintenance now requires an explicit sync step.

## Follow-up Work

- Wire request-path token usage and spend attribution to `usage_cost_events`.
- Expand exact coverage for additional billing modifiers only when the required usage and pricing inputs are explicit.
- Add operational visibility for stale catalog age and refresh failures.
- Evaluate whether additional providers should get first-class `pricing_provider_id` support.

## Attribution

This ADR was prepared through collaborative human + AI implementation/design work.
