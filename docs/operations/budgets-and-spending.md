# Budgets and Spending

`See also`: [Data Relationships](../reference/data-relationships.md), [Pricing Catalog and Accounting](../configuration/pricing-catalog-and-accounting.md), [Request Lifecycle and Failure Modes](../reference/request-lifecycle-and-failure-modes.md), [Identity and Access](../access/identity-and-access.md), [Admin Control Plane](../access/admin-control-plane.md), [ADR: Spend Control Plane Reporting and Team Hard-Limit Enforcement](../adr/2026-03-15-spend-control-plane-reporting-and-team-hard-limits.md)

This page describes the live spend contract in the gateway.

## Source of Truth

- spend ledger:
  - `usage_cost_events`
- request-path enforcement:
  - [../crates/gateway-service/src/budget_guard.rs](../../crates/gateway-service/src/budget_guard.rs)
- ledger writes:
  - [../crates/gateway-service/src/service.rs](../../crates/gateway-service/src/service.rs)
- admin spend APIs:
  - [../crates/gateway/src/http/spend.rs](../../crates/gateway/src/http/spend.rs)

## Ledger Contract

- `usage_cost_events` is the canonical usage and spend ledger
- request accounting is idempotent on `(request_id, ownership_scope_key)`
- pricing is resolved from the internal pricing catalog and persisted into the ledger row
- spend math uses fixed-point money and integer arithmetic

Pricing states are explicit:

- `priced`
- `legacy_estimated`
- `unpriced`
- `usage_missing`

Only `priced` and `legacy_estimated` rows count toward spend totals and budget windows.

## Runtime Enforcement

Pre-provider hard-limit checks run on the live request path for:

- `POST /v1/chat/completions`
- `POST /v1/embeddings`

Budgets are enforced by owner scope:

- user-owned API keys use the active user budget
- team-owned API keys use the active team budget

Hard-limit behavior:

- if projected spend in the active window would exceed the configured amount and `hard_limit = true`, the request fails with `budget_exceeded`
- the HTTP status is `429`
- the provider is not executed on this path
- observability records the request as a budget rejection outcome instead of a provider execution

## Two-Phase Enforcement

Budget enforcement has two phases:

1. pre-provider blocking against current priced spend
2. post-provider projected-cost blocking before the priced ledger row is inserted

This matters because duplicate requests bypass both phases as a no-op, and because the current stream versus non-stream difference still lives in the later ledger-write stage.

Ownership scope keys:

- user:
  - `user:<user_id>`
- team:
  - `team:<team_id>:actor:none`

`actor:none` is the current team attribution contract. Acting-user attribution is still deferred.

## Ledger Write Semantics

- successful request handling writes a ledger row when provider usage can be normalized
- if usage is missing, the row is marked `usage_missing`
- if pricing cannot be matched exactly, the row is marked `unpriced`
- `unpriced` and `usage_missing` rows stay visible in reporting but do not count toward spend totals

Use [request-lifecycle-and-failure-modes.md](../reference/request-lifecycle-and-failure-modes.md) for the cross-cutting path from request execution to ledger state.

## Budget Configuration Model

- `user_budgets` stores active and inactive user budgets
- `team_budgets` stores active and inactive team budgets
- each table enforces one active budget per owner

Budget fields:

- `cadence`
  - `daily`, `weekly`, or `monthly`
- `amount_10000`
- `hard_limit`
- `timezone`

`timezone` is stored now, but enforcement windows still use UTC.

## Budget Threshold Alerts

Budget alerts have deeper behavior than a plain email side effect.

- alerts are stored durably in `budget_alerts`
- per-recipient delivery attempts are stored in `budget_alert_deliveries`
- the initial threshold is fixed at `20%` remaining budget
- monthly cadence is supported end to end

Alert creation happens:

- after a new chargeable ledger row is written
- after a budget upsert, if the current spend is already at or below the threshold

Delivery behavior:

- alert creation is durable-first
- request handling writes alert rows and queued delivery rows first
- a background dispatcher sends email later
- delivery is single-attempt oriented in this slice
- email is the only live channel today, but the schema is channel-aware

Recipient readiness:

- user budgets notify the user email
- team budgets notify active team owners or admins with emails

That means email readiness is part of the practical identity setup for alerting.

## Spend Reporting APIs

Live admin spend APIs:

- `GET /api/v1/admin/spend/report`
- `GET /api/v1/admin/spend/budgets`
- `GET /api/v1/admin/spend/budget-alerts`
- `PUT /api/v1/admin/spend/budgets/users/{user_id}`
- `DELETE /api/v1/admin/spend/budgets/users/{user_id}`
- `PUT /api/v1/admin/spend/budgets/teams/{team_id}`
- `DELETE /api/v1/admin/spend/budgets/teams/{team_id}`

These routes require an authenticated platform-admin session.

## Window Semantics

- daily windows start at `00:00:00 UTC`
- weekly windows start at `Monday 00:00:00 UTC`
- monthly windows start at `00:00:00 UTC` on the first day of the month
- `Sunday 23:59:59 UTC` is still part of the previous weekly window

## Current Gaps

- provider breakdown is not part of spend reporting v1
- acting-user attribution for team-owned keys remains `actor:none`
- timezone-aware budget windows are still deferred
- declarative config-driven budgets are not supported yet
  - [issue #64](https://github.com/ahstn/oceans-llm/issues/64)
  - [issue #65](https://github.com/ahstn/oceans-llm/issues/65)

## What This Page Does Not Own

- exact pricing coverage and `unpriced` causes:
  - [pricing-catalog-and-accounting.md](../configuration/pricing-catalog-and-accounting.md)
- end-to-end request path:
  - [request-lifecycle-and-failure-modes.md](../reference/request-lifecycle-and-failure-modes.md)
- operator-facing admin UI behavior:
  - [admin-control-plane.md](../access/admin-control-plane.md)
- identity lifecycle and email readiness:
  - [identity-and-access.md](../access/identity-and-access.md)
