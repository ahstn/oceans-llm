# ADR: MCP Tool Cardinality Observability

## Status

Accepted.

## Context

Admins need request-level visibility into how many tools are exposed to a model and how many tool calls are invoked. This is a prerequisite for MCP governance, but the gateway does not yet have a dedicated MCP access and filtering subsystem.

The MCP tools model separates tool exposure through `tools/list` from invocation through `tools/call`. The gateway can observe analogous OpenAI-compatible Chat Completions and Responses traffic today, but it cannot yet authoritatively report MCP server references or filtered tool counts.

Storing these values in generic request metadata would make the admin API and UI depend on optional ad hoc keys. Adding compatibility fallbacks for old metadata shapes would preserve a pattern that the request-log contract is intentionally moving away from.

## Decision

Tool cardinality is stored as typed nullable request-log data.

Implementation points:

- `RequestToolCardinality` is part of the request-log domain record.
- `request_logs` has explicit nullable columns for referenced MCP servers, exposed tools, invoked tools, and filtered tools.
- Chat Completions counts exposed tools from the top-level request `tools` array.
- Responses counts exposed tools from `request.tools` after the request body is serialized for logging.
- Non-stream responses count invoked tools from normalized response bodies.
- Streaming responses count invoked tools while the existing SSE request-log collector parses frames.
- MCP server and filtered counts remain `null` until a real MCP access/filtering layer supplies those facts.
- Admin DTOs expose typed fields on request-log summaries and leaderboard rows.
- Leaderboard averages use recorded-only denominators for each dimension.

## Consequences

Benefits:

- real zeroes are distinguishable from unknown historical or not-yet-observable data
- admin UI and generated API types read stable fields instead of metadata conventions
- future MCP access work can populate the existing columns without changing the request-log contract again
- metrics remain bounded by dimension name and operation, without per-tool or per-server labels

Trade-offs:

- v1 does not identify individual tools or MCP servers
- MCP-specific dimensions intentionally appear as `n/a` until the gateway owns the corresponding access decisions
- malformed non-array `tools` values are counted as zero rather than traversed or guessed

## Follow-Up

- Add first-class MCP server/tool access records.
- Populate referenced MCP server counts from the MCP access layer.
- Populate filtered tool counts from explicit allow/deny decisions.
- Add per-tool/server drill-down only after a bounded, privacy-aware aggregation design exists.
