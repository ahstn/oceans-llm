# Request Logs

`See also`: [Observability and Request Logs](../observability-and-request-logs.md), [MCP Invocations](mcp-invocations.md), [Request Lifecycle and Failure Modes](../../reference/request-lifecycle-and-failure-modes.md), [Data Relationships](../../reference/data-relationships.md), [Admin Control Plane](../../access/admin-control-plane.md)

Request logs are the primary operator view for one gateway request. They are request-scoped, not tool-scoped: each row describes the user-visible API outcome, selected model route, owner context, payload capture state, and bounded tool cardinality.

## Admin UI Route

Use `/admin/observability/request-logs`.

The page supports:

- list filtering by caller service, component, environment, and one bespoke tag pair
- request id visibility for correlation with traces, logs, and MCP invocation records
- detail inspection for sanitized request and response payloads
- payload policy visibility, including capture mode, byte limits, stream event limit, policy version, and truncation flags
- provider-attempt inspection for upstream execution metadata
- MCP/tool cardinality fields for exposed, invoked, filtered, and referenced MCP server counts

When MCP invocation logging is available, request-log detail links to `/admin/observability/mcp-invocations?request_id=<request_id>` so admins can move from request outcome to individual tool calls.

## API Contract

Current endpoints:

- `GET /api/v1/admin/observability/request-logs`
- `GET /api/v1/admin/observability/request-logs/{request_log_id}`

Current list filters:

- `page`
- `page_size`
- `request_id`
- `model_key`
- `provider_key`
- `status_code`
- `user_id`
- `team_id`
- `service`
- `component`
- `env`
- `tag_key`
- `tag_value`

## Storage Boundary

Request-log data is split across:

- `request_logs` for hot summary fields
- `request_log_payloads` for sanitized payload bodies
- `request_log_tags` for bounded caller tags
- `request_log_attempts` for upstream provider attempts

MCP tool execution rows are intentionally separate from request logs. Request logs keep cardinality counts and correlation ids; MCP invocation records own per-tool status, policy result, latency, and redacted argument/result metadata.

## Failure Reading

Pre-provider failures such as `budget_error`, `invalid_request`, and `no_routes_available` can produce request-log summary rows with no provider attempts. Provider-backed failures can include one or more request-attempt rows once retry/fallback execution exists.

Missing detail rows return `404 not_found`.

## What This Page Does Not Own

- MCP per-tool invocation records: [mcp-invocations.md](mcp-invocations.md)
- payload redaction policy details: [Observability and Request Logs](../observability-and-request-logs.md)
- request path and failure classes: [Request Lifecycle and Failure Modes](../../reference/request-lifecycle-and-failure-modes.md)
