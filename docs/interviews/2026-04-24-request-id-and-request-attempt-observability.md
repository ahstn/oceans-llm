# Request ID and Request Attempt Observability Interview

`See also`: [Request ID and Request Attempt Observability Design](../superpowers/specs/2026-04-24-request-id-and-request-attempt-observability-design.md), [Observability and Request Logs](../operations/observability-and-request-logs.md), [Request Lifecycle and Failure Modes](../reference/request-lifecycle-and-failure-modes.md)

Date: 2026-04-24

## Scope

This interview aligned implementation decisions for the combined GitHub issue #17 and #19A work:

- #17: unify gateway request-id generation and propagation.
- #19A: add first-class request-attempt observability records while preserving current single-attempt runtime behavior.
- #118 remains a follow-up for configurable retry/fallback execution policy.
- #18 is out of scope because request-log payload policy was merged separately.

## Decisions

### Packaging

- Ship #17 and #19A in one PR.
- Keep implementation internally phased:
  1. canonical request ID cleanup,
  2. request-attempt schema/API/UI/docs.
- Explicitly exclude #118 retry/fallback execution behavior from this PR.
- Write a combined design spec and proceed directly to implementation.

### Request ID Ownership

- HTTP middleware is the single request-id owner.
- Handlers consume Tower's `RequestId` extension instead of reading or generating `x-request-id` themselves.
- If a handler does not receive the `RequestId` extension, return `500 internal_error` and log a pipeline invariant violation.
- Remove handler-local UUID fallback generation.
- Remove handler-local `x-request-id` response insertion if tests confirm `PropagateRequestIdLayer` covers JSON, streaming, and error responses.
- Preserve caller-provided `x-request-id` values as-is; do not add validation/normalization in this scope.
- Middleware still applies globally, but handler-level consumption is limited to provider-execution `/v1/*` endpoints.
- Remove stale tracing span fields `attempt_count` and `fallback_used` from the HTTP middleware span.
- Tests should cover success, stream, error, provided header, and missing header behavior.

### Request Attempt Scope

- #19A records attempts for current single-attempt runtime only.
- No retry/fallback behavior change in this PR.
- No request-attempt rows for pre-provider failures such as auth failure, model not found, capability mismatch, or budget hard-limit rejection.
- Attempts are children of request logs. If request logging is disabled and no summary row is written, no attempt rows are written.
- Attempt metadata is written atomically with the request summary/payload/tags.
- Do not add fallback attempt-only persistence if request-log persistence fails.

### Attempt Lifecycle

- For non-streaming calls, build the attempt in memory and insert it with the final request log after the provider outcome is known.
- For streaming calls, keep attempt state in the stream wrapper and insert it with the final request log when the stream completes or fails.
- Provider stream-start failures create one failed attempt with status `stream_start_error`, `stream=true`, and `produced_final_response=false`.
- Mid-stream failures create one failed attempt with status `stream_error`, `stream=true`, and `produced_final_response=false`.
- Cleanly completed streams create one successful attempt with `produced_final_response=true`.
- Provider execution success with later accounting failure still records attempt status `success`.

### Attempt Data Model

- Table name: `request_log_attempts`.
- Primary key: `request_attempt_id`.
- Include `request_log_id` foreign key and duplicate `request_id` for correlation.
- Attempt numbers start at `1`.
- Include route/provider target fields:
  - `route_id`,
  - `provider_key`,
  - `upstream_model`.
- Do not duplicate gateway resolved model key; parent request log owns requested/resolved gateway model identity.
- Store metadata only, not provider request/response payloads.
- Include fixed status enum/check constraint:
  - `success`,
  - `provider_error`,
  - `stream_start_error`,
  - `stream_error`.
- Include:
  - `status_code`, using gateway-mapped HTTP status,
  - `error_code`, using `GatewayError::error_code()`,
  - bounded redacted `error_detail`,
  - `error_detail_truncated`,
  - `retryable`,
  - `terminal`,
  - `produced_final_response`,
  - `stream`,
  - `started_at`,
  - `completed_at`,
  - `latency_ms`,
  - `metadata_json` default `{}`.
- Error detail limit: 2 KiB after redaction/truncation.
- Make truncation visible through `error_detail_truncated`.
- Attempt latency measures provider execution time only, not full gateway request duration.
- Store both timestamps and latency.
- Add minimal indexes only:
  - parent/detail ordering by `(request_log_id, attempt_number)`,
  - request-id correlation.

### Store and Service Boundaries

- Use a separate `RequestAttemptRepository` trait for attempt reads.
- Add an atomic aggregate insert method on `RequestLogRepository`, such as `insert_request_log_with_attempts(log, payload, attempts)`, for summary/payload/tags/attempt writes.
- Keep attempt-specific read/list methods on `RequestAttemptRepository`.
- `gateway-service` owns attempt construction/finalization; handlers pass provider outcome facts.
- Rename `ChatRequestLogContext` to a generalized request-log context.
- Clean up touched chat-specific request-log lifecycle names that now apply to chat, Responses, and embeddings.

### Embeddings Logging

- Bring `/v1/embeddings` into the generalized request-log lifecycle.
- Use the same payload envelope as other non-stream operations:
  - request: `{ headers, body }`,
  - response: `{ body }`.
- Apply the merged #18 redaction/truncation/capture policy.
- Populate usage fields from response `usage` when available.
- Record embeddings attempts because embeddings is a provider-execution endpoint.
- Do not create attempts for `GET /v1/models`.

### Admin API and UI

- Include attempts in the existing request-log detail response as `attempts: RequestAttemptView[]`.
- Guarantee attempts are sorted by `attempt_number ASC`.
- Use formatted timestamp strings consistent with existing request-log timestamp fields.
- Expose `route_id` in the admin API.
- Do not add request-attempt filters to the list API in #19A.
- Do not add list-page attempt indicators in #19A.
- In the request-log detail dialog, place Attempts between summary/tags and payload cards.
- Use existing shadcn `Table` for the new attempts section.
- Do not refactor the existing virtualized request-log list.
- Show visible default fields:
  - attempt number,
  - status,
  - provider key,
  - upstream model,
  - latency,
  - retryable,
  - terminal,
  - produced final response.
- Show route ID and error detail as secondary/expanded text.
- Show a neutral empty state if no attempts exist; do not synthesize legacy attempts.
- Reuse parent/request-log icon context for #19A; do not add per-attempt icon metadata yet.

### Migrations and Docs

- Add forward `V19__request_log_attempts.sql` migrations for libsql and PostgreSQL.
- Update migration registry.
- Do not edit only `V17__baseline.sql` as the primary migration mechanism.
- Create an ADR for request-attempt observability records.
- Update canonical docs:
  - request lifecycle,
  - observability and request logs,
  - data relationships,
  - model routing/API behavior as needed.
- Mention #118 only as a follow-up; do not document retry/fallback config knobs that do not exist yet.
- Keep request-attempt records after payload retention/purge; attempts are summary-adjacent metadata and should be deleted only with parent request logs.

### Validation

Plan for full mixed validation:

- `mise run lint`
- `mise run test`
- `mise run admin-contract-generate`
- `mise run admin-contract-check`
- docs checks as applicable
