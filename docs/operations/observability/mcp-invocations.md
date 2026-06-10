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
- `parent_invocation_id`
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
- parent linkage: nullable `parent_invocation_id` for nested Code Mode tool calls
- owner context: owner kind, API key id, user id, and team id when known
- MCP target identity: nullable stable IDs plus required server/tool display keys and names
- outcome: status, error code, latency, and policy result
- payload state: `has_payload`, argument/result redaction flags, and argument/result truncation flags
- occurrence time

Arguments and results must be redacted and bounded before persistence. Sensitive headers, tokens, provider credentials, OAuth material, and API keys must never be stored in MCP invocation payloads.

`server_id` and `tool_id` are nullable so policy-denied, unknown, or inactive tool names can still be audited. Successful registry-backed `tools/call` executions populate stable server and tool ids.

Aggregate `/mcp` `search_tools` and `describe_tool` calls are discovery operations and do not create MCP invocation rows. Aggregate `call_tool` and direct `/mcp/{server_key}` mediated `tools/call` executions create invocation rows.

## Code Mode Invocations

The `/code-mode-mcp` surface logs at two levels.

Every `explore` and `execute` tool call writes a parent invocation row, even though the corresponding aggregate discovery operations are not logged: Code Mode runs model-authored code, so each execution is auditable. Parent rows use a synthetic gateway identity with no registry-backed ids:

- `server_display_key = "code-mode"`, `server_display_name = "Code Mode"`
- `tool_display_key = "explore" | "execute"`
- `server_id` and `tool_id` are null
- metadata carries `mcp_route = "code-mode"`

Parent-row statuses map execution outcomes:

- `success`: the code ran to completion
- `policy_denied` with error code `capability_denied`: the execution's uncaught error was a host-function call outside its capability profile, for example `oceans.callTool` from `explore`; a capability denial caught and handled by the code does not affect the parent status (a later unrelated failure is logged as `gateway_error`)
- `gateway_error` with error code `code_execution_error`: the code threw or failed in the sandbox
- `gateway_error` with error code `code_mode_executor_error`: the sandbox backend itself failed
- `timeout`: epoch preemption or wall-clock expiry
- `invalid_request`: the `code` argument was missing or empty; the executor never ran

Each nested `oceans.callTool` execution writes an ordinary invocation row with real `server_id`/`tool_id` identity and its `parent_invocation_id` set to the parent row. Nested rows keep their normal statuses (`success`, `unauthorized`, `upstream_error`, `timeout`, `gateway_error`). Use the `parent_invocation_id` list filter to retrieve all nested calls for one execution.

Redaction guarantees and limits:

- the parent row's arguments payload is the submitted `code` string **stored as-submitted**, and its result payload carries the execution result, error, captured console log lines, and truncation flag. Both payloads pass through the same capture-mode, byte-bounding, and key-based redaction policy as every other invocation payload, but key-based redaction cannot scrub secrets embedded *inside* an opaque string: a credential written into the code text or printed via `console.log` is persisted in plaintext. Callers must not embed secrets in submitted code; secrets belong in upstream credential bindings, which never enter the sandbox
- operators who cannot accept code/log persistence can disable MCP invocation payload capture entirely via the request-log payload policy (`request_logging.payloads.capture_mode`), which Code Mode shares; summary rows are still written
- nested-call arguments and results are redacted and bounded identically to aggregate `call_tool` payloads (key-based redaction applies normally to their structured JSON)
- sandbox trap and infrastructure detail is reduced to generic error strings before logging; host-side stack traces and guest internals are never persisted

## Relationship to Request Logs

Request logs keep the request-level outcome and tool cardinality. MCP invocation logs keep per-tool audit detail.

`request_id` is the durable correlation key. `request_log_id` is an optional non-owning link when the request-log row is known; it is not required for insertion because request-log summaries are written at final outcome and may be absent or purged independently.

Use request logs first when debugging the model/API request. Use MCP invocation logs when the question is which tool ran, whether access policy allowed it, how long it took, and whether the tool result failed or was truncated.

Policy-denied `tools/call` requests are logged before upstream execution. Allowed calls are logged with `allowed`; upstream failures, timeouts, and invalid requests keep their distinct status values.

## What This Page Does Not Own

- request-log payload policy and stream parsing: [Observability and Request Logs](../observability-and-request-logs.md)
- request lifecycle failure classes: [Request Lifecycle and Failure Modes](../../reference/request-lifecycle-and-failure-modes.md)
- user, team, and API-key ownership policy: [Identity and Access](../../access/identity-and-access.md)

## Validation

Run `mise run docs:check` before handing off documentation changes.
