# Budgets and Spending

This document describes user-facing behavior for usage accounting and budget enforcement.

## What Counts Toward Budget

- Budget checks are based on usage cost events recorded per request.
- Usage cost events are always recorded, even when request logging is disabled.
- In this phase, hard budget enforcement applies to user-owned API keys.

## Reset Cadence and Time Boundaries

- Daily budgets reset at `00:00:00 UTC` each day.
- Weekly budgets reset at `Monday 00:00:00 UTC`.
- `Sunday 23:59:59 UTC` is still part of the previous week.
- `Monday 00:00:00 UTC` starts a new weekly budget window.

## Hard Limit Behavior

- If `hard_limit` is enabled, requests are blocked when projected spend would exceed the active budget amount.
- If `hard_limit` is disabled, usage can continue beyond the budget amount.

## Attribution Policy

- User-owned key: usage is attributed to that user.
- Team-owned key with acting-user context: usage is attributed to both the team and the acting user.
- Team-owned key without acting-user context: usage is attributed to the team only.

## Notes

- Team-owned keys are not budget-blocked by user budgets in this phase.
- This document describes operational behavior, not full schema internals.
