# Request logs

Request logs lets an authorized user review seeded gateway requests, filter them by service and tags, inspect stored payloads and provider attempts, and follow related MCP invocation data.

## Sub-features

- `logs-list` shows the seeded request list in desktop or mobile form.
- `logs-filter` filters by service, component, environment, and tag pair.
- `logs-detail` opens stored request metadata, payloads, tool counts, MCP token-overhead estimates, and provider attempts.
- `logs-mcp-link` follows a request to related MCP invocations when permitted.

## How to get to it (user POV)

- Sign in and choose `Request Logs` under `Observability`.
- Open `/admin/observability/request-logs` directly.
- Choose `Inspect` on a request row or card to open its detail.

## Driving it with control-oceans-admin

Preconditions:

- `control-oceans-admin doctor` passes.
- `mise run dev-stack` completed its local demo seed.
- The signed-in session includes the `request_logs` page permission.

- **Open list.** Choose `Request Logs`. The `Request logs` heading and `Request list` card are visible. At desktop width, `request-log-desktop-table` is visible.
- **Filter.** Fill `Service`, `Component`, or `Environment`, then choose `Apply Filters`. For tags, fill both `Tag key` and `Tag value`; a partial tag pair is invalid.
- **Clear.** Choose `Clear`. The filter fields are empty and the route returns to its unfiltered query.
- **Inspect.** Choose `Inspect` for `demo-req-016`. The detail shows stored request information. `MCP Token Overhead` confirms definition tokens, estimator confidence, cache counts, and context share. `MCP & Tools`, `Provider Attempts`, and `Payload view` expose the related sections.
- **Related MCP data.** Choose `View MCP Invocations` only when the signed-in session has access. The destination query keeps the request ID.
- **Proof.** Capture the list before filtering, the applied filter and result count, and one request detail. Record the request ID and compare its visible fields with the local gateway detail response used by the UI.

## Gotchas

- The mobile list replaces `request-log-desktop-table` below the `lg` breakpoint.
- Payload capture can be redacted or truncated by `gateway.yaml`. A truncation badge is a valid stored result and must not be treated as missing UI data.
- Tag filtering requires both key and value. The Apply button is disabled for a partial pair.
- Request rows use virtualization on desktop. Locate visible content or scroll the table viewport before selecting an off-screen row.
- `demo-req-016` has a request-ID deep link to MCP Invocations but no seeded invocation row. Prove the retained query, not a non-empty destination result.
