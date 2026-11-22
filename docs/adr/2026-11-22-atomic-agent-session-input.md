# Atomic agent-session input and bounded analysis work

- Status: accepted
- Date: 2026-11-22

## Context

Passive correlation used separate request-link and observation transactions. Each took an exclusive session-row lock. Request appends counted all request rows and found the maximum ordinal; observation appends scanned every prior JSON fact to enforce the nested-fact budget. A failure between commits could leave a request without its observations.

The isolated baseline load run exposed a PostgreSQL lock cycle between session `FOR UPDATE` waiters and the `FOR KEY SHARE` check taken by an analysis-queue foreign key. Its duplicate-open-window errors came from a later update that restored a stale `open` lifecycle, not from the initial insert. The arbitrary window-update operation has been removed.

## Decision

Persist a request and its observations through one `append_agent_session_input` repository operation. Observation-only input remains a supported use case for parser recomputation. Request-only input supports records without captured observations. These are explicit input shapes, not compatibility wrappers around the previous APIs.

Both backends store session-local request count, next ordinal, nested-fact count and input generation. Migration V85 backfills them from existing child rows. It requires a V85-aware reader because an older writer would not maintain the counters.

PostgreSQL takes one `FOR NO KEY UPDATE` session lock. The primary key does not change, so this lock serialises session writers while remaining compatible with foreign-key readers. Source writes finish before the input transaction. The transaction then visits the session, request and observation rows in that order. libSQL uses one independent-connection `BEGIN IMMEDIATE` write transaction.

Prepare JSON and bounded fact counts before taking the writer lock. Insert observations in batches rather than one statement per observation. Maintain counters and the event-time watermark in the same transaction as the new facts. A replay consumes neither a counter nor an ordinal. A duplicate request is checked before rejecting a closed window, so an exact replay remains idempotent after finalisation. Conflicting facts fail and roll back the entire input.

The repository, not a preliminary service count, enforces the 1,000-request limit. It finalises a full window and reports closure; the service resolves the current open window or races to create one through the existing partial unique index. A losing creator loads the winning window. Finalisation cannot restore an open lifecycle.

The nested-fact budget remains the combined count of supplied tools, supplied skills and file interactions, capped at 2,048. The store chooses an explicitly supplied truncated variant when needed and rejects a fallback that would still grow the budget past its limit. Existing oversized historical data can receive zero-nested-fact observations without growing further. Observation-set replay compares identity and facts, not incidental vector order.

Input generation advances once for each committed input change and on finalisation, even when an out-of-order request does not advance the maximum timestamp. Event time remains the source for request ordering and window display. Retention removes child facts and resets their counters in one write transaction; PostgreSQL locks the affected sessions before their children.

## Consequences

The session row still serialises writers for a busy conversation. This is necessary for window capacity, persisted ordering and a shared nested-fact budget, but its lock no longer covers whole-history counting or JSON serialisation. Scalar counters require migration and transactional maintenance on every mutation path.

A new input and its observations now commit or roll back together. Production ingestion also schedules durable analysis in that transaction. A queue-write failure rolls back the input. Request-log annotation runs after the commit. [Generation-fenced analysis](./2026-11-22-generation-fenced-agent-session-analysis.md) defines job coalescing and guarded report publication.

Contract coverage runs on both libSQL and PostgreSQL. It covers migration backfill with non-contiguous ordinals, duplicate replay, capacity, combined nested budget, rollback on an observation conflict, concurrent open-window creation and irreversible finalisation. A PostgreSQL contract also holds a foreign-key-compatible reader lock while appending input, so replacing the writer lock with an incompatible mode causes a failure.
