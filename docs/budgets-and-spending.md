# Budgets and Spending

This document describes the current runtime behavior and the schema groundwork for future spend controls.

## Current Runtime Behavior

- `/v1/chat/completions` does not enforce user budgets in this slice.
- `/v1/chat/completions` does not write `usage_cost_events` in this slice.
- `budget_exceeded` is therefore not part of the live chat request path yet.
- Request logging is separate from budget accounting. When enabled, request logs capture the final user-visible outcome of an executed chat request.

## Persisted Schema Foundation

- `user_budgets` stores per-user budget settings, including cadence, amount, timezone, and `hard_limit`.
- `usage_cost_events` is reserved for future pricing-ledger and spend-accounting work.
- The current schema keeps daily and weekly cadence fields so a later pricing-backed implementation does not require schema churn.

## Planned Budget Semantics Once Pricing Exists

- User-owned requests are the initial enforcement target.
- Team-owned keys are not planned to be budget-blocked by user budgets in the initial rollout.
- Daily windows are intended to start at `00:00:00 UTC`.
- Weekly windows are intended to start at `Monday 00:00:00 UTC`.
- `Sunday 23:59:59 UTC` remains part of the previous weekly window.
- `Monday 00:00:00 UTC` starts a new weekly window.

## Notes

- This document does not promise active runtime enforcement until pricing and spend attribution are wired end to end.
- Schema presence should not be interpreted as live policy enforcement.
