# Request logs

Request logs lets an authorized user review seeded gateway requests, filter them by service and tags, inspect stored payloads and provider attempts, and follow related MCP invocation data.

## Sub-features

- `logs-list` shows the seeded request list in desktop or mobile form.
- `logs-filter` adds removable filter chips for model, provider, status, service, component, environment, tag pair, cost, and request ID. Each chip writes its URL search param (`model_key`, `provider_key`, `status_code`, `service`, `component`, `env`, `tag_key` and `tag_value`, `min_cost_usd`, `request_id`), so the same filters deep-link (for example `?request_id=demo-req-016` or `?status_code=429`). `Model` and `Status` pick from a list instead of a text box: `Model` lists the gateway model catalog (aliases included) from one `GET /api/v1/admin/models/options` request, and `Status` lists common HTTP codes with their reason phrase.
- `logs-cost-filter` is the `Cost` chip, which reads `Cost | above | $ | <amount>`. It keeps requests whose priced cost is strictly above the amount (`?min_cost_usd=2` for requests costing more than $2). Unpriced requests never match. The amount takes up to four decimals; anything else shows the `Invalid cost filter` alert and stays out of the URL.
- `logs-page` is the pager under the table: a `first–last of total` range, `Go to previous page`, numbered `Go to page N` links with an ellipsis for each hidden stretch, `Go to next page`, and `Rows per page` (50, 100 or 200). It writes `page` and `page_size` to the URL, leaving out page 1 and the default 100. Any filter or search change returns to page 1.
- `logs-search` is the debounced `Search request logs by model or user` box (placeholder `Search by model or user…`). It writes `q` to the URL with a history replace, so Back skips intermediate keystrokes.
- `logs-detail` opens the request detail sheet (`request-log-detail-sheet`). Its header shows the status badge, operation, timestamp, the request ID as the sheet title with a `Copy request ID` button, and the model and provider (`request-log-model-route`). The body shows the `Latency`, `Tokens`, `Cost` and `Cache hit` tiles (`request-log-key-metrics`), a two-column fact list (`request-log-facts`: `Caller`, `API key`, `Agent harness`, `User-agent`, `Request ID`, `Request log ID`, `Operation` (Anthropic Messages, Chat Completions, Responses, Embeddings, or Decisions), `Stream`, `Service tier`, `Occurred at`, `Error code`, `Tags`), then the `MCP & tools` (`request-log-tools`), `Provider attempts` (`request-log-attempts`) and `Payloads` sections. The MCP token overhead estimate (`request-log-mcp-overhead`) sits inside `MCP & tools` when present. IDs truncate on one line, with the full value in the `title` attribute. `Service tier` is hidden when the log has neither a requested nor a served tier; [Service tiers](./service-tiers.md) covers its badges.
- `logs-mcp-link` follows a request to related MCP invocations when permitted.
- `logs-budget-rejection` shows a request that a hard budget rejected before any provider call. Its row has the status the caller received (the gateway's `budget_exceeded` status), `Error code` `budget_exceeded`, and `No provider attempts recorded`.

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
- **Model and status filters.** Choose `Filters`, then `Model`. The chip's picker opens with `Search models…` and the catalog loads on first use. Choose a seeded model such as `gpt-oss-120b`; the URL gains `model_key` at once, with no Enter needed. Add `Status`, choose `200` (`OK`), and the URL gains `status_code=200` as a number. Typing a three-digit code that is not listed, such as `529`, offers `Filter by “529”`; other text offers nothing.
- **Tag filter.** Choose `Add filter`, then `Tag`. Fill `Tag key` only. The `Incomplete tag filter` alert shows and the URL does not change. Fill `Tag value` and press Enter to commit the pair as `tag_key` and `tag_value`.
- **Cost filter.** Choose `Filters`, then `Cost`. Fill `Cost above value` with `abc` and press Enter: the `Invalid cost filter` alert shows and the URL does not change. Fill `0.4` and press Enter. The URL gains `min_cost_usd` (the router quotes number-like text, so it reads `min_cost_usd=%220.4%22`; a hand-typed `?min_cost_usd=0.4` works too), and every row's `Cost` is above `$0.4000`. Compare `total` with `GET /api/v1/admin/observability/request-logs?min_cost_usd=0.4`.
- **Pagination.** With no filters, the pager reads `1–100 of <total>`. Choose `Go to next page`: the URL gains `page=2`, the range reads `101–200 of <total>`, and the table opens at its first row. Choose `Rows per page`, then `50`: the URL gains `page_size=50` and returns to page 1. Add any filter chip from page 2 and the URL drops `page`.
- **Clear.** Choose `Clear`. The chips are removed, the URL returns to its unfiltered query, and `Showing N of M` returns to the unfiltered total.
- **Inspect.** Add a `Request ID` chip for `demo-req-016`, or open `?request_id=demo-req-016`, then choose `Inspect`. The detail sheet opens with the heading `demo-req-016`, a `200` status badge, and `gpt-oss-120b` via `bedrock-us-east-1`. The key metric tiles read `978 ms`, `1,960` (`1,340 in · 620 out`), `$0.4210` and `31%` (`415 of 1,340 prompt tokens`). `MCP & tools` shows the `Offered` group (`MCP servers` 4, `Request tools` 31, `Tools exposed` 27, `Tools filtered` 5) and the `Called` group (`Tool calls` 6, `MCP tool calls` 4, `Distinct tools called` 4), then the MCP token overhead: `3,105` definition tokens, `2.36%` of a `131,072`-token context window, cache hits and misses, confidence and estimator. `Payloads` has the toggle group `Payload view` (`Request` / `Response` / `Split`, default `Split`) and shows the `Request payload` and `Response payload` code blocks (`request-log-payloads`), each with a `full` or `truncated` badge and a copy button, or `No payload stored`.
- **Messages parity.** For the single canary in [Anthropic Messages](./anthropic-messages.md), filter by its response request ID. Require the expected stream flag, model/provider, status and usage, plus the matching successful upstream attempt. Require the `Operation` fact to read `Messages` and `log.metadata.operation` to be `messages`. Mask `request-log-payloads` in screenshots and omit it from ARIA/text artifacts.
- **Budget rejection.** As `admin@local`, create a temporary key owned by `gina@research.local` (seeded at 104% of a hard $200 monthly budget) with access to one model, following the API Keys recipe. Send one non-streaming `/v1/chat/completions` request with a unique `x-request-id`. Expect the `budget_exceeded` status and `"type": "budget_error"` in the body, with no provider call and no spend. Repeat with `/v1/responses`, `/v1/messages`, `/v1/embeddings` (an embeddings model), or `/v1/decisions` (`jev`) when the change touches that route. Add a `Request ID` filter chip for each request ID. Each row shows the same status. Inspect it: the header status badge matches, `Error code` reads `budget_exceeded`, the `Tokens` and `Cost` tiles read `—`, and `Provider attempts` reads `No provider attempts recorded`. Through the API, `items[0].status_code` and `items[0].error_code` match and `attempts` is `[]`. Revoke the key afterwards.
- **Related MCP data.** Choose `View MCP invocations` (in the `MCP & tools` section header) only when the signed-in session has access. The destination query keeps the request ID.
- **Proof.** Capture the list before filtering, the applied filter and result count, and one request detail. Record the request ID and compare its visible fields with `GET /api/v1/admin/observability/request-logs?request_id=…` (`items[0].request_log_id`) and `GET /api/v1/admin/observability/request-logs/{request_log_id}` (`log.*`, `attempts[]`, `mcp_token_overhead`).

## Gotchas

- The mobile list replaces `request-log-desktop-table` below the `lg` breakpoint.
- The detail sheet is one scrolling column (`request-log-detail-body`). Split payloads sit side by side from `xl`; below that they stack. Each payload block shows up to 28 lines and scrolls inside itself, and long lines wrap rather than scroll sideways.
- `gateway.yaml` capture limits apply only to live requests; seeded rows carry fixture payloads. A truncation badge is a valid stored result, not missing UI data.
- Non-platform-admin sessions see only their own logs. Verify parity as `admin@local`.
- Decisions logs store the response in the caller's format. A row from `/v1/decisions` with an OpenAI-format body has an ordered `answers` list, and a row from `/v1/systemone` has an `answers` map. Prompt and completion counts match the upstream usage in both cases.
- The detail does not show guardrail decisions. Use [Guardrails](./guardrails.md) or `/api/v1/admin/guardrails/decisions`.
- There is no apply button. A text chip commits on Enter or when its input loses focus, and only if it changed the filters. A `Model` or `Status` choice commits as soon as it is picked. Wait for `Showing N of M` to change before you compare it with the API.
- `Clear` resets only the chips. A search term in `q` stays until you empty the search box.
- Tag filtering requires both key and value. The UI keeps a partial pair out of the URL, but the API still returns 400 for `tag_key` without `tag_value` (or the reverse).
- `Add filter` and `Clear` appear only once a chip exists. A field that already has a chip is disabled in the `Filter by` menu. Remove one chip with its `Remove <Field> filter` button.
- `Showing N of M` counts the rows on the current page against the filtered total; the pager's range says which page they are.
- The cost filter matches the usage ledger's priced cost on `(request_id, api_key_id)`, the same pairing as the `Cost` column, so a row showing `—` never matches.
- Request rows use virtualization on desktop. Locate visible content or scroll the table viewport before selecting an off-screen row.
- A budget rejection writes a row under the same policy as any request: `request_logging.payloads.capture_mode` must not be `disabled`, and a user-owned key's owner must have `request_logging_enabled: true` (Gina does). Service-account keys always log. Batch creation (`POST /v1/batches`) enforces the same budget but writes no request-log row.
- `demo-req-016` has a request-ID deep link to MCP Invocations but no seeded invocation row. Prove the retained query, not a non-empty destination result.
