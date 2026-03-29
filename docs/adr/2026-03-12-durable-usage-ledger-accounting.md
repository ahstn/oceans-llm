# ADR: Durable Usage Ledger and Effective-Dated Spend Accounting

- Date: 2026-03-12
- Status: Accepted

## Current state

- [../pricing-catalog-and-accounting.md](../pricing-catalog-and-accounting.md)
- [../budgets-and-spending.md](../budgets-and-spending.md)
- [../data-relationships.md](../data-relationships.md)

## Context

The gateway already had identity-aware ownership, request logging, user budgets, and a hybrid pricing catalog, but spend accounting was still incomplete in ways that would have made enforcement unsafe:

- `usage_cost_events` was only schema groundwork and not yet the authoritative accounting ledger,
- request retries and replays could not be made idempotent against a canonical request identity,
- pricing was resolved at runtime from the catalog but not persisted as a historical pricing snapshot per request,
- budget charging was not yet tied to one durable usage record per successful request,
- PostgreSQL support had recently become a first-class runtime backend, so new accounting behavior needed backend parity instead of libsql-only behavior.

Issues #4 and #7 required the gateway to move from approximate or deferred accounting toward a durable, auditable request-spend ledger that preserves historical totals even when pricing changes later.

## Decision

### 1. Make `usage_cost_events` the authoritative request usage ledger

We kept the existing `usage_cost_events` table name, but expanded it into the canonical usage and spend ledger rather than creating a second accounting table.

The ledger now stores:

- canonical request ownership scope,
- normalized token usage,
- raw provider usage payload,
- pricing resolution status,
- pricing row reference plus copied provenance fields,
- fixed-point computed request cost.

Why:
- existing schema groundwork and read paths could evolve without renaming the core accounting table,
- one authoritative ledger avoids drift between “usage”, “cost”, and “budget” tables,
- request logs remain operational telemetry instead of becoming a second source of truth.

### 2. Use canonical request identity plus ownership scope for idempotent charging

Every request is now accounted against a canonical key:

- `request_id`
- `ownership_scope_key`

`ownership_scope_key` is derived as:

- `user:<owner_user_id>` for user-owned keys,
- `team:<owner_team_id>:actor:<actor_user_id|none>` for team-owned keys.

`actor_user_id` is persisted as a nullable reserved field even though the current auth path does not yet populate it.

Why:
- request replay protection has to be scoped to the owner being charged,
- the same raw request identifier should not double-charge if the gateway retries internally or the client replays a request,
- reserving `actor_user_id` now avoids another ledger uniqueness migration when acting-user attribution arrives later.

### 3. Guarantee and return a canonical gateway request ID

`/v1/chat/completions` now always has a canonical gateway request ID. If `x-request-id` is not provided, the gateway generates a UUID and returns it in the response.

Why:
- idempotent accounting cannot rely on `"missing-request-id"` placeholders,
- tracing, debugging, and replay analysis all benefit from a stable request identifier,
- this keeps the public API stable while tightening accounting guarantees.

### 4. Persist effective-dated model pricing rows and charge from the matched row

We added `model_pricing` as an explicit normalized pricing store keyed by:

- pricing provider id,
- pricing model id,
- effective time window.

Pricing still originates from the hybrid models.dev-backed catalog introduced earlier, but request accounting now resolves a concrete pricing row at request time and copies the matched pricing metadata into the ledger row.

Why:
- historical spend must not change when the pricing catalog refreshes,
- persisted pricing rows give auditable provenance for each computed charge,
- effective windows let the gateway represent price changes without mutating prior request rows.

### 5. Compute request cost with exact fixed-point integer arithmetic

Request cost is computed from normalized usage and matched pricing rates using integer math only, then stored as scaled fixed-point money.

The canonical spend states are:

- `priced`
- `unpriced`
- `usage_missing`
- `legacy_estimated`

Only `priced` and migrated `legacy_estimated` rows count toward spend totals and budget windows.

Why:
- floating-point arithmetic would make budget enforcement and historical totals nondeterministic,
- unpriced or usage-missing requests must remain available without silently charging an approximate amount,
- explicit pricing states make accounting gaps visible instead of hidden in zeros or nulls.

### 6. Run budget enforcement against the ledger, not transient request state

Budget charging and spend-window aggregation now read from the durable ledger and use idempotent insert semantics:

- if the `(request_id, ownership_scope_key)` row already exists, the replay is a no-op,
- if the row is new and `priced`, spend is aggregated from ledger rows inside the window before insert,
- if the request is `unpriced` or `usage_missing`, it is recorded but excluded from budget charging.

Why:
- retries must not increase spend,
- budget enforcement should use the same persisted state that later reporting reads,
- ledger-first enforcement makes concurrent request races easier to reason about than transient in-memory charging.

### 7. Migrate legacy accounting rows forward instead of dropping history

Migration `V8` reshapes legacy `usage_cost_events` rows into the new ledger model and preserves prior spend history by backfilling them as `legacy_estimated`.

The migration also:

- derives `ownership_scope_key` from the legacy row owner fields,
- deduplicates historical rows by `(request_id, ownership_scope_key)`,
- archives discarded duplicates into `usage_cost_event_duplicates_archive`,
- adds effective-dated `model_pricing`.

Why:
- historical spend should survive the ledger redesign,
- duplicate historical rows should not remain chargeable,
- auditability is better served by archiving duplicate rows than silently deleting them.

### 8. Require libsql and PostgreSQL accounting parity

The accounting feature is implemented in the split backend store architecture for both libsql and PostgreSQL.

This includes:

- ledger lookup and idempotent insert,
- budget-window spend aggregation,
- pricing row persistence and resolution,
- backend-specific migration `V8`,
- gateway and store tests covering PostgreSQL behavior.

For PostgreSQL specifically, fixed-point money remains stored as `BIGINT`, and aggregate spend queries cast `SUM(bigint)` results back to `BIGINT` at the SQL boundary before decoding to `Money4`.

Why:
- production-shaped environments now default to PostgreSQL, so accounting cannot be correct only on libsql,
- the domain money type remains fixed-point `i64`, so Postgres aggregate widening to `NUMERIC` should be normalized at the query boundary rather than introducing decimal types into the accounting model,
- backend parity reduces the chance of production-only accounting regressions.

## Consequences

Positive:
- successful requests now have one durable accounting record per canonical owner scope,
- retries and replays no longer double-charge spend or budgets,
- historical request totals remain stable after pricing catalog refreshes,
- pricing gaps are explicit through `unpriced` and `usage_missing` ledger states,
- PostgreSQL and libsql now share the same accounting semantics.

Tradeoffs:
- the schema and migration surface for accounting is materially larger than the earlier groundwork,
- some successful requests may intentionally remain uncharged until pricing coverage becomes exact,
- effective-dated pricing rows and copied provenance add storage overhead in exchange for auditability.

## Follow-up Work

- Add reporting and administrative surfaces that read directly from the durable usage ledger.
- Introduce acting-user attribution for team-owned keys when authenticated end-user context is available.
- Expand exact pricing coverage only when billing modifiers and provider usage semantics are explicit enough to preserve accounting integrity.
- Add operational visibility for unpriced and usage-missing request rates.

## Attribution

This ADR was prepared through collaborative human + AI implementation/design work.
