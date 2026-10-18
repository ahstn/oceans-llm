# Rate Limits

Rate Limits is a platform-admin page that lists every rate-limit policy, from `gateway.yaml` (read-only, `Config` badge) and from the database, and creates, edits, pauses, and deletes database policies. A policy caps inference requests, inference tokens, token counts, or MCP tool calls per key, user, team, or the whole gateway, over a sliding minute or hour or a UTC day or month. The gateway enforces policies on the request path: an exhausted policy returns `429` with `Retry-After`, and every response a policy matched carries `X-RateLimit-Limit`, `X-RateLimit-Remaining`, and `X-RateLimit-Reset`.

## Sub-features

- `rate-limits-list` shows `Policies` with name, limit (`N requests / minute`), traffic, `Counted per`, status badges (`Enforced`/`Paused`, `Config`), and `Edit`/`Delete` for database policies only.
- `rate-limits-create` opens `New rate limit`, saves a policy for one API key, and shows it in the list and in `GET /api/v1/admin/rate-limits`.
- `rate-limits-enforce` admits the first inference request with `X-RateLimit-Remaining: 0` and rejects the next with `429`, `error.code: rate_limited`, and `Retry-After`, without a provider call or request-log row.
- `rate-limits-edit` changes the limit and pauses the policy; a paused policy no longer limits or adds headers.
- `rate-limits-delete` removes the policy after `Delete rate limit`.
- `rate-limits-mcp` (optional) rejects a second MCP `tools/call` with HTTP `429` and JSON-RPC `-32029`, and records an MCP invocation with error code `rate_limited`.

## How to get to it (user POV)

- Sign in as a platform administrator and choose `Rate Limits` under `Control Plane`.
- Open `/admin/rate-limits` directly. Users and team admins without the `rate_limits` page are sent to `/admin/profile`, and `GET /api/v1/admin/rate-limits` returns `403`. Only a session with no permitted pages lands on `/admin/no-access`.

## Driving it with control-oceans-admin

Preconditions:

- `control-oceans-admin doctor` passes, and you are signed in as the seeded `admin@local`.
- The default `gateway.yaml` defines no `rate_limits.policies`, so the page starts empty. A reused `gateway.db` may hold policies from earlier runs; delete any `verify-*` policy first.
- `rate-limits-enforce` sends one short paid request. Follow [Live LLM requests](./live-llm-requests.md): `OPENROUTER_API_KEY` must be in the launch environment, and you create a temporary key limited to `deepseek-v4.1-flash` through [API keys](./api-keys.md). Without the credential, record `rate-limits-enforce` as `blocked`. The rejection itself never reaches the provider.

- **Open.** Choose `Rate Limits`. The `Rate Limits` heading and the `Policies` card are visible, with `No rate limits` on a fresh seed.
- **Create.** Choose `New rate limit`. Fill `Name` with a unique `verify-<run id>`, leave `Traffic` as `Inference requests`, choose `API key` in `Counted per`, choose the temporary key in `Applies to`, set `Limit` to `1`, keep `Per minute (sliding)`, and choose `Create rate limit`. The row shows `1 requests / minute`, `Inference requests`, `API key: <key name>`, and `Enforced`.
- **API parity.** With the same session, `GET /api/v1/admin/rate-limits` returns the policy with `source: admin`, `scope_kind: api_key`, the key id in `scope_id`, and `limit: 1`.
- **Enforce.** Send one non-streaming `/v1/chat/completions` request with the temporary key, `model: deepseek-v4.1-flash`, a short synthetic prompt, and `max_tokens` at most 32. Expect `200` with `x-ratelimit-limit: 1` and `x-ratelimit-remaining: 0`. Send the same request again within the minute. Expect `429`, `error.type: rate_limit_error`, `error.code: rate_limited`, a message starting `rate limited: API key rate limit exceeded: 1 inference requests per minute`, and a `retry-after` header. `Request Logs` shows only the first request.
- **Edit.** Choose `Edit` on the row, turn off `Enforced`, and choose `Save rate limit`. The row shows `Paused`. A third request (paid) is optional: it returns `200` with no `x-ratelimit-*` headers.
- **Delete.** Choose `Delete`, then `Delete rate limit`. The row disappears and `GET /api/v1/admin/rate-limits` no longer returns it.
- **MCP (optional).** With a registered MCP server and a granted key from the [MCP](./mcp.md) recipe, create a policy with `Traffic` `MCP tool calls`, that server, `Counted per` `API key`, and `Limit` `1`. Call one read-only tool twice on `/mcp/<server_key>`. The second call returns HTTP `429` with JSON-RPC error code `-32029`, and `MCP Invocations` shows it with error code `rate_limited`.
- **Cleanup.** Delete every `verify-*` policy and revoke the temporary key.
- **Proof.** Capture the empty page, the created row, the enforcement headers and status codes (no prompt, response, or key), and the list after deletion. Record with `control-oceans-admin record rate-limits`, one `--check` per sub-feature.

## Gotchas

- The sidebar link needs both the `rate_limits` page and the platform-admin role. `gateway.yaml` lists `rate_limits` under `permissions.platform_admins.pages`; a config without it hides the page even from platform admins.
- Limits slide. After a rejection, the next request fits once the estimate drops, which can be sooner than a full minute; read `retry-after` instead of waiting 60 seconds.
- A policy change applies at once on the gateway that served the admin request. Other replicas pick it up within `rate_limits.policy_refresh_seconds` (default 10). The local stack has one gateway.
- `Applies to` lists only unrevoked keys. A key created after the page loaded appears after a reload.
- Inference rejections write no request-log row by design. Do not report the missing second row as a logging fault.
