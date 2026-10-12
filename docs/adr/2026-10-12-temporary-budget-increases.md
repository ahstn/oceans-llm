# ADR: Temporary Budget Increases

- Date: 2026-10-12
- Status: Accepted

## Current state

- [Budgets](../access/budgets.md#temporary-budget-increases)
- [Configuration Reference](../configuration/configuration-reference.md#permissions)

## Context

When a user nears a hard budget, the only remedy was for a platform admin to edit the budget. An edit is permanent: it changes the configured amount and, for config-sourced budgets, turns the row into a manual budget. Team admins could not act on their own members at all, because Spend Controls was a platform-admin page with global data.

We wanted a short-lived top-up with these properties:

- the people closest to the user can grant it;
- it is only available when it is actually needed;
- it is bounded in size;
- it undoes itself.

## Decision

### 1. Increases are separate, expiring rows

Migration `V56__budget_increases` (libSQL and Postgres) adds `budget_increases`, which is keyed to `budget_id`. Each row has `amount_usd`, `starts_at`, `expires_at`, `created_by_user_id`, and `superseded_at`. The budget row itself never changes.

Each budget has at most one active increase. Creating an increase marks any unexpired one as superseded in the same transaction, and lookups also take the newest row as a guard against races. Increases replace each other; they do not stack.

Supersession is not retroactive. `superseded_at` is the moment the replacement starts, and lookups treat an increase as applying from `starts_at` until it expires or is superseded. Enforcement and alerts evaluate a request at its `occurred_at`, so a request that started under the old increase is still checked against it, even if its usage is recorded after the replacement.

Creates are serialized per budget: Postgres locks the parent `budgets` row with `FOR UPDATE`, and libSQL uses an `IMMEDIATE` transaction. The API checks the 85% threshold against the active increase it read. It passes that increase's id to the store, which returns a conflict (HTTP 409) if a different increase is active once the lock is held. Two admins who act at the same moment therefore cannot both pass the threshold check against the same stale limit.

### 2. The effective limit is the base plus the active increase

`gateway_service::budget_scopes::effective_budget_limit` returns the base budget plus the active increase, if one exists. It is used by:

- hard-limit enforcement in `budget_guard`;
- threshold alerts in `budget_alerts`;
- the Spend Controls listing, which returns `active_increase` for each user.

### 3. One endpoint with server-side rules

`POST /api/v1/admin/spend/budgets/increases` takes `user_id`, `amount_usd`, and `period` (`end_of_month` or `seven_days`). The gateway enforces every rule. The UI repeats them only so it can explain why the button is disabled.

- **Permission.** Platform admins can increase any user's budget. Team owners and admins can increase budgets only for members of their own team; any other target returns 403.
- **Threshold.** Current-window spend must be at least 85% of the effective limit.
- **Cap.** `0 < amount_usd ≤ 3 × base budget`.
- **Expiry.** `end_of_month` expires at the first instant of the next calendar month in UTC. `seven_days` expires seven days after creation.

Increases apply only to user budgets. Service-account and user-model budgets are unchanged.

### 4. Team admins get a team-scoped Spend Controls

`spend_controls` can now be granted to `team_admins`, and it is now in their default pages. `require_spend_controls_scope` resolves a platform or team data scope. For team admins, the budget listing contains only their team's users and service accounts. Mutating budget endpoints and alert history still require a platform admin. In the UI, team admins see a read-only page with no `Configure`, `Remove`, `Add model budget`, or `Alert history`.

## Consequences

Benefits:

- team admins can unblock their own members without touching platform-wide budget config;
- increases expire on their own, so permanent budgets stay as configured;
- config sync and `Configure` edits keep the same `budget_id`, so an active increase survives them.

Trade-offs:

- every installation's team admins now see Spend Controls for their team unless config overrides `permissions.team_admins.pages`;
- hard-limit enforcement runs one extra indexed lookup per applicable budget on each request;
- there is no revoke endpoint, so an increase stays until it expires or a newer one supersedes it;
- alerts are deduplicated per budget window, so a user who already crossed the 20% remaining threshold is not alerted again after an increase is used up;
- if a budget is deactivated and later recreated, the new row gets a new `budget_id` and the old increase no longer applies;
- the threshold is checked only when the increase is created, so an increase granted at 85% stays valid if spend later falls, for example in a new window.

## Follow-up work

- Add revoke, and record increase history in the admin audit trail.
- Decide whether to support increases for service-account and user-model budgets.
- Consider re-arming threshold alerts once an increase is in effect.

## Attribution

This ADR was prepared through collaborative human + AI implementation and design work.
