# Structured Request-Log Bounding

- Status: Accepted
- Date: 2026-08-25

## Decision

Oceans measures stored request budgets as uncompressed serialized JSON bytes. The normal persisted-request cap is 128 KiB, and 256 KiB is the absolute inline request ceiling. These values are gateway operating limits, not database limits.

Request analysis and request storage use separate projections. Agent Session Analysis reads the structured redacted request before storage bounding. The stored projection reserves a diagnostic envelope before it assigns the remaining budget to input content.

The gateway keeps the current request-log storage layout. This decision does not add split columns, a tool-schema table, or tool-schema deduplication.

## Implementation

- Requests within the configured cap keep the existing fast path after one serialized-size check.
- Stored headers use an explicit diagnostic allow-list. Other headers are removed before persistence.
- The essential envelope retains session and lineage identifiers, model and reasoning settings, tool names and bounded schema shape, stream and include settings, cache metadata, and message or item identity fields.
- Oversized requests first truncate text-bearing input leaves with UTF-8-safe head and tail retention. A total content budget enforces the final serialized request cap.
- If the envelope is too large, the gateway compacts verbose tool descriptions, schema descriptions, examples, and defaults before it removes essential diagnostic fields.
- Top-level truncation metadata records the strategy version, original and stored sizes, truncated-field count, omitted bytes, bounded affected paths, and tool or known-large-field compaction counts.
- A complete-payload marker is the final safety fallback when the bounded envelope cannot fit.
- Response truncation keeps its existing budget and semantics.

## Why

Database compression does not protect gateway serialization latency, API transfer, JSON parsing, or admin UI rendering. Structure-preserving request bounds keep session evidence and debugging context available without retaining an unbounded payload.

The 64 KiB preferred range, 128 KiB normal cap, and 256 KiB absolute ceiling are an engineering inference from representative gateway requests and common observability limits. They are not limits imposed by PostgreSQL, SQLite, or one logging vendor.

## Trade-offs

- Oversized prompt text can lose bytes, but the gateway keeps its item structure and an explicit omission marker.
- Tool-heavy requests can lose verbose descriptions, examples, and defaults while tool names and schema shape remain available.
- The final fallback can remove the structured envelope when the bounded envelope itself cannot fit.
- Counting serialized JSON bytes adds work only for oversized requests after the normal size check.
- The current single-column layout avoids migration and join costs, but it does not deduplicate repeated tool schemas.

## Rejected Alternatives

### Use compressed database size as the request budget

Rejected because compression occurs after gateway serialization and does not reduce API transfer, JSON parsing, or UI costs.

### Truncate the complete request first

Rejected because it removes session headers and structured request facts that Agent Session Analysis and operators need.

### Split request data across columns or tool-schema tables

Rejected for this change because structured bounding meets the required operating limit without a storage migration or a new consistency boundary.

## Follow-up

- Verify the effective configured cap in each deployment.
- Track stored-request size percentiles and aim for P95 at or below 64 KiB.
- Re-run the request-bounding measurement harness when request shapes or provider schemas change.
- Reconsider schema normalization only if production storage or query evidence justifies the added complexity.
