# ADR: Spend Control Plane Reporting and Team Hard-Limit Enforcement

- Date: 2026-03-15
- Status: Accepted

## Implemented By

- Canonical docs:
  - [../budgets-and-spending.md](../operations/budgets-and-spending.md)
  - [../admin-control-plane.md](../access/admin-control-plane.md)

## Current state

- [../budgets-and-spending.md](../operations/budgets-and-spending.md)
- [../admin-control-plane.md](../access/admin-control-plane.md)

## Context

Issues #4 and #7 established a durable, idempotent usage ledger and exact fixed-point pricing/accounting behavior. That work made `usage_cost_events` trustworthy as a charging source, but the control plane still lacked spend-facing capabilities needed by operations:

- no live admin reporting from ledger data,
- no budget management APIs for teams,
- no hard-limit enforcement for team-owned API keys,
- no admin UI surfaces for live spend reporting and budget controls.

Issues #28 and #30 required shipping these spend-facing controls without introducing a second accounting source or breaking replay/idempotency guarantees already landed.

## Decision

### 1. Keep a single accounting source of truth

We continue to use `usage_cost_events` as the sole source for:

- budget-window spend checks,
- spend reporting aggregates,
- priced/unpriced/usage-missing accounting visibility.

No parallel spend tables were added.

Why:
- avoids drift between enforcement and reporting,
- preserves idempotent replay semantics,
- keeps accounting auditability centralized.

### 2. Add first-class team budget persistence with backend parity

We added `team_budgets` with matching migrations for libsql and PostgreSQL, including:

- one-active-budget-per-team partial unique index,
- parity CRUD/query behavior in store implementations.

Why:
- team-owned keys now require independent spend policy,
- parity across both runtime backends is required for production correctness.

### 3. Add spend reporting query surface in store/domain layers

We added store/domain contracts for:

- windowed spend sums by owner,
- daily aggregate series,
- owner breakdown (`user`/`team`),
- model breakdown with gateway model key when available and upstream fallback otherwise.

Aggregations include priced cost and counts split across `priced/legacy_estimated`, `unpriced`, and `usage_missing`.

Why:
- admin reporting must expose both charged spend and pricing-coverage gaps,
- model attribution should stay consistent with routing identity when possible.

### 4. Add admin spend API endpoints under `/api/v1/admin/spend/...`

We introduced new platform-admin protected endpoints for:

- live reporting (`/report`),
- budget state listing (`/budgets`),
- user/team budget upsert + deactivate.

Responses follow existing envelope patterns used by admin APIs.

Why:
- spend controls are operational admin functions,
- auth behavior should match identity-admin routes.

### 5. Extend hard-limit enforcement to team-owned keys

Budget guard now enforces:

- user budgets for user-owned keys,
- team budgets for team-owned keys.

Enforcement rules remain unchanged:

- only `priced` and `legacy_estimated` rows count toward spend checks,
- `unpriced` and `usage_missing` rows are recorded but not charged,
- duplicate request replays remain no-op for charging/enforcement.

`budget_exceeded` error code is unchanged; payload now identifies ownership scope (`user:*` or `team:*`).

Why:
- team-owned credentials need enforceable spend controls,
- preserving existing error code avoids unnecessary contract churn.

### 6. Ship live admin UI surfaces backed by gateway APIs

We replaced mock usage-cost page behavior with live reporting calls and added:

- 7/30 day window controls,
- owner-kind filters (`all`/`user`/`team`),
- owner/model breakdown tables,
- spend controls page for user/team budget lifecycle actions.

Provider breakdown is intentionally deferred from v1.

Why:
- issue scope prioritized operational spend visibility and governance controls,
- owner + model dimensions were selected over provider grouping for initial delivery.

## Consequences

Positive:
- spend reporting and budget controls are now live and backed by ledger data,
- user and team hard limits are both enforceable on request paths,
- admin control plane can configure and inspect spend policy without DB access,
- backend parity and tests reduce production-only regressions.

Tradeoffs:
- reporting v1 omits provider breakdown,
- team scope still uses `actor:none` until acting-user attribution lands,
- timezone is stored per budget but enforcement/report windows remain UTC in this slice.

## Follow-up Work

- Add acting-user attribution for team-owned requests and evolve ownership scope from `actor:none`.
- Add provider breakdown when reporting requirements justify the additional cardinality/cost.
- Revisit timezone-aware windows if product policy moves beyond UTC-fixed accounting windows.
- Continue expanding E2E/admin contract coverage as additional spend controls land.

## Attribution

This ADR was prepared through collaborative human + AI implementation/design work.
