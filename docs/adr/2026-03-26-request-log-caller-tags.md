# ADR: Request Log Caller Tags Use Summary Columns Plus a Bounded Tag Table

- Date: 2026-03-26
- Status: Accepted
- Related issues:
  - [#59: Add request tagging support for caller-level filtering and attribution](https://github.com/ahstn/oceans-llm/issues/59)
  - [#20: Improve admin request-log filtering and detail UX](https://github.com/ahstn/oceans-llm/issues/20)
  - [#54: Harden chat observability metrics and streamed request-log parsing](https://github.com/ahstn/oceans-llm/issues/54)

## Current state

- [../observability-and-request-logs.md](../operations/observability-and-request-logs.md)

## Context

Before this change, request logs already had a useful split:

- [`request_logs`](../reference/data-relationships.md) held hot summary fields used by the observability list views.
- `request_log_payloads` held sanitized request and response bodies for detail inspection.

That shape worked well for provider, model, latency, status, and payload inspection, but it had a blind spot: teams sharing one API key could not reliably attribute traffic back to the calling service or component. Operators could answer "what happened to this request?" but not "which caller produced this class of requests?" without inferring from payloads or external systems.

Issue [#59](https://github.com/ahstn/oceans-llm/issues/59) addressed that gap by adding caller-supplied request tags that are safe to capture at the gateway and cheap to query later. The implementation needed to solve three related problems at once:

1. Accept and validate caller tags at the HTTP boundary.
2. Persist them in a way that keeps common observability queries fast across both PostgreSQL and libSQL.
3. Expose them through the admin API and UI without overloading existing runtime metadata.

## Decision

We treat caller tags as a first-class request-log contract with a hybrid storage model:

- Universal attribution fields are stored directly on the request-log summary row.
- Bespoke tags are stored in a bounded side table.
- Caller tags are exposed explicitly in API and UI models instead of being hidden inside payload JSON or `metadata_json`.

## How It Works

### HTTP contract and validation

The gateway now accepts four request-tagging headers:

- `x-oceans-service`
- `x-oceans-component`
- `x-oceans-env`
- `x-oceans-tags`

Parsing and validation live in [`request_tags.rs`](../../crates/gateway/src/http/request_tags.rs). This module is intentionally strict:

- universal headers may only appear once
- `x-oceans-tags` may only appear once
- bespoke tags use `key=value; key2=value2`
- bespoke tags are capped at five entries
- duplicate bespoke keys are rejected
- reserved keys `service`, `component`, and `env` cannot be redefined in bespoke tags
- keys and values are ASCII-bounded and format-constrained

These rules keep the data shape stable for indexing, querying, and UI rendering. Invalid tag input fails fast as a `400` at the gateway boundary rather than leaking malformed data into storage.

### Typed request-log model

Caller tags are represented in the shared domain model as [`RequestTags`](../../crates/gateway-core/src/domain.rs) and [`RequestTag`](../../crates/gateway-core/src/domain.rs). That type now travels through the request-log pipeline:

- extracted in [`handlers.rs`](../../crates/gateway/src/http/handlers.rs)
- passed into request logging in [`request_logging.rs`](../../crates/gateway-service/src/request_logging.rs)
- included in query and response types in [`observability.rs`](../../crates/gateway/src/http/observability.rs)

This keeps the contract explicit at the Rust boundary. Future work can evolve the tag model in one place instead of re-parsing or re-encoding ad hoc maps at each layer.

### Storage model

We store the three universal caller dimensions directly on `request_logs`:

- `caller_service`
- `caller_component`
- `caller_env`

We store bespoke tags in `request_log_tags`, keyed by `(request_log_id, tag_key)`.

The schema change is implemented in the active backend baselines:

- [`V17__baseline.sql` for PostgreSQL](../../crates/gateway-store/migrations/postgres/V17__baseline.sql)
- [`V17__baseline.sql` for libSQL](../../crates/gateway-store/migrations/V17__baseline.sql)

Repository implementations were updated in:

- [`postgres_store/request_logs.rs`](../../crates/gateway-store/src/postgres_store/request_logs.rs)
- [`libsql_store/request_logs.rs`](../../crates/gateway-store/src/libsql_store/request_logs.rs)

The read path loads summary rows first, then hydrates bespoke tags in bulk for the returned page. That keeps the hot list query centered on the summary table while still supporting exact-match bespoke tag filters.

### Admin API and UI

The admin observability API now accepts caller-tag filters and returns caller tags on each request-log record in [`observability.rs`](../../crates/gateway/src/http/observability.rs).

The admin UI surfaces those tags in both filtering and display:

- route and filter UI in [`request-logs.tsx`](../../crates/admin-ui/web/src/routes/observability/request-logs.tsx)
- API mapping in [`admin-data.server.ts`](../../crates/admin-ui/web/src/server/admin-data.server.ts)
- shared frontend types in [`api.ts`](../../crates/admin-ui/web/src/types/api.ts)

This matters because attribution is only useful if operators can see and query it without opening raw payloads.

## Why This Shape

### Why not put everything in `metadata_json`?

We rejected a JSON-only design because `metadata_json` already carries runtime-owned observability facts such as operation and stream mode. Mixing caller-supplied attribution into that field would blur ownership and make filtering more backend-specific. It would also encourage future features to treat caller metadata and runtime metadata as interchangeable, which they are not.

### Why not store every tag in a fully exploded tag table?

We rejected an all-tags-in-rows model because three fields are expected to be common, stable filters: service, component, and environment. Requiring a join or subquery for those dimensions would make the most common request-log filters more expensive than necessary and would weaken index clarity.

### Why a hybrid model?

The hybrid model matches observed access patterns:

- service, component, and environment are common attribution dimensions and deserve first-class columns
- bespoke tags are useful, but less common and intentionally bounded
- both storage backends can support this model without backend-specific JSON indexing tricks

This gives us cheap exact-match filters for common cases and bounded extensibility for caller-specific tags.

## Consequences

### Positive

- Caller attribution is now visible from the gateway boundary through the admin UI.
- Common filters stay cheap because the hottest dimensions live on the summary row.
- Bespoke tags remain queryable without making the summary schema unbounded.
- The implementation keeps `metadata_json` semantically cleaner and protects metrics from user-controlled cardinality.

### Tradeoffs

- Request-log writes now touch an extra table when bespoke tags are present.
- Query code is more complex because bespoke tags are hydrated after reading the summary page.
- The bespoke filter path currently supports exact-match filtering for one tag at a time, which is enough for issue [#59](https://github.com/ahstn/oceans-llm/issues/59) but leaves room for future UX work in [#20](https://github.com/ahstn/oceans-llm/issues/20).

## Scope Boundaries

This decision intentionally does not do two things:

- It does not forward caller tags to upstream model providers. The tags terminate at the gateway and are used for local observability only.
- It does not turn caller tags into metrics labels. User-controlled values would create unsafe cardinality growth and reduce the long-term reliability of metrics.

Issue [#54](https://github.com/ahstn/oceans-llm/issues/54) remains adjacent but separate. That issue is about stream parsing and observability hardening; this ADR is about attribution storage and queryability.

## Code Areas To Start With

Future changes in this area will usually start in one of these files:

- HTTP parsing and validation: [`request_tags.rs`](../../crates/gateway/src/http/request_tags.rs)
- request entry points: [`handlers.rs`](../../crates/gateway/src/http/handlers.rs)
- shared domain contract: [`domain.rs`](../../crates/gateway-core/src/domain.rs)
- request-log writing: [`request_logging.rs`](../../crates/gateway-service/src/request_logging.rs)
- observability API: [`observability.rs`](../../crates/gateway/src/http/observability.rs)
- storage repositories: [`postgres_store/request_logs.rs`](../../crates/gateway-store/src/postgres_store/request_logs.rs) and [`libsql_store/request_logs.rs`](../../crates/gateway-store/src/libsql_store/request_logs.rs)
- admin UI route: [`request-logs.tsx`](../../crates/admin-ui/web/src/routes/observability/request-logs.tsx)

## Follow-up Work

- Continue request-log filter ergonomics, pagination, and richer operator workflows under [#20](https://github.com/ahstn/oceans-llm/issues/20).
- Revisit retention and archival policy if caller-tag history materially changes request-log storage costs.
- If provider propagation ever becomes desirable, make it a separate decision with its own explicit privacy and compatibility analysis.
