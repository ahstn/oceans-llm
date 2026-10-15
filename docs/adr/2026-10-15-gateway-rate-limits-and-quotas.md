# ADR: Gateway Rate Limits and Request Quotas

- Date: 2026-10-15
- Status: Accepted

## Current state

- [Rate Limits](../configuration/rate-limits.md)
- [Configuration Reference](../configuration/configuration-reference.md#rate_limits)
- [Request Lifecycle and Failure Modes](../reference/request-lifecycle-and-failure-modes.md#rate_limited)

## Context

The gateway had no traffic controls on the request path. Budgets cap spend, but they do not stop a single key, user or team from sending an unbounded number of cheap requests, MCP tool calls, or token counts. The only limiter was the per-instance in-flight cap on `count_tokens` (8 per key per instance). It does not cap sequential volume, and the cap multiplies by the number of keys and replicas.

The Helm chart supports more than one gateway replica (`autoscaling.minReplicas: 2`). The only shared state the deployment provides is Postgres. Redis appears only inside the local single-sign-on test fixture, never next to the gateway. In-process counters alone would let each replica admit the full limit.

IP allow and deny lists (#450) need the same request-time plumbing: a middleware that sees every inference and MCP request, and a client IP resolved through trusted proxies.

## Decision

### 1. Policies are explicit rows, and every matching policy applies

A rate-limit policy has:

| Field | Values |
|---|---|
| `operation` | `inference` (chat, responses, messages, embeddings, decisions), `count_tokens`, `mcp_tool_call` |
| `scope_kind` | `global`, `api_key`, `user`, `team` |
| `scope_id` | optional; set to target one key, user or team, unset to give each principal of that kind its own counter |
| `model_key` | optional filter for `inference` and `count_tokens` |
| `mcp_server_id` | optional filter for `mcp_tool_call` |
| `metric` | `requests`, or `tokens` (`inference` only) |
| `period` | `minute`, `hour`, `day`, `month` |
| `limit` | positive integer |

A request is checked against every enabled policy that matches it. **The most restrictive wins:** if any matching policy is exhausted, the request is rejected. A specific policy can tighten a default policy for one principal, but it cannot raise it.

A `model_key` filter matches either the requested model key or the model it resolves to, so a policy on a model also covers its aliases.

Policies come from two sources:

- **Config** (`rate_limits.policies` in `gateway.yaml`) holds platform-wide defaults. Config policies cannot name a specific principal. Each is identified by its `name`, held in memory, read-only in the admin UI, and never written to the database. This keeps config-as-code free of database ids and matches how guardrail policy is loaded.
- **Admin** policies are rows in `rate_limit_policies` (migration `V59__rate_limits`). Platform admins manage them on the Rate Limits page. They can target a specific key, user, team or MCP server.

### 2. Sliding windows for rates, calendar windows for quotas

- `minute` and `hour` use a **sliding-window counter**. Each counter keeps one row per fixed window. The estimate is `previous × (1 − elapsed fraction) + current`. This removes the double burst a fixed window allows at its boundary, and it needs only two rows per counter, both updated with ordinary SQL.
- `day` and `month` are **fixed UTC calendar windows**. They reset at 00:00 UTC and on the first of the month. Quotas are understood as "per day" and "per month", so they reset at those boundaries.

We chose a sliding-window counter over a token bucket. A token bucket in SQL needs a read-modify-write of a fractional balance and a refill timestamp under a row lock. The window counter is an integer upsert, and its remaining count and reset time are easy to explain in headers.

### 3. Counters live in the gateway database

`rate_limit_counters (counter_key, window_start, amount, expires_at)` is keyed by counter and window. All replicas consume the same rows.

A request check is one transaction:

1. Sort the charges by counter key, so concurrent transactions lock rows in the same order and cannot deadlock.
2. For each `requests` charge, `INSERT ... ON CONFLICT DO UPDATE SET amount = amount + 1 RETURNING amount`. In Postgres this locks the row until commit, which serializes concurrent checks of the same counter across replicas. libSQL uses an `IMMEDIATE` transaction.
3. Read the previous window's amount and compute the estimate.
4. If any charge is over its limit, roll back. A rejected request consumes nothing on any scope. Otherwise commit.

**Tokens** are not known until a response finishes. A `tokens` policy is checked before the request (it rejects once the window is already exhausted) and charged afterwards with the response's `total_tokens`. A burst of concurrent requests can therefore overshoot a token limit by up to one request each. Token counting only charges successful synchronous requests. Batches are not rate limited.

Counter rows expire one window after their window ends. Each replica deletes up to 500 expired rows at most once a minute.

When there are no matching policies, a request makes no database call. Policies are cached per replica. The cache reloads every `rate_limits.policy_refresh_seconds` (default 10) and right after an admin change on the same replica, so changes reach other replicas within the refresh interval.

### 4. Fail open by default

If the counter store errors, `rate_limits.failure_mode` decides:

- `open` (default): the request is admitted without rate-limit headers, the gateway logs a warning, and the `gateway.rate_limit.decisions` metric counts it with outcome `store_error`.
- `closed`: the request is rejected with `503`.

The rate-limit table is in the same database that authenticates API keys. When that database is down, most requests fail anyway. Failing open stops a counter-only problem, such as lock contention, from becoming an outage.

### 5. One error contract, per protocol

| Surface | Rejection |
|---|---|
| OpenAI-compatible inference | `429`, `{"error": {"type": "rate_limit_error", "code": "rate_limited", ...}}` |
| `/v1/messages`, `count_tokens` | `429`, Anthropic `{"type": "error", "error": {"type": "rate_limit_error", ...}}` |
| `/mcp/{server_key}` `tools/call`, aggregate `call_tool` | `429` with a JSON-RPC error, code `-32029`, and `data.retry_after_seconds` |

Every limited response carries `Retry-After` (seconds). Every response that a policy applied to, admitted or rejected, carries `X-RateLimit-Limit`, `X-RateLimit-Remaining` and `X-RateLimit-Reset` (seconds until the window resets) for the tightest policy: the rejecting one, or otherwise the one with the least remaining. The message names the operation, metric, period and scope kind, but no ids.

The rate-limit check runs after model resolution and before the request log, guardrails, budget and provider call. A rejected inference request writes no request-log row and calls no provider. The gateway does not retry or fall back across providers, so one request is counted once.

Rejected MCP calls are logged in the MCP invocation log with status `policy_denied`, policy result `denied` and error code `rate_limited`, plus the policy, period and retry delay in `metadata`. Grant and guardrail denials are already told apart by error code, so this needs no new status value and no table rebuild.

### 6. Shared request-policy middleware

`http::request_policy` runs on the inference and MCP routes. It:

- resolves the client IP (`http::client_ip`). It uses the socket peer address, and reads `X-Forwarded-For` only when the peer is in `server.trusted_proxies` (CIDR list). It walks the header from the right and skips trusted hops;
- puts a `RequestPolicyContext { client_ip, rate_limit }` into the request extensions;
- writes the rate-limit headers onto the response once the handler returns, whether it succeeded or failed, and before any streaming body is sent.

Handlers run the rate-limit check because only they know the principal and the model. #450 can add its global IP check to this middleware and its per-server check next to the MCP rate-limit check, using `RequestPolicyContext::client_ip`.

### 7. Admin API

- `GET /api/v1/admin/rate-limits`
- `POST /api/v1/admin/rate-limits`
- `PATCH /api/v1/admin/rate-limits/{policy_id}`
- `DELETE /api/v1/admin/rate-limits/{policy_id}`

Every endpoint requires a platform admin. Admin page id: `rate_limits`. Each mutation calls `record_rate_limit_policy_change`, which emits a structured `admin_audit` tracing event with the actor, action, and the policy before and after. This is the hook for the admin audit log (#99).

## Consequences

Benefits:

- limits hold across any number of replicas without new infrastructure;
- deployments without policies pay nothing on the request path;
- the error contracts match what the OpenAI, Anthropic and MCP clients already handle, and 429 now always means "slow down";
- #450 gets client-IP resolution and a request-time hook without another middleware.

Trade-offs:

- every request that matches a policy runs one short write transaction, and a `global` policy serializes all matching requests on one row for that transaction. Expect a few milliseconds per request, and more under heavy contention;
- token limits can overshoot by one request per concurrent request;
- policy changes take up to `policy_refresh_seconds` to reach other replicas;
- fail-open means a counter-store outage removes limits until it recovers;
- the sliding estimate assumes requests in the previous window were evenly spread, so it can be off by a few requests near a window boundary.

## Follow-up work

- Write policy changes to the admin audit log once #99 lands.
- Consider an `UNLOGGED` counter table on Postgres if write volume becomes a concern.
- Allow a specific policy to raise a default for one principal, if admins need that.
- Rate limit batch creation and `/v1/models`, if needed.
- Replace the per-instance `count_tokens` in-flight cap once operators have set `count_tokens` policies. For now it remains as a cheap first guard.

## Attribution

This ADR was prepared through collaborative human + AI implementation and design work.
