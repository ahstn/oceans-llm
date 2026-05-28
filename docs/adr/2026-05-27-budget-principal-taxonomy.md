# Budget Principal Taxonomy

Date: 2026-05-27

## Status

Accepted

## Decision

Budget enforcement is scoped to spend-bearing principals only:

- human users
- service accounts
- human user model scopes

Teams are grouping and service-account ownership metadata. Teams are not budget principals, API-key owner kinds, or runtime ownership scopes.

The budget API is generic and scope based:

- `GET /api/v1/admin/spend/budgets`
- `PUT /api/v1/admin/spend/budgets`
- `POST /api/v1/admin/spend/budgets/deactivate`

Supported budget scope keys are:

- `budget:v1:user:<user_id>`
- `budget:v1:service_account:<service_account_id>`
- `budget:v1:user:<user_id>:model:<model_id>`
- `budget:v1:user:<user_id>:upstream_model:<trimmed_upstream_model>`

## Implementation

The runtime uses a generic `budgets` table with typed nullable columns and canonical `scope_key` uniqueness for active rows. Historical `user_budgets`, `team_budgets`, and `service_account_budgets` data is migrated or deleted by V28 migration SQL. Team budget and team-owned key cleanup is destructive because there are no production accounting histories to preserve.

Human user traffic evaluates a user model budget before the user's general budget. Service-account traffic evaluates only the service-account budget.

Active service-account API keys require an active service-account budget. Admins must revoke or deactivate active service-account keys before deactivating that service account's budget.

Config-seeded API keys declare the service account they create or reconcile, including the owning team and active service-account budget. There is no implicit singleton service account and no reserved `system-legacy` owner.

## Consequences

- Team budget APIs and UI controls are removed.
- Spend/report filters support `all`, `user`, and `service_account`; `team` is rejected.
- Team metadata can still narrow or describe service-account reporting, but it is not an ownership scope.
- Hard-limit enforcement remains best effort under concurrency and may overshoot. Reservations are out of scope.

## Supersedes

This supersedes the budget and seeded-key ownership portions of:

- [2026-03-05 Identity Foundation](2026-03-05-identity-foundation.md)
- [2026-03-15 Spend Control Plane Reporting and Team Hard-Limit Enforcement](2026-03-15-spend-control-plane-reporting-and-team-hard-limits.md)
- [2026-03-31 Declarative Config Seeded Identity and Budget Reconciliation](2026-03-31-declarative-config-seeded-identity-and-budget-reconciliation.md)
- [2026-03-31 Pre-v1 Migration Rebaseline](2026-03-31-pre-v1-migration-rebaseline.md)
- [2026-05-10 Team Service Accounts for Non-Human Gateway Access](2026-05-10-team-service-accounts.md)
- [2026-05-19 FOCUS Billing Data Export](2026-05-19-focus-billing-data-export.md)
