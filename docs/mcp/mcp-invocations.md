# MCP Invocations

`See also`: [Observability and Request Logs](../operations/observability-and-request-logs.md), [MCP Registry and Discovery](../contributing/mcp/mcp-registry-and-discovery.md), [Request Logs](../operations/observability/request-logs.md), [Identity and Access](../access/identity-and-access.md), [Admin Control Plane](../access/admin-control-plane.md), [Data Relationships](../contributing/reference/data-relationships.md), [Request Lifecycle and Failure Modes](../reference/request-lifecycle-and-failure-modes.md)

MCP invocation logs are the durable audit view for individual MCP tool calls. They are narrower than request logs: one request can produce zero, one, or many tool invocation rows.

## Admin UI Route

Use `/admin/observability/mcp-invocations`.

The page supports filters for:

- request id
- MCP server display key/name
- target: the tool, resource URI, or prompt name
- JSON-RPC method
- API key id
- user id
- team id
- status
- policy result
- time range

The list shows owner context, server, JSON-RPC method, target, status, policy result, latency, error code, and payload redaction/truncation flags. The detail view shows sanitized arguments and sanitized result payloads when the MCP invocation payload policy captures them.

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
- `mcp_method`
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
- tool version: for `tools/call`, the served schema version of the called tool (`tool_schema_version`, shown as **Tool version** in the detail view). It is set when the client saw the gateway's stored definition: on aggregate `call_tool`, and on direct calls to servers that require approval or to a tool whose change still waits. A direct call that passes the live upstream definition through records none, because the upstream may have changed since the last discovery. See [Tool versions and approval](mcp-tool-access.md#tool-versions-and-approval).

The gateway must redact and bound arguments and results before persistence. It must never store sensitive headers, tokens, provider credentials, OAuth material, or API keys in MCP invocation payloads.

`server_id` and `tool_id` are nullable so admins can still audit policy-denied, unknown, or inactive tool names. Successful registry-backed `tools/call` executions populate stable server and tool ids.

Aggregate `/mcp` `search_tools` and `describe_tool` calls are discovery operations and do not create MCP invocation rows. Aggregate `call_tool` and direct `/mcp/{server_key}` mediated `tools/call` executions create invocation rows.

A call rejected by a [rate limit](../configuration/rate-limits.md) is recorded with status `policy_denied`, policy result `denied`, and error code `rate_limited`. Its `metadata.rate_limit` names the policy, scope kind, metric, period, limit, and retry delay. Grant denials (`mcp_tool_not_granted`) and guardrail denials (`guardrail_policy_denied`) use the same status, so filter by error code to tell them apart.

## Relationship to Request Logs

Request logs keep the request-level outcome and tool cardinality. MCP invocation logs keep per-tool audit detail.

`request_id` is the durable correlation key. `request_log_id` is an optional non-owning link when a request-log row exists. Insertion does not require it because the gateway writes request-log summaries at the final outcome and may omit or purge them separately.

Use request logs first when debugging the model/API request. Use MCP invocation logs to find which tool ran, whether access policy allowed it, how long it took, and whether truncation or another error affected the tool result.

The gateway logs policy-denied `tools/call` requests before upstream execution. It logs permitted calls with `allowed`; upstream failures, timeouts, and invalid requests keep their distinct status values.

## What This Page Does Not Own

- request-log payload policy and stream parsing: [Observability and Request Logs](../operations/observability-and-request-logs.md)
- request lifecycle failure classes: [Request Lifecycle and Failure Modes](../reference/request-lifecycle-and-failure-modes.md)
- user, team, and API-key ownership policy: [Identity and Access](../access/identity-and-access.md)

## Validation

Run `mise run //docs:build` before handing off documentation changes.
