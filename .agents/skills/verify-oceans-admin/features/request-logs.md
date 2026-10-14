# Request logs

Request logs lets an authorized user review seeded gateway requests, filter them by service and tags, inspect stored payloads and provider attempts, and follow related MCP invocation data.

## Sub-features

- `logs-list` shows the seeded request list in desktop or mobile form.
- `logs-filter` adds removable filter chips for model, provider, service, component, environment, tag pair, and request ID. Each chip writes its URL search param (`model_key`, `provider_key`, `service`, `component`, `env`, `tag_key` and `tag_value`, `request_id`), so the same filters deep-link (for example `?request_id=demo-req-016`).
- `logs-search` is the debounced `Search request logs by model or user` box (placeholder `Search by model or user…`). It writes `q` to the URL with a history replace, so Back skips intermediate keystrokes.
- `logs-detail` opens the `Request Log Detail` sheet: summary rows (including `Operation`: Chat Completions, Responses, Embeddings, or Decisions), `MCP & Tools`, `Request Tags`, `MCP Token Overhead` when present, `Provider Attempts`, and `Payloads`.
- `logs-mcp-link` follows a request to related MCP invocations when permitted.
- `logs-budget-rejection` shows a request that a hard budget rejected before any provider call. Its row has the status the caller received (the gateway's `budget_exceeded` status), `Error Code` `budget_exceeded`, and `No provider attempts recorded`.

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
- **Filter.** Choose `Filters`, then choose `Service` in the `Filter by` menu. A `Service` chip appears with its input focused. Fill `Service filter value` with `developer-productivity` and press Enter. The URL search gains `service=developer-productivity`, the `Filters` button shows a count badge of `1`, and the toolbar reads `Showing 10 of 10`. `Component` and `Environment` work the same way.
- **Tag filter.** Choose `Add filter`, then `Tag`. Fill `Tag key` only. The `Incomplete tag filter` alert shows and the URL does not change. Fill `Tag value` and press Enter to commit the pair as `tag_key` and `tag_value`.
- **Clear.** Choose `Clear`. The chips are removed, the URL returns to its unfiltered query, and `Showing N of M` returns to the unfiltered total.
- **Inspect.** Choose `Inspect` for `demo-req-016`. The `Request Log Detail` sheet opens. `MCP Token Overhead` shows definition tokens, estimator confidence, cache counts, and context share. `Payloads` has the toggle group `Payload view` (`Request` / `Response` / `Split`, default `Split`) and shows the `Request Payload` and `Response Payload` cards, each with a `full` or `truncated` badge, or `No payload stored`.
- **Budget rejection.** As `admin@local`, create a temporary key owned by `gina@research.local` (seeded at 104% of a hard $200 monthly budget) with access to one model, following the API Keys recipe. Send one non-streaming `/v1/chat/completions` request with a unique `x-request-id`. Expect the `budget_exceeded` status and `"type": "budget_error"` in the body, with no provider call and no spend. Repeat with `/v1/responses`, `/v1/messages`, `/v1/embeddings` (an embeddings model), or `/v1/decisions` (`jev`) when the change touches that route. Add a `Request ID` filter chip for each request ID. Each row shows the same status. Inspect it: `Status` matches, `Error Code` reads `budget_exceeded`, and `Provider Attempts` reads `No provider attempts recorded`. Through the API, `items[0].status_code` and `items[0].error_code` match and `attempts` is `[]`. Revoke the key afterwards.
- **Related MCP data.** Choose `View MCP Invocations` only when the signed-in session has access. The destination query keeps the request ID.
- **Proof.** Capture the list before filtering, the applied filter and result count, and one request detail. Record the request ID and compare its visible fields with `GET /api/v1/admin/observability/request-logs?request_id=…` (`items[0].request_log_id`) and `GET /api/v1/admin/observability/request-logs/{request_log_id}` (`log.*`, `attempts[]`, `mcp_token_overhead`).

## Gotchas

- The mobile list replaces `request-log-desktop-table` below the `lg` breakpoint.
- `gateway.yaml` capture limits apply only to live requests; seeded rows carry fixture payloads. A truncation badge is a valid stored result, not missing UI data.
- Non-platform-admin sessions see only their own logs. Verify parity as `admin@local`.
- Decisions logs store the response in the caller's format. A row from `/v1/decisions` with an OpenAI-format body has an ordered `answers` list, and a row from `/v1/systemone` has an `answers` map. Prompt and completion counts match the upstream usage in both cases.
- The detail does not show guardrail decisions. Use [Guardrails](./guardrails.md) or `/api/v1/admin/guardrails/decisions`.
- There is no apply button. A chip value commits on Enter or when its input loses focus, and only if it changed the filters. Wait for `Showing N of M` to change before you compare it with the API.
- `Clear` resets only the chips. A search term in `q` stays until you empty the search box.
- Tag filtering requires both key and value. The UI keeps a partial pair out of the URL, but the API still returns 400 for `tag_key` without `tag_value` (or the reverse).
- `Add filter` and `Clear` appear only once a chip exists. A field that already has a chip is disabled in the `Filter by` menu. Remove one chip with its `Remove <Field> filter` button.
- Request rows use virtualization on desktop. Locate visible content or scroll the table viewport before selecting an off-screen row.
- A budget rejection writes a row under the same policy as any request: `request_logging.payloads.capture_mode` must not be `disabled`, and a user-owned key's owner must have `request_logging_enabled: true` (Gina does). Service-account keys always log. Batch creation (`POST /v1/batches`) enforces the same budget but writes no request-log row.
- `demo-req-016` has a request-ID deep link to MCP Invocations but no seeded invocation row. Prove the retained query, not a non-empty destination result.
