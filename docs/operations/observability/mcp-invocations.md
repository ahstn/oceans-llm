# MCP Invocations

`See also`: [Observability and Request Logs](../observability-and-request-logs.md), [MCP Registry and Discovery](mcp-registry-and-discovery.md), [Request Logs](request-logs.md), [Identity and Access](../../access/identity-and-access.md), [Admin Control Plane](../../access/admin-control-plane.md), [Data Relationships](../../reference/data-relationships.md), [Request Lifecycle and Failure Modes](../../reference/request-lifecycle-and-failure-modes.md)

MCP invocation logs are the durable audit view for individual MCP tool calls. They are narrower than request logs: one request can produce zero, one, or many tool invocation rows.

## Admin UI Route

Use `/admin/observability/mcp-invocations`.

The page is intended to support filters for:

- request id
- MCP server display key/name
- tool display key/name
- API key id
- user id
- team id
- status
- policy result
- time range

The list shows owner context, server/tool identity, status, policy result, latency, error code, and payload redaction/truncation flags. The detail view shows sanitized arguments and sanitized result payloads when the MCP invocation payload policy captures them.

## API Contract

The admin UI slice uses these endpoints:

- `GET /api/v1/admin/observability/mcp-invocations`
- `GET /api/v1/admin/observability/mcp-invocations/{mcp_tool_invocation_id}`

Expected list filters:

- `request_id`
- `server_display_key`
- `server_display_name`
- `tool_display_key`
- `tool_display_name`
- `api_key_id`
- `user_id`
- `team_id`
- `status`
- `policy_result`
- `occurred_at_start` (RFC3339 timestamp)
- `occurred_at_end` (RFC3339 timestamp)

Expected statuses:

- `success`
- `unauthorized`
- `policy_denied`
- `upstream_error`
- `gateway_error`
- `timeout`
- `invalid_request`

Expected policy results:

- `allowed`
- `denied`
- `not_evaluated`

The admin UI consumes these schemas from the generated admin OpenAPI artifact.

## Audit Fields

Each invocation record should carry:

- request correlation: `request_id`
- owner context: owner kind, API key id, user id, and team id when known
- MCP target identity: nullable stable IDs plus required server/tool display keys and names
- outcome: status, error code, latency, and policy result
- payload state: `has_payload`, argument/result redaction flags, and argument/result truncation flags
- occurrence time

Arguments and results must be redacted and bounded before persistence. Sensitive headers, tokens, provider credentials, OAuth material, and API keys must never be stored in MCP invocation payloads.

`server_id` and `tool_id` are nullable today so invocation logging can operate before registry-backed execution exists. Future execution should populate them from the external MCP registry's stable server and tool ids.

## Relationship to Request Logs

Request logs keep the request-level outcome and tool cardinality. MCP invocation logs keep per-tool audit detail.

`request_id` is the durable correlation key. `request_log_id` is an optional non-owning link when the request-log row is known; it is not required for insertion because request-log summaries are written at final outcome and may be absent or purged independently.

Use request logs first when debugging the model/API request. Use MCP invocation logs when the question is which tool ran, whether access policy allowed it, how long it took, and whether the tool result failed or was truncated.

## What This Page Does Not Own

- request-log payload policy and stream parsing: [Observability and Request Logs](../observability-and-request-logs.md)
- request lifecycle failure classes: [Request Lifecycle and Failure Modes](../../reference/request-lifecycle-and-failure-modes.md)
- user, team, and API-key ownership policy: [Identity and Access](../../access/identity-and-access.md)

## Validation

Run `mise run docs:check` before handing off documentation changes.
