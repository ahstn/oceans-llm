# Request logs

Request logs lets an authorized user review seeded gateway requests, filter them by service and tags, inspect stored payloads and provider attempts, and follow related MCP invocation data.

## Sub-features

- `logs-list` shows the seeded request list in desktop or mobile form.
- `logs-filter` filters by service, component, environment, and tag pair, plus the URL-only `request_id`, `model_key`, and `provider_key` (for example `?request_id=demo-req-016`).
- `logs-detail` opens the `Request Log Detail` sheet: summary rows (including `Operation`: Chat Completions, Responses, Embeddings, or Decisions), `MCP & Tools`, `Request Tags`, `MCP Token Overhead` when present, `Provider Attempts`, and `Payloads`.
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
- **Filter.** Fill `Service`, `Component`, or `Environment`, then choose `Apply Filters`. For tags, fill both `Tag key` and `Tag value`. With only one filled, the `Incomplete tag filter` alert shows and `Apply Filters` is disabled. The API rejects a partial pair as an invalid request.
- **Clear.** Choose `Clear`. The filter fields are empty and the route returns to its unfiltered query.
- **Inspect.** Choose `Inspect` for `demo-req-016`. The `Request Log Detail` sheet opens. `MCP Token Overhead` shows definition tokens, estimator confidence, cache counts, and context share. `Payloads` has the toggle group `Payload view` (`Request` / `Response` / `Split`, default `Split`) and shows the `Request Payload` and `Response Payload` cards, each with a `full` or `truncated` badge, or `No payload stored`.
- **Related MCP data.** Choose `View MCP Invocations` only when the signed-in session has access. The destination query keeps the request ID.
- **Proof.** Capture the list before filtering, the applied filter and result count, and one request detail. Record the request ID and compare its visible fields with `GET /api/v1/admin/observability/request-logs?request_id=…` (`items[0].request_log_id`) and `GET /api/v1/admin/observability/request-logs/{request_log_id}` (`log.*`, `attempts[]`, `mcp_token_overhead`).

## Gotchas

- The mobile list replaces `request-log-desktop-table` below the `lg` breakpoint.
- `gateway.yaml` capture limits apply only to live requests; seeded rows carry fixture payloads. A truncation badge is a valid stored result, not missing UI data.
- Non-platform-admin sessions see only their own logs. Verify parity as `admin@local`.
- The detail does not show guardrail decisions. Use [Guardrails](./guardrails.md) or `/api/v1/admin/guardrails/decisions`.
- Tag filtering requires both key and value. The Apply button is disabled for a partial pair.
- Request rows use virtualization on desktop. Locate visible content or scroll the table viewport before selecting an off-screen row.
- `demo-req-016` has a request-ID deep link to MCP Invocations but no seeded invocation row. Prove the retained query, not a non-empty destination result.
