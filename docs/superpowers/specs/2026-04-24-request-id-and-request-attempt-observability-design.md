# Request ID and Request Attempt Observability Design

`See also`: [Request ID and Request Attempt Observability Interview](../../interviews/2026-04-24-request-id-and-request-attempt-observability.md), [ADR: Request Attempt Observability Records](../../adr/2026-04-24-request-attempt-observability-records.md), [Observability and Request Logs](../../operations/observability-and-request-logs.md), [Request Lifecycle and Failure Modes](../../reference/request-lifecycle-and-failure-modes.md)

Date: 2026-04-24

## Issues

- GitHub issue #17: unify gateway request-id generation and propagation.
- GitHub issue #19: add first-class request-attempt observability records.
- GitHub issue #118: follow-up for configurable retry/fallback execution policy, explicitly out of scope here.

## Goals

1. Make HTTP middleware the only gateway request-id generation/propagation owner.
2. Remove handler-local request-id fallback generation and duplicate response-header propagation.
3. Add persisted request-attempt records for provider execution attempts without changing current single-attempt runtime behavior.
4. Expose attempt metadata through request-log detail APIs and the admin UI.
5. Generalize request-log lifecycle naming and bring embeddings into the same lifecycle used by chat and Responses.
6. Document the new request identity and attempt observability contract.

## Non-goals

- No retry/fallback execution behavior in this change.
- No synthetic attempt rows for old logs or pre-provider failures.
- No per-attempt payload storage.
- No request-attempt filtering/list analytics.
- No request-id validation or normalization beyond existing middleware behavior.

## Request ID Design

`SetRequestIdLayer` remains the boundary that preserves a caller-provided `x-request-id` or creates one with `MakeRequestUuid`. Handlers that need the request ID consume Tower's `RequestId` extension. A missing extension is an internal pipeline invariant violation and returns `500 internal_error` rather than silently creating another ID.

`PropagateRequestIdLayer` remains responsible for writing `x-request-id` onto responses. Handler-local response header insertion is removed if tests confirm propagation for JSON, streaming, and error responses.

Stale tracing span fields `attempt_count` and `fallback_used` are removed from the HTTP span. Attempt observability is introduced through explicit request-attempt records instead of fallback-era span placeholders.

## Attempt Data Model

Add `request_log_attempts` as a child table of `request_logs`.

Fields:

- `request_attempt_id` primary key
- `request_log_id` foreign key with cascade delete
- `request_id` duplicated for correlation
- `attempt_number`, starting at `1`
- `route_id`
- `provider_key`
- `upstream_model`
- `status`: `success`, `provider_error`, `stream_start_error`, `stream_error`
- `status_code`
- `error_code`
- `error_detail`
- `error_detail_truncated`
- `retryable`
- `terminal`
- `produced_final_response`
- `stream`
- `started_at`
- `completed_at`
- `latency_ms`
- `metadata_json`, default `{}`

Indexes stay minimal:

- unique/order by `(request_log_id, attempt_number)`
- request-id correlation index

Attempt rows describe upstream provider execution only. Pre-provider failures such as auth rejection, model grant failure, capability mismatch, route unavailability, and budget rejection have zero attempts.

## Attempt Lifecycle

The service layer owns attempt construction and finalization. Handlers pass provider outcome facts and timing boundaries.

Non-stream attempts are built in memory around the provider call and inserted atomically with the final request log.

Streaming attempts are carried by the stream wrapper and finalized when the stream completes or fails. Stream-start failures before returning a response are recorded as `stream_start_error`. Mid-stream failures are recorded as `stream_error`. Clean stream completion records `success`.

Attempt status describes provider execution only. Post-success accounting failures do not change successful attempt status.

Attempt writes happen only when request summary logging writes. If user logging preferences suppress the summary row, no attempts are written.

## Store and Service Boundaries

Add a separate `RequestAttemptRepository` trait for attempt detail reads. Extend the request-log aggregate write boundary to insert summary, payload, tags, and attempts in one transaction.

The existing chat-specific request-log context is renamed to a generalized request-log context. Touched request-log lifecycle methods are renamed where they now apply to chat, Responses, and embeddings.

Embeddings moves into the generalized request-log lifecycle with the same payload envelope and policy as other non-stream operations.

## Admin API and UI

`GET /api/v1/admin/observability/request-logs/{request_log_id}` returns attempts in ascending attempt-number order.

The admin UI request-log detail dialog gets an Attempts section between summary/tags and payload cards. The new section uses existing shadcn Table components and shows a neutral empty state when no attempts exist. The existing virtualized request-log list is not refactored.

## Documentation

Add an ADR for request-attempt observability records. Update canonical docs for observability, lifecycle, and data relationships. Mention #118 as future configurable retry/fallback work without documenting unimplemented knobs.

## Testing

Request ID tests cover:

- provided request ID is preserved,
- missing request ID is generated by middleware,
- response headers match logs/ledger where applicable,
- error responses propagate request ID,
- streaming responses propagate request ID.

Attempt tests cover:

- non-stream success,
- non-stream provider failure,
- stream-start failure,
- mid-stream failure,
- stream success,
- embeddings success,
- pre-provider failures write no attempts,
- request logging disabled writes no attempts,
- libsql and PostgreSQL store insert/list ordering.

## Validation

Run the relevant full checks before handoff:

- `mise run admin-contract-generate`
- `mise run admin-contract-check`
- `mise run docs-check` or docs verification task
- `mise run lint`
- `mise run test`
