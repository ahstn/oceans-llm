# Budgets and Spending

`Owns`: spend ledger semantics, budget enforcement rules, spend APIs, and current spend-policy deferrals.
`Depends on`: [data-relationships.md](data-relationships.md), [model-routing-and-api-behavior.md](model-routing-and-api-behavior.md), [pricing-catalog-and-accounting.md](pricing-catalog-and-accounting.md)
`See also`: [identity-and-access.md](identity-and-access.md), [admin-control-plane.md](admin-control-plane.md), [adr/2026-03-15-spend-control-plane-reporting-and-team-hard-limits.md](adr/2026-03-15-spend-control-plane-reporting-and-team-hard-limits.md)

This document describes the live spend contract in the gateway.

## Source of Truth

- Spend ledger: `usage_cost_events`
- Request-path enforcement: [../crates/gateway-service/src/budget_guard.rs](../crates/gateway-service/src/budget_guard.rs)
- Ledger writes: [../crates/gateway-service/src/service.rs](../crates/gateway-service/src/service.rs)
- Admin spend APIs: [../crates/gateway/src/http/spend.rs](../crates/gateway/src/http/spend.rs)

## Pricing and Accounting Source of Truth

- `usage_cost_events` is the canonical usage and spend ledger
- Request accounting is idempotent on `(request_id, ownership_scope_key)`
- Pricing is resolved from the internal hybrid catalog and persisted into the ledger row
- Spend math uses fixed-point money (`usd * 10_000`) and integer arithmetic

Pricing states are explicit:

- `priced`
- `legacy_estimated`
- `unpriced`
- `usage_missing`

Only `priced` and `legacy_estimated` rows count toward spend totals and budget windows.

For why successful requests can still become `unpriced` or `usage_missing`, see [pricing-catalog-and-accounting.md](pricing-catalog-and-accounting.md).

## Runtime Enforcement

Pre-provider hard-limit checks run on the live request path for both current write paths:

- `POST /v1/chat/completions`
- `POST /v1/embeddings`

The gateway enforces budgets by owner scope:

- user-owned API keys use the active user budget
- team-owned API keys use the active team budget

Hard-limit behavior:

- if projected spend in the active window would exceed the configured amount and `hard_limit = true`, the request fails with `budget_exceeded`
- the HTTP status is `429`

Idempotent replay behavior:

- duplicate `(request_id, ownership_scope_key)` is a no-op for charging and enforcement

## Two-Phase Enforcement Contract

Budget enforcement has two important phases:

1. pre-provider blocking against the current priced spend in the active window
2. post-provider projected-cost blocking before the priced ledger row is inserted

This matters because duplicate `(request_id, ownership_scope_key)` requests bypass both phases as a no-op and because post-provider ledger-write behavior is where the current stream/non-stream inconsistency still exists.

Ownership scope keys:

- user: `user:<user_id>`
- team: `team:<team_id>:actor:none`

`actor:none` is the current team attribution contract. Acting-user attribution remains deferred.

## Ledger Write Semantics

- Successful request handling writes a ledger row when provider usage can be normalized
- If usage is missing, the ledger row is marked `usage_missing`
- If pricing cannot be matched exactly, the ledger row is marked `unpriced`
- `unpriced` and `usage_missing` rows remain visible in reporting but do not count toward spend totals

Common `unpriced` causes include:

- missing or unsupported pricing-provider mapping
- unsupported Vertex publisher or location
- unsupported billing modifiers such as `service_tier` / `serviceTier`
- missing exact pricing-rate coverage

One important known rough edge remains:

- stream and non-stream chat requests do not yet share identical post-provider ledger failure semantics

That follow-up is tracked in [issue #49](https://github.com/ahstn/oceans-llm/issues/49).

## Budget Configuration Model

- `user_budgets` stores active and inactive user budgets
- `team_budgets` stores active and inactive team budgets
- each table enforces one active budget per owner through a partial unique index

Budget fields:

- `cadence`: `daily` or `weekly`
- `amount_10000`
- `hard_limit`
- `timezone`

`timezone` is stored now, but current enforcement windows remain UTC-anchored.

## Spend Reporting APIs

Live admin spend APIs:

- `GET /api/v1/admin/spend/report`
  - daily windowed series
  - owner breakdown (`user` and `team`)
  - model breakdown
  - totals for priced cost and request counts by pricing state
- `GET /api/v1/admin/spend/budgets`
  - current user and team budget state
  - current-window spend
- `PUT /api/v1/admin/spend/budgets/users/{user_id}`
- `DELETE /api/v1/admin/spend/budgets/users/{user_id}`
- `PUT /api/v1/admin/spend/budgets/teams/{team_id}`
- `DELETE /api/v1/admin/spend/budgets/teams/{team_id}`

These routes require an authenticated platform-admin session.

## Window Semantics

- Budget and reporting windows are UTC-based
- Daily windows start at `00:00:00 UTC`
- Weekly windows start at `Monday 00:00:00 UTC`
- `Sunday 23:59:59 UTC` is included in the previous weekly window

## Scope and Deferrals

Current explicit deferrals:

- provider breakdown is not included in spend reporting v1
- acting-user attribution for team-owned keys remains `actor:none`
- timezone-aware windows are deferred even though timezone is stored
- request-log payload retention and request-log performance policy are separate from spend accounting

For the operator surface built on top of these rules, see [admin-control-plane.md](admin-control-plane.md).
