# Budgets and Spending

`See also`: [Budgets](../access/budgets.md), [Data Relationships](../reference/data-relationships.md), [Pricing Catalog and Accounting](../configuration/pricing-catalog-and-accounting.md), [Request Lifecycle and Failure Modes](../reference/request-lifecycle-and-failure-modes.md), [Identity and Access](../access/identity-and-access.md), [Service Accounts](../access/service-accounts.md), [Admin Control Plane](../access/admin-control-plane.md)

This page is the developer/operator contract for spend accounting and budget enforcement. Product-facing setup guidance lives in [Budgets](../access/budgets.md).

## Source of Truth

- ledger writes: [../../crates/gateway-service/src/service.rs](../../crates/gateway-service/src/service.rs)
- budget domain: [../../crates/gateway-core/src/budgets.rs](../../crates/gateway-core/src/budgets.rs)
- budget scope evaluation: [../../crates/gateway-service/src/budget_scopes.rs](../../crates/gateway-service/src/budget_scopes.rs)
- request-path enforcement: [../../crates/gateway-service/src/budget_guard.rs](../../crates/gateway-service/src/budget_guard.rs)
- budget persistence: [../../crates/gateway-store/src/libsql_store/budgets.rs](../../crates/gateway-store/src/libsql_store/budgets.rs) and [../../crates/gateway-store/src/postgres_store/budgets.rs](../../crates/gateway-store/src/postgres_store/budgets.rs)
- admin spend APIs: [../../crates/gateway/src/http/spend.rs](../../crates/gateway/src/http/spend.rs)

## Ledger Contract

`usage_cost_events` is the canonical usage and spend ledger.

- request accounting is idempotent on `(request_id, ownership_scope_key)`
- `ownership_scope_key` uses `user:<user_id>` or `service_account:<service_account_id>`
- pricing is resolved from the internal pricing catalog and persisted into the ledger row
- spend math uses fixed-point money and integer arithmetic
- `team_id` remains reporting metadata for service-account rows, not a spend principal

Pricing states are explicit:

- `priced`
- `legacy_estimated`
- `unpriced`
- `usage_missing`

Only `priced` and `legacy_estimated` rows count toward budget windows and spend totals. `unpriced` and `usage_missing` rows remain report-visible accounting-quality signals.

## Budget Scopes

Budgets are stored in the generic `budgets` table. `scope_key` is canonical and has one active budget at a time.

Supported active scope keys:

- `budget:v1:user:<user_id>`
- `budget:v1:service_account:<service_account_id>`
- `budget:v1:user:<user_id>:model:<model_id>`
- `budget:v1:user:<user_id>:upstream_model:<trimmed_upstream_model>`

Supported `scope_kind` values:

- `user`
- `service_account`
- `user_model`

Team budget scopes do not exist. Migration-only references to historical team budget tables are confined to migration SQL.

## Enforcement Order

Budget checks run after model resolution and before provider execution, then again after provider execution when actual usage and pricing are known.

Human user traffic evaluates:

1. user model budget, when a matching model scope applies
2. user budget

User model matching uses the resolved gateway `model_id` when present. The exact, trim-only upstream model fallback is used only when `model_id` is absent.

Service-account traffic evaluates only:

1. service-account budget

Service-account credentials cannot authenticate unless their service account is active and has an active service-account budget.

## Hard And Soft Limits

Hard-limit behavior:

- pre-provider rejection returns `429 budget_exceeded`
- no provider call occurs on the pre-provider rejection path
- post-provider rejection happens before inserting a new priced ledger row
- duplicate request ids bypass budget math as an idempotent no-op for the same ownership scope

Soft budgets never reject. They still contribute to alert readiness and reporting.

Concurrency caveat: hard-limit enforcement is best effort under concurrent requests and can overshoot. Reservations are intentionally out of scope.

## Windows

Budget windows use UTC today:

- daily windows start at `00:00:00 UTC`
- weekly windows start at `Monday 00:00:00 UTC`
- monthly windows start at `00:00:00 UTC` on the first day of the month

`timezone` is stored on budget settings for future display/window work, but live enforcement still uses UTC.

## Alerts

Budget alerts are durable-first:

- alert records are stored in `budget_alerts`
- delivery attempts are stored in `budget_alert_deliveries`
- email is the only live channel today
- the threshold is fixed at `20%` remaining budget
- alert creation runs after a chargeable ledger row and after a budget upsert when current spend is already at or below the threshold

Recipients:

- user budgets notify the user email
- user model budgets notify the user email
- service-account budgets notify active owners or admins of the owning team

## Admin APIs

Live admin spend APIs:

- `GET /api/v1/admin/spend/report`
- `GET /api/v1/admin/spend/focus.csv`
- `GET /api/v1/me/spend/focus.csv`
- `GET /api/v1/admin/spend/budgets`
- `PUT /api/v1/admin/spend/budgets`
- `POST /api/v1/admin/spend/budgets/deactivate`
- `GET /api/v1/admin/spend/budget-alerts`

Budget mutation requests use typed `scope` objects. Responses include:

- `budget_id`
- typed `scope`
- computed `scope_key`
- settings
- current-window spend
- alert readiness

Old user, team, and service-account path-specific budget routes are not part of the runtime contract.

## Reporting And FOCUS Export

Spend report and FOCUS owner filters support:

- `all`
- `user`
- `service_account`

`team` is rejected. Service-account rows can still include owning team metadata and tags for reporting context.

FOCUS exports aggregate one row per UTC day, owner scope, upstream provider/model, and pricing status for `priced` and `legacy_estimated` ledger rows. `unpriced` and `usage_missing` rows are excluded from charge rows and reported through response headers.

## Declarative Seed

Config-seeded API keys must declare the service account they create or reconcile:

- service-account key
- service-account name
- owning team
- active service-account budget
- model grants

There is no implicit singleton service account, no reserved `system-legacy` team, and no team-owned runtime key fallback.

## Validation

When changing this area, run:

```bash
mise run admin-contract-generate
mise run admin-contract-check
mise -C docs run check
mise run lint
```
