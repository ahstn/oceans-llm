# ADR: Observability Usage Leaderboard

- Date: 2026-04-21
- Status: Accepted
- Related Issues:
  - [#84](https://github.com/ahstn/oceans-llm/issues/84)
- Builds On:
  - [2026-03-12-durable-usage-ledger-accounting.md](2026-03-12-durable-usage-ledger-accounting.md)
  - [2026-03-15-otlp-observability-and-request-log-payloads.md](2026-03-15-otlp-observability-and-request-log-payloads.md)
  - [2026-03-15-spend-control-plane-reporting-and-team-hard-limits.md](2026-03-15-spend-control-plane-reporting-and-team-hard-limits.md)
  - [2026-03-28-generated-admin-api-contract-and-typed-same-origin-client.md](2026-03-28-generated-admin-api-contract-and-typed-same-origin-client.md)

## Current state

- [../operations/budgets-and-spending.md](../operations/budgets-and-spending.md)
- [../operations/observability-and-request-logs.md](../operations/observability-and-request-logs.md)
- [../access/admin-control-plane.md](../access/admin-control-plane.md)

## Context

The admin control plane already exposed two adjacent but distinct surfaces:

- spend reporting, which summarized cost and request totals across owners and models,
- observability request logs, which supported per-request inspection and filtering.

What it did not expose was a fast answer to a common operational question: who is driving usage right now, and how does that change across the recent window?

Issue [#84](https://github.com/ahstn/oceans-llm/issues/84) asked for a dedicated leaderboard page under `Observability` that keeps the answer visually legible and operationally useful:

- a chart that compares the top spenders over time,
- a fixed leaderboard table ranked by spend for the same range,
- a small date-range control shared by both views.

There were a few constraints that mattered architecturally:

- the page needed live gateway-backed data, not preview or mock data,
- the ranking semantics needed to be explicit and stable,
- the chart needed enough aggregation to stay readable and inexpensive to fetch,
- the UI needed to fit the existing shadcn-based admin shell instead of introducing a parallel charting style.

## Decision

We added a dedicated leaderboard surface at `/observability/leaderboard` backed by a dedicated admin API endpoint.

The core decisions are:

### 1. Use a dedicated observability leaderboard endpoint

The gateway now exposes `GET /api/v1/admin/observability/leaderboard?range=7d|31d`.

Why:

- the leaderboard has different semantics from `/api/v1/admin/spend/report`,
- the chart and table need a coordinated response shape with ranked users, zero-filled time buckets, and dominant-model metadata,
- a dedicated endpoint keeps spend reporting and leaderboard evolution decoupled.

### 2. Rank the page by total spend over the selected window

Both the chart cohort and the table ranking use total spend across the selected range.

Tie-breaks are explicit:

- leaderboard rank: `total_spend desc`, then `total_requests desc`, then `user_name asc`,
- most-used model: `request_count desc`, then `spend desc`, then `model_key asc`.

Why:

- leaderboard pages need deterministic ordering,
- spend is the primary operational signal for this surface,
- explicit tie-breaks prevent jitter between refreshes and across storage backends.

### 3. Use a fixed top-five chart cohort and top-thirty table

The chart shows the first five ranked users for the selected range. The table shows the first thirty ranked users.

Why:

- a fixed chart cohort is much easier to read than per-bucket leader reshuffling,
- the table remains broad enough to be operationally useful without requiring pagination in v1,
- the chart and table stay semantically linked because both derive from the same ranked set.

### 4. Aggregate chart data into UTC-aligned 12-hour buckets

The response uses UTC half-day buckets across the selected range:

- `7d` produces 14 buckets per series,
- `31d` produces 62 buckets per series.

Why:

- the raw ledger can be much denser than a chart should render,
- 12-hour buckets preserve trend shape while keeping payload size and visual density controlled,
- fixed UTC alignment makes results deterministic across clients and storage implementations.

### 5. Reuse the existing admin shell, generated contract, and shadcn chart patterns

The page is implemented as a normal admin route and uses:

- the generated gateway OpenAPI contract,
- the existing same-origin admin data layer,
- shadcn chart/table/toggle primitives,
- the existing green chart token palette (`--chart-1` through `--chart-5`).

Why:

- the leaderboard should behave like the rest of the control plane,
- generated contract types reduce drift at the gateway/UI boundary,
- visual consistency matters more than inventing a bespoke observability style for one page.

## Implementation

### Gateway contract and handler

- [../../crates/gateway/src/http/admin_contract.rs](../../crates/gateway/src/http/admin_contract.rs)
  - defines the leaderboard query and response DTOs.
- [../../crates/gateway/src/http/observability.rs](../../crates/gateway/src/http/observability.rs)
  - validates the range, enforces admin auth, computes the window, selects the ranked users, builds zero-filled UTC buckets, and returns the combined payload.
- [../../crates/gateway/src/http/mod.rs](../../crates/gateway/src/http/mod.rs)
  - registers the new route.
- [../../crates/gateway/openapi/admin-api.json](../../crates/gateway/openapi/admin-api.json)
  - carries the checked-in generated contract for the new endpoint.

### Storage queries

- [../../crates/gateway-core/src/traits.rs](../../crates/gateway-core/src/traits.rs)
  - adds repository methods for leaderboard users and bucket aggregates.
- [../../crates/gateway-store/src/libsql_store/budgets.rs](../../crates/gateway-store/src/libsql_store/budgets.rs)
- [../../crates/gateway-store/src/postgres_store/budgets.rs](../../crates/gateway-store/src/postgres_store/budgets.rs)
  - compute ranked users, dominant models, total requests, and 12-hour spend buckets for the chart cohort.

The store implementations intentionally compute:

- the top 30 users for the full selected window,
- the top 5 chart users from that ranked list,
- only the bucket aggregates needed for those 5 chart users.

That avoids over-fetching and keeps both storage backends aligned on the same semantics.

### Admin UI

- [../../crates/admin-ui/web/src/routes/observability/leaderboard.tsx](../../crates/admin-ui/web/src/routes/observability/leaderboard.tsx)
  - renders the page, uses loader data for the initial `7d` range, and refetches via a server function for `31d`.
- [../../crates/admin-ui/web/src/server/admin-data.server.ts](../../crates/admin-ui/web/src/server/admin-data.server.ts)
- [../../crates/admin-ui/web/src/server/admin-data.functions.ts](../../crates/admin-ui/web/src/server/admin-data.functions.ts)
  - expose the new leaderboard read path to the route.
- [../../crates/admin-ui/web/src/components/ui/chart.tsx](../../crates/admin-ui/web/src/components/ui/table.tsx)
- [../../crates/admin-ui/web/src/components/ui/toggle.tsx](../../crates/admin-ui/web/src/components/ui/toggle-group.tsx)
  - provide the shadcn primitives needed by the page.
- [../../crates/admin-ui/web/src/components/layout/admin-nav.ts](../../crates/admin-ui/web/src/components/layout/admin-nav.ts)
  - adds the `Leaderboard` item under `Observability`.

The UI uses an overlaid `AreaChart` rather than stacked data so each top user remains individually comparable against the others. It also keeps loading skeletons and a no-data empty state, because operational pages need clear behavior even when the ledger is empty or a range change is still in flight.

## Tradeoffs

- A dedicated endpoint introduces one more admin API surface to maintain, but it avoids overloading the spend report contract with leaderboard-specific concerns.
- UTC-only buckets are operationally stable, but they do not reflect the viewer's local timezone in v1.
- A fixed top-five chart cohort is easier to interpret than dynamic per-bucket leaders, but it deliberately does not answer "who led this exact bucket" if a lower-ranked user briefly spikes.
- Returning exactly thirty rows keeps the first version simple, but it leaves pagination and search for later if the operator needs a broader leaderboard.

## Consequences

Positive:

- operators now have a single observability surface for recent high-usage users,
- ranking behavior is deterministic across libsql and postgres,
- the leaderboard fits the existing same-origin admin architecture and visual system,
- the API payload is shaped for the page instead of forcing the UI to reconstruct leaderboard semantics client-side.

Negative:

- contributors now have another generated contract path to keep in sync,
- future changes to ranking semantics will need coordinated backend and UX updates,
- UTC aggregation may prompt future requests for timezone-aware bucketing or richer range options.

## Follow-Up

Potential follow-up work, if operational needs justify it:

- add more date ranges or a custom range picker,
- add pagination or search for users beyond the top 30,
- expose timezone-aware presentation while keeping storage semantics deterministic,
- reuse the same ranked-user query primitives for alerts or scheduled usage summaries.

This ADR was prepared through collaborative human + AI implementation and documentation work.
