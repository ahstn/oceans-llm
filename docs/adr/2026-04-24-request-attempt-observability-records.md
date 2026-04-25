# ADR: Request Attempt Observability Records

- Date: 2026-04-24
- Status: Accepted

## Context

Request logs record the final user-visible gateway outcome. That summary is intentionally optimized for filtering by request id, model, provider, owner, status, latency, and usage. It should not become a JSON sink for provider execution details.

The gateway currently keeps single-route execution as the runtime contract. Configurable retry and fallback behavior is tracked separately in issue #118 because automatic retries can affect cost, latency, idempotency, and streaming semantics.

## Decision

Add `request_log_attempts` as a child table of `request_logs`.

Each row records one upstream provider execution attempt:

- request and parent log identity,
- attempt number,
- route id,
- provider key,
- upstream model,
- bounded status fields,
- retryability classification,
- whether the attempt produced the final response,
- stream flag,
- provider-attempt timing.

The first implementation records the current single provider attempt only. It does not introduce retry or fallback execution. Pre-provider failures such as authentication rejection, route capability mismatch, and budget hard-limit rejection do not create attempt rows because no upstream provider target was attempted.

Attempt rows are written atomically with the request-log summary, payload, and tags. If request logging is disabled for the authenticated owner and no summary row is written, no attempt rows are written.

The admin request-log detail API returns attempts ordered by `attempt_number ASC`, and the admin UI renders them in the request-log detail dialog.

## Consequences

Positive:

- provider execution diagnostics are explicit and queryable,
- final request summaries stay small and filter-friendly,
- the schema is ready for future configurable retry/fallback behavior without adding ad hoc JSON blobs,
- operators can distinguish pre-provider failures from upstream provider failures.

Trade-offs:

- request-log writes include another child table,
- request-log detail responses grow by a small metadata array,
- old request logs may have no attempts and are shown honestly rather than synthesized.

## Follow-up

Issue #118 will define and implement configurable retry/fallback execution policy. That work should keep retries disabled by default and document the cost, latency, idempotency, and streaming constraints before changing runtime behavior.
