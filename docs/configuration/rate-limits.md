# Rate Limits

Rate limits cap how much traffic a key, user, team, or the whole gateway can send. They apply to inference requests, Anthropic token counts, and MCP tool calls. They are separate from [budgets](../access/budgets.md): budgets cap spend, and rate limits cap volume.

Design and trade-offs: [ADR: Gateway Rate Limits and Request Quotas](../adr/2026-10-15-gateway-rate-limits-and-quotas.md).

## Policies

A policy counts one kind of traffic for one scope over one window.

| Field | Values | Notes |
|---|---|---|
| `operation` | `inference`, `count_tokens`, `mcp_tool_call` | `inference` covers `/v1/chat/completions`, `/v1/responses`, `/v1/messages`, `/v1/embeddings`, `/v1/decisions`, `/v1/systemone`, and their unversioned aliases. `count_tokens` covers `/v1/messages/count_tokens`. `mcp_tool_call` covers `tools/call` on `/mcp/{server_key}` and aggregate `call_tool` on `/mcp`. The aggregate `search_tools` and `describe_tool` read the gateway's own catalog and call no MCP server, so they are not counted. |
| scope | `global`, `api_key`, `user`, `team` | `global` is one counter for everyone. The other kinds give each key, user, or team its own counter. Admin policies can also target one specific key, user, or team. |
| `model` | gateway model key | Optional, for `inference` and `count_tokens`. Matches the requested model or the model it resolves to, so a policy on a model also covers its aliases. |
| `mcp_server` | MCP server key | Optional, for `mcp_tool_call`. |
| `metric` | `requests` (default), `tokens` | `tokens` is `inference` only and counts the provider's `total_tokens`. |
| `period` | `minute`, `hour`, `day`, `month` | See [Windows](#windows). |
| `limit` | positive integer | |

A `user` scope applies to user keys and OAuth callers; a service-account key has no user. A `team` scope applies to a service-account key's team, and to a user who belongs to a team, whether they use their own key or an OAuth client, so every member counts against the team's counter. The gateway reads a user's team when a team policy could apply and keeps it for the policy refresh interval (10 seconds by default). If that read fails, `failure_mode: closed` rejects the request with `503`; `open` checks the other policies.

An `api_key` scope counts the credential a request presents. For an MCP client that signed in with OAuth, that is its grant: each authorized client has its own counter, as each key does. Use a `user` scope to limit a person across all their keys and clients.

### Every matching policy applies

A request is checked against every enabled policy that matches it, and **the most restrictive one wins**. If any matching policy is exhausted, the request is rejected, and nothing is charged against any of the other policies.

A specific policy can tighten a default for one principal, but it cannot raise it. For example, a config default of 600 requests a minute per key and an admin policy of 60 a minute for one key leave that key at 60, and every other key at 600.

### Windows

- `minute` and `hour` are **sliding windows**. The gateway estimates the last 60 seconds or the last hour from the current and previous fixed windows, weighting the previous one by how much of it still overlaps. This avoids the double burst a fixed window allows at its boundary.
- `day` and `month` are **quotas on UTC calendar windows**. They reset at 00:00 UTC and on the first of each month.

### Token limits

Token counts are only known once a response finishes. A `tokens` policy rejects a request once its window is already used up, then charges the response's `total_tokens` after it completes. The charge is written before the response is returned, so requests sent one after another always see earlier usage. Only concurrent requests can each pass the check before the others are charged, so a token limit can be exceeded by up to one response per request in flight.

Token limits count the usage the provider reports, the same usage budgets charge. That includes partial usage from a request that failed, from a stream that ended in an error, and from a stream the client cancelled after usage was reported, because the provider consumed those tokens. A response's usage is charged to all of its token policies or to none. A cancelled stream is charged in the background. The next check of the same token counter on that gateway instance waits up to a second for the charge, so cancelling and retrying straight away still sees the usage. In `closed` mode, a check that is still waiting after that second is rejected with `503`. Requests that report no usage and batch jobs are not charged.

## Configure

Platform-wide defaults go in `gateway.yaml`:

```yaml
rate_limits:
  failure_mode: open          # or closed
  policy_refresh_seconds: 10  # 1 to 3600
  policies:
    - name: inference per key
      operation: inference
      scope: api_key
      period: minute
      limit: 600
    - name: tokens per user
      operation: inference
      scope: user
      metric: tokens
      period: hour
      limit: 2000000
    - name: token counts per key
      operation: count_tokens
      scope: api_key
      period: minute
      limit: 300
    - name: notes calls per key
      operation: mcp_tool_call
      scope: api_key
      mcp_server: notes
      period: day
      limit: 5000
```

Config policies cannot name a specific key, user, or team. Policy names must be unique. Each operation can have at most 100 enabled policies, config and admin combined, which bounds the work a single request can cause. A gateway refuses to start if config and the enabled admin policies together exceed it, or if a config policy has the same name as an admin policy, paused or not. A `model` must be a configured gateway model. A policy whose `mcp_server` is not registered logs a warning at startup and applies once a server with that key exists. See [Configuration Reference](configuration-reference.md#rate_limits) for validation rules.

Platform admins manage more policies on the **Rate Limits** page in the admin UI (page id `rate_limits`), or through `GET`/`POST /api/v1/admin/rate-limits` and `PATCH`/`DELETE /api/v1/admin/rate-limits/{policy_id}`. Admin policies can target one key, user, team, or MCP server, and can be paused without deleting them. Config policies appear on the same page, read-only.

An admin change applies on the replica that handled it straight away, and on every other replica within `policy_refresh_seconds`. Changing a policy's operation, model or MCP server filter, period, or metric starts its counters from zero; changing the limit or name keeps them.

### Recommended defaults

Start with generous per-key limits that only stop runaway clients, then tighten for specific keys or teams:

| Traffic | Suggested starting point |
|---|---|
| Inference requests | 600 per minute per key |
| Inference tokens | a per-user or per-team hourly token limit sized to your provider quota |
| Token counts | 300 per minute per key |
| MCP tool calls | 120 per minute per key, plus a daily quota on servers with costly or rate-limited upstreams |

A `global` policy protects a shared provider quota, but it serializes every matching request on one counter row. Prefer per-key or per-team policies for high-volume traffic.

## Responses

A rejected request returns `429` before route selection, guardrails, budget checks, or any provider or upstream MCP call.

| Surface | Body |
|---|---|
| OpenAI-compatible routes | `{"error": {"type": "rate_limit_error", "code": "rate_limited", "message": "..."}}` |
| `/v1/messages`, `/v1/messages/count_tokens` | `{"type": "error", "error": {"type": "rate_limit_error", "code": "rate_limited", "message": "..."}}` |
| MCP `tools/call` and aggregate `call_tool` | a JSON-RPC error with code `-32029` and `data: {"error_code": "rate_limited", "retry_after_seconds": N}` |

The message names the scope kind, the limit, the window, and the retry delay, for example `rate limited: API key rate limit exceeded: 60 inference requests per minute. Retry after 12 seconds`. The JSON-RPC error message has no `rate limited: ` prefix. It never includes ids.

Headers:

| Header | When | Value |
|---|---|---|
| `Retry-After` | rejected requests | Seconds until the request would fit |
| `X-RateLimit-Limit` | any request a policy matched | The tightest policy's limit |
| `X-RateLimit-Remaining` | any request a policy matched | Requests or tokens left in that policy's window |
| `X-RateLimit-Reset` | any request a policy matched | Seconds until that policy's whole limit is free again. For `day` and `month`, the end of the window. For `minute` and `hour`, usage in the current window keeps counting, at a falling weight, until the end of the next window, so that is the reset. |

The tightest policy is the one that rejected the request, or, when the request was admitted, the one with the smallest share of its limit left. Shares are compared rather than amounts, because request and token limits count different units. Requests that no policy matches carry none of these headers.

The headers describe the state when the request was admitted. For a token policy, `X-RateLimit-Remaining` does not include the tokens of the response that carries it, because streamed responses send headers before usage is known; non-streaming responses behave the same way so the two agree. The next request's headers include them.

### Retries and fallbacks

- The gateway checks a request once, before it picks a route. It does not retry or fall back across providers, so one request is counted once.
- A client retry is a new request and is counted again. The OpenAI and Anthropic Python SDKs retry `429` with backoff by default and read `Retry-After`.
- Budget rejections use `422` by default, so `429` from this gateway normally means "slow down". If `budgets.exceeded_status_code` is set to `429`, a `429` can also be a budget rejection: check `error.code`, which is `rate_limited` for a rate limit and `budget_exceeded` for a budget. See [Budgets](../access/budgets.md#hard-and-soft-limits).
- Each key may still have at most 8 token counts in flight on each gateway instance. That cap is checked before rate limits and also returns `429`.

## Storage and replicas

Counters live in the gateway database (`rate_limit_counters`), so every replica enforces the same limits. Each check is one short transaction that charges every matching counter only if all of them admit the request. In Postgres the charge locks the counter row until the transaction commits, which serializes concurrent checks of the same counter across replicas. No Redis or other store is needed.

When no policy matches a request, the check makes no database call.

Counter rows expire one window after the window ends. Each replica deletes expired rows at most once a minute, in batches of 500 until a batch comes back short (up to 50,000 rows a minute), on any request that is checked while policies are loaded, whether or not a policy matches it.

### When the database fails

`rate_limits.failure_mode` controls what happens if a counter check fails:

- `open` (default): the request is admitted without rate-limit headers, and the gateway logs a warning.
- `closed`: the request is rejected with `503`. MCP `tools/call` and aggregate `call_tool` return HTTP `503` with a JSON-RPC error (code `-32603`). After a token usage write fails, requests that match a token policy are also rejected for 5 seconds, so usage that could not be recorded cannot keep passing a read-only check.

Both modes treat a replica that has never loaded the admin policies, and cannot, as a store failure: it cannot tell which policies a request would match. A replica that loaded them before keeps enforcing that set.

API-key authentication uses the same database, so most requests fail anyway when it is down. Failing open keeps a counter-only problem, such as lock contention, from becoming an outage.

## Observability

- `gateway.rate_limit.decisions` counts checks by `rate_limit.operation` and `rate_limit.outcome` (`admitted`, `rejected`, or `store_error`). Requests that no policy matches are not counted.
- A rejected MCP call appears in [MCP Invocations](../mcp/mcp-invocations.md) with status `policy_denied`, policy result `denied`, and error code `rate_limited`. Its metadata names the policy, scope kind, metric, period, limit, and retry delay.
- A rejected inference request writes no request-log row, so a client hammering the limit cannot flood the request log. The gateway logs a `request rejected by rate limit` event with the operation, policy name, and client IP.

## Client IP and trusted proxies

The gateway records the client IP for each inference and MCP request. By default it is the TCP peer. Behind a load balancer or ingress, list the proxies in `server.trusted_proxies` so the gateway reads `X-Forwarded-For`:

```yaml
server:
  trusted_proxies:
    - 10.0.0.0/8
    - fd00::/8
```

The gateway reads `X-Forwarded-For` only when the peer is a trusted proxy. It walks the header from the right, skips trusted hops, and takes the first untrusted address, so a caller cannot choose its own IP by sending the header. Rate limits do not key on IP today. Policy changes are recorded in the [admin audit log](../access/admin-audit-log.md). [Admin audit entries](../access/admin-audit-log.md#request-ip) use the same resolution, and IP allow and deny lists will too.

## Limits of this release

- Batches, `/v1/models`, and admin APIs are not rate limited.
- A specific policy cannot raise a default for one principal.
