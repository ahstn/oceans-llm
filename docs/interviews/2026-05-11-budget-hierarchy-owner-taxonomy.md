# Budget Hierarchy and Owner Taxonomy Interview

`See also`: [Budgets and Spending](../operations/budgets-and-spending.md), [Data Relationships](../reference/data-relationships.md), [ADR: Spend Control Plane Reporting and Team Hard-Limit Enforcement](../adr/2026-03-15-spend-control-plane-reporting-and-team-hard-limits.md), [GitHub issue #106](https://github.com/ahstn/oceans-llm/issues/106)

Date: 2026-05-11

## Scope

This interview aligned design decisions for GitHub issue #106. The original direction considered a hierarchy of personal user budgets, team aggregate budgets, team-owned/service-style traffic, and per-user model budgets. A follow-up interview revised that direction: team budgets and team-owned API keys should be removed entirely, and spend control should be based on user and service-account principals.

## Superseded Direction

The first pass considered keeping team budgets as aggregate guardrails and treating service-account-style traffic as a team-owned-key convention. That direction is superseded by the decisions below.

## Revised Core Direction

Drop team budgets entirely.

Teams remain useful for membership, service-account grouping, and model-access inheritance, but they are not spend-bearing principals and are not budget owners.

Spend-bearing principals are:

1. human users,
2. service accounts.

The implementation goal becomes:

> Replace team-owned spend control with user/service-account principal budgeting, remove team budgets and team-owned API keys, and add human user model budgets.

## Decisions

### Team Budgets

- Remove team budgets from runtime, persistence, API, UI, docs, and tests.
- Teams are not budget scopes.
- Delete existing `team_budgets` and associated team budget alert data in migration.
- Because there are no production instances, destructive cleanup is acceptable.

### Team-Owned API Keys

- Remove team-owned API keys in the same change.
- Delete existing team-owned API keys during migration.
- Remove `ApiKeyOwnerKind::Team` from the runtime/domain owner enum after migration cleanup.
- If migration-only decoding is needed, keep it local to migration/store code rather than runtime domain behavior.

### Historical Team-Owned Ledger Rows

- Delete historical team-owned `usage_cost_events` rows.
- This is intentionally destructive and acceptable because there are no production instances or accounting history to preserve.
- New ledger data should only be attributable to human users or service accounts.

### Service Accounts

- Service accounts are a separate principal kind, not users.
- Service accounts use the same aggregate budget mechanics as users.
- Every service account must belong to exactly one team.
- Service accounts inherit model access from their required team association.
- Budgets attach directly to the service-account principal, not to the team.

### Service Account Budget Requirement

- A service account must have an active budget before any service-account API key can be active.
- The budget may be hard or soft.
- Human user budgets remain optional.

### Human User Budgets

- Human user aggregate budgets remain supported.
- Human user budgets are optional.
- Personal user budgets remain the primary fairness mechanism for human user-owned traffic.

### Model Budgets

- Model budgets apply only to human users.
- Do not add service-account model budgets in #106.
- Model budgets are keyed primarily by gateway model ID.
- Fallback to exact stored `upstream_model` only when `model_id` is absent.
- Upstream model normalization is trim-only; no lowercasing or aliasing.

### Enforcement Order

- Human user-owned traffic checks budgets in narrow-to-broad order:
  1. user model budget, when one applies,
  2. overall user budget, when one exists.
- Service-account-owned traffic checks the service-account budget.
- Narrower budget failures should surface first.

### Generic Budget Table

- Keep the generic `budgets` table direction.
- Supported scope kinds are:
  - `user`,
  - `service_account`,
  - `user_model`.
- Do not include a `team` budget scope kind.
- Store typed nullable columns plus a canonical `scope_key`.
- Enforce one active budget per `scope_key`.

### Budget Scope Keys

Use versioned budget scope keys:

```text
budget:v1:user:<user_id>
budget:v1:service_account:<service_account_id>
budget:v1:user:<user_id>:model:<model_id>
budget:v1:user:<user_id>:upstream_model:<trimmed_upstream_model>
```

- Keep `usage_cost_events.ownership_scope_key` separate from budget `scope_key`.
- The budget evaluator maps request context to one or more budget scope keys.

### API Contract

- Replace old user/team budget admin endpoints with a generic budget API.
- Requests use typed `scope` objects.
- Responses include typed scope plus computed `scope_key`.
- This is a breaking admin-control-plane change.

### Rollout

Implement as one breaking migration PR:

- backend generic budget API,
- removal of team budget APIs,
- removal of team-owned API key creation/authentication paths,
- service-account budget enforcement,
- admin UI update,
- docs,
- ADR,
- migration guide.

### Existing Budget Data Migration

- Backfill existing `user_budgets` rows into the generic `budgets` table.
- Preserve IDs and fields where possible.
- Delete `team_budgets` rows rather than migrating them.
- Delete associated team budget alert data.
- Leave old tables only if needed as migration artifacts; runtime code should use generic budgets only.

### Soft Budgets

- `hard_limit = false` remains alerting/reporting only.
- Soft budgets never reject requests.
- A service account can satisfy the active-budget requirement with either a soft or hard budget.

### Unpriced and Usage-Missing Rows

- Never count `unpriced` or `usage_missing` rows toward any budget.
- Continue recording and reporting them.

### Pre-Provider Checks

- Evaluate all applicable scopes pre-provider and post-provider.
- Pre-provider rejects only when an applicable hard budget is already at or over limit.
- Post-provider uses actual projected cost.

### Reporting

- Global spend totals remain single-source from `usage_cost_events`.
- Owner/principal filters become:
  - `all`,
  - `user`,
  - `service_account`.
- Optional team filtering may narrow service-account reporting by the service account's required team association.
- Team is grouping metadata only, not an owner kind or budget scope.
- Budget-scope spend is independent and may overlap.
- Reports must not sum user and user-model budget-scope spend as additive totals.

### Timezones

- Keep current UTC window behavior in #106.
- Preserve the timezone field.
- Isolate window calculation so #101 can introduce timezone-aware windows later.

### Alerts

- Migrate budget alerts/history to generic budget scopes.
- Delivery behavior remains unchanged.
- Remove team budget alert records during migration.

### Admin UI

Spend Controls should show:

1. User Budgets,
2. Service Account Budgets,
3. User Model Budgets.

There should be no Team Budgets section.

### Config-Seeded API Keys

- Seeded keys must declare or create a service-account owner.
- Seeded service accounts must include:
  - required team association,
  - active budget.
- Remove the `system-legacy` team fallback for spend-bearing seeded keys.

### Concurrency

- Preserve current best-effort hard-limit semantics.
- Document possible concurrent overshoot.
- Keep atomic/idempotent insert guarantees and tests.

## Undocumented Assumptions to Capture

- Teams are grouping/access-control constructs, not spend principals.
- Team-level budgets are harmful as a fairness mechanism because they are too coarse and can allow one actor/workload to consume the shared pool.
- Human user fairness should be handled by human user budgets and human user model budgets.
- Service-account spend should be controlled by service-account budgets.
- Service accounts must belong to exactly one team for model-access inheritance.
- Budget scope keys and ledger ownership keys are separate concepts.
- User-model budgets overlap user budgets and must not create a second accounting source.
- Hard limits are not strictly race-safe under concurrent requests.
- Destructive migration of team budget/key/ledger data is acceptable because there are no production instances.

## Potential Follow-Up Enhancements

- Service-account model budgets, if workload-specific expensive-model controls become necessary.
- Race-safe budget reservations or transactional counters.
- Timezone-aware windows from #101.
- Provider-qualified upstream model fallback if exact upstream matching proves ambiguous.
- Generic scope hierarchy/rollup reporting.
