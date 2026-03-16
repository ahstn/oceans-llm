# Budgets and Spending

This document describes live spend accounting, reporting, and budget enforcement behavior in the gateway.

## Pricing and Accounting Source of Truth

- `usage_cost_events` is the canonical usage and spend ledger.
- Request accounting is idempotent on `(request_id, ownership_scope_key)`.
- Pricing is resolved from the hybrid models.dev-backed catalog and persisted as effective-dated pricing metadata in ledger rows.
- Spend math uses fixed-point money (`usd * 10_000`) and integer arithmetic.
- Pricing states are explicit:
  - `priced`
  - `legacy_estimated`
  - `unpriced`
  - `usage_missing`

Only `priced` and `legacy_estimated` rows count toward spend totals and budget windows.

## Runtime Budget Enforcement

- `/v1/chat/completions` writes usage ledger rows for successful request handling.
- Budget checks are enforced on the request path for both owner scopes:
  - user-owned API keys use active user budgets,
  - team-owned API keys use active team budgets.
- Hard-limit behavior:
  - if projected window spend exceeds budget amount and `hard_limit = true`, the request fails with `budget_exceeded` (`HTTP 429`).
- Idempotent replay behavior:
  - duplicate `(request_id, ownership_scope_key)` is a no-op for charging and enforcement.
- Ownership scope keys:
  - user: `user:<user_id>`
  - team: `team:<team_id>:actor:none` (acting-user attribution remains deferred)

## Budget Configuration Model

- `user_budgets` stores active/inactive user budget configs.
- `team_budgets` stores active/inactive team budget configs.
- Both tables enforce one active budget per owner via partial unique indexes.
- Budget fields include:
  - `cadence`: `daily` or `weekly`
  - `amount_10000`
  - `hard_limit`
  - `timezone` (stored for future timezone-aware policy; runtime windows currently UTC-anchored)

## Spend Reporting and Admin APIs

Live admin spend APIs are exposed under `/api/v1/admin/spend/...`:

- `GET /api/v1/admin/spend/report`
  - windowed daily series
  - owner breakdown (`user`/`team`)
  - model breakdown
  - totals for priced cost and request counts by pricing state
- `GET /api/v1/admin/spend/budgets`
  - current user/team budget state and current-window spend
- `PUT/DELETE /api/v1/admin/spend/budgets/users/{user_id}`
- `PUT/DELETE /api/v1/admin/spend/budgets/teams/{team_id}`

Admin spend routes require an authenticated platform-admin session.

## Window Semantics

- Budget and reporting windows are UTC-based.
- Daily windows start at `00:00:00 UTC`.
- Weekly windows start at `Monday 00:00:00 UTC`.
- `Sunday 23:59:59 UTC` is included in the previous weekly window.

## Scope and Deferrals

- Spend reporting v1 includes owner and model breakdowns.
- Provider breakdown is intentionally deferred.
- Acting-user attribution for team keys is deferred; scope remains `actor:none`.
- Request-log payload design/performance work remains separate from spend accounting.
