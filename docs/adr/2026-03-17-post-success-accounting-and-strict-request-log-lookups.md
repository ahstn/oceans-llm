# ADR: Fail-Open Post-Success Accounting and Strict Request-Log Detail Lookups

- Date: 2026-03-17
- Status: Accepted

## Context

Two follow-up issues exposed gaps in the contracts established by earlier work:

- [#49](https://github.com/ahstn/oceans-llm/issues/49): `/v1/chat/completions` handled post-success usage-ledger failures differently for streaming and non-streaming responses.
- [#50](https://github.com/ahstn/oceans-llm/issues/50): the admin request-log detail endpoint returned `200` with `data: null` for missing ids instead of a strict `404`.

These problems sat directly on top of earlier accepted decisions:

- [2026-03-12 durable usage ledger accounting](./2026-03-12-durable-usage-ledger-accounting.md) made `usage_cost_events` the authoritative accounting ledger.
- [2026-03-15 OTLP observability and request logs](./2026-03-15-otlp-observability-and-request-log-payloads.md) moved request-log assembly into a dedicated lifecycle service and exposed admin list/detail APIs.

The resulting implementation had two contract mismatches:

1. In the chat execution path, [the non-stream success branch](../../crates/gateway/src/http/handlers.rs) propagated `record_chat_usage(...)` failures to the client after the upstream provider had already succeeded, while the stream completion path only logged a warning and still completed successfully.
2. In the observability read path, [the request-log repository trait](../../crates/gateway-core/src/traits.rs), [service layer](../../crates/gateway-service/src/service.rs), [request logging service](../../crates/gateway-service/src/request_logging.rs), [store implementations](../../crates/gateway-store/src/libsql_store/request_logs.rs), [PostgreSQL store](../../crates/gateway-store/src/postgres_store/request_logs.rs), and [HTTP handler](../../crates/gateway/src/http/observability.rs) all treated a missing detail lookup as optional success rather than a not-found error.

For future readers, the important meaning is this:

- a request that has already succeeded upstream needs one explicit policy for what happens if downstream accounting fails afterward,
- an identity lookup in an internal admin API should not normalize absence into nullable success unless that is the intentional resource contract.

## Decision

### 1. Treat post-success accounting failures as operational failures, not client failures

Once the upstream provider has already succeeded, the gateway now **fails open** if usage-ledger/accounting persistence fails afterward.

This policy is implemented in the shared accounting finalization logic in [the gateway handler execution layer](../../crates/gateway/src/http/handlers.rs) and is used by:

- non-stream chat completions,
- stream completion after the final SSE chunk,
- embeddings, which share the same post-success accounting path.

Why:

- true streaming cannot reliably be turned back into a clean HTTP error once bytes have been sent,
- transport-specific behavior was accidental and would have continued to spread if left uncorrected,
- the client-visible result should represent the upstream execution result, while post-success accounting failure should be visible through operations and telemetry.

### 2. Add explicit observability for fail-open accounting incidents

The gateway now emits a dedicated accounting-failure signal through [gateway metrics](../../crates/gateway/src/observability.rs) in addition to structured warnings from the handler path.

Why:

- fail-open only works as a durable policy if the operational failure remains observable,
- usage-record failures should not disappear inside generic success metrics,
- future alerting and dashboards need a stable counter for this class of integrity incident.

### 3. Make request-log detail lookup a strict resource contract

`GET /api/v1/admin/observability/request-logs/{request_log_id}` is now a strict lookup:

- existing record -> `200`
- missing record -> `404`

This strictness is enforced at the repository boundary, not only in HTTP translation:

- [repository trait](../../crates/gateway-core/src/traits.rs),
- [gateway service](../../crates/gateway-service/src/service.rs),
- [request logging service](../../crates/gateway-service/src/request_logging.rs),
- [libsql store](../../crates/gateway-store/src/libsql_store/request_logs.rs),
- [PostgreSQL store](../../crates/gateway-store/src/postgres_store/request_logs.rs),
- [AnyStore dispatch](../../crates/gateway-store/src/store.rs),
- [observability handler](../../crates/gateway/src/http/observability.rs).

Why:

- “detail by id” is a resource lookup, not an optional list projection,
- pushing strictness down to the repository/service boundary prevents nullable-success behavior from reappearing in future API layers,
- the existing gateway error envelope already had the right not-found shape, so the nullable wrapper was adding ambiguity without providing real compatibility value.

### 4. Keep the admin UI on the normal fetch-error path for missing request logs

The admin UI now treats a missing request log as a failed fetch instead of a successful null payload:

- [server data adapter](../../crates/admin-ui/web/src/server/admin-data.server.ts),
- [request-log route/dialog](../../crates/admin-ui/web/src/routes/observability/request-logs.tsx),
- [server tests](../../crates/admin-ui/web/src/server/admin-data.server.test.ts),
- [route tests](../../crates/admin-ui/web/src/test/routes/request-logs-route.test.tsx).

Why:

- the UI should mirror the backend resource contract directly,
- a normal error state is easier to reason about than dual success/null semantics,
- removing the nullable branch makes it harder for future admin APIs to copy the same pattern.

## Consequences

Positive:

- upstream-success responses now have one explicit post-success accounting policy regardless of transport,
- request-log detail lookup semantics are stricter and simpler across store, service, API, and UI boundaries,
- observability now distinguishes usage-record failures from generic request success,
- the code paths are less likely to drift back into endpoint-specific or UI-specific special cases.

Tradeoffs:

- a successful upstream response can now complete without a persisted accounting row if the post-success accounting phase fails,
- operators must rely on the new warnings and metrics to detect and remediate that integrity gap,
- strict request-log lookup breaks nullable-success compatibility for any caller that depended on `data: null`, though this was acceptable for the current internal admin API stage.

## Follow-up Work

- Add dashboards and alert thresholds for post-success accounting failures using the new metric.
- Consider whether other admin detail endpoints should adopt the same strict “resource or 404” contract by default.
- Continue documenting request/accounting policy in user-facing runtime docs such as [README.md](../../README.md) whenever the operational contract changes.

## Attribution

This ADR was prepared through collaborative human + AI implementation/design work.
