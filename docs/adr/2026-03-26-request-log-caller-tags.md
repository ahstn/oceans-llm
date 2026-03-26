# ADR: Request Log Caller Tags Use Summary Columns Plus a Bounded Tag Table

- Date: 2026-03-26
- Status: Accepted

## Context

Issue #59 adds request tagging so callers sharing one API key can still split traffic by service, component, environment, or other bounded dimensions.

The existing observability model already separates:

- `request_logs` for hot summary reads
- `request_log_payloads` for sanitized request/response bodies

That split keeps common request-log scans cheap. Adding caller tags directly into payload JSON or generic metadata would make filtering backend-specific and harder to reason about, while storing every tag only as exploded rows would make common filters more expensive than they need to be.

## Decision

### 1. Treat caller tags as a first-class request-log contract

The gateway accepts:

- `x-oceans-service`
- `x-oceans-component`
- `x-oceans-env`
- `x-oceans-tags`

The bespoke tag header uses `key=value; key2=value2` formatting and is capped at five tags.

### 2. Store universal tags on the `request_logs` summary row

The summary table now owns:

- `caller_service`
- `caller_component`
- `caller_env`

Why:

- these are the most common filter dimensions
- exact-match filters should not require JSON inspection or a join
- both libsql and PostgreSQL can index these fields directly

### 3. Store bespoke tags in a bounded side table

The gateway stores bespoke tags in `request_log_tags` keyed by `(request_log_id, tag_key)`.

Why:

- bespoke tags remain queryable without overloading `metadata_json`
- the write amplification is bounded to at most five rows per request
- exact-match lookups can use a dedicated `(tag_key, tag_value, request_log_id)` index

### 4. Keep caller tags out of runtime metadata and metrics labels

`metadata_json` remains runtime-owned observability metadata such as operation, stream mode, and fallback count. Caller-supplied tags are exposed through explicit request-log fields instead of being mixed into runtime metadata.

Caller tags are not promoted to metric labels.

Why:

- runtime metadata should stay semantically stable for maintainers
- user-controlled values would create unsafe metric cardinality growth

## Consequences

Positive:

- common request-log filters stay cheap and explicit
- caller tags remain visible in list/detail APIs and the admin UI
- storage growth from bespoke tags stays bounded and understandable

Tradeoffs:

- request-log writes now touch one additional table when bespoke tags are present
- backend query code is slightly more complex because page reads hydrate bespoke tags after the summary scan

## Follow-up Work

- Extend broader request-log UX work in issue #20 around additional filter ergonomics and pagination.
- Revisit retention and archival policy if request-log volume makes tag history expensive to retain indefinitely.

## Attribution

This ADR was prepared through collaborative human + AI implementation/design work.
