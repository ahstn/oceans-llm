# ADR: Budget Exhaustion Returns 422 Instead of 429

- Date: 2026-10-14
- Status: Accepted
- Supersedes: the `429 budget_exceeded` contract in [ADR: Chat Observability Hardening and Stream Parse Contract](2026-03-31-chat-observability-hardening-and-stream-parse-contract.md)

## Current state

- [Budgets](../access/budgets.md#hard-and-soft-limits)
- [Request Lifecycle and Failure Modes](../reference/request-lifecycle-and-failure-modes.md#budget_exceeded)
- [Configuration Reference](../configuration/configuration-reference.md#budgets)

## Context

When a hard budget rejected a request, the gateway returned HTTP 429 with `type: budget_error` and `code: budget_exceeded`. Clients treat 429 as "slow down and try again". The OpenAI and Anthropic Python SDKs retry 429 with backoff by default (see [Verification](#verification)), and agent tools that use these SDKs with default retry settings inherit that behavior. A budget window does not reset for hours or days, so these retries cannot succeed. They add latency to a failure that is already final. In tools that retry for a long time, they also hide the real cause behind a timeout or a generic "rate limited" message.

The budget check runs before any provider call, so the retries cost no provider spend. They still add gateway load, and they leave users unsure why their tool stalled.

## Decision

### 1. Default to 422

Hard-budget rejections now return **422 Unprocessable Content** by default. The body keeps `type: budget_error` and `code: budget_exceeded`, so callers can tell a budget rejection from a request-validation 422 (`type: invalid_request_error`) by `type` or `code` alone.

We chose 422 over 402 Payment Required:

- The OpenAI and Anthropic Python SDKs treat 422 as a final client error. Both raise `UnprocessableEntityError` and do not retry. Other clients may differ.
- 402 is still "reserved for future use" in RFC 9110, so its meaning varies from client to client. Some proxies and client libraries special-case it, and several surface it as an unknown status.
- 402 suggests the caller can fix the problem by paying, which is not true for a budget an administrator sets.

402 remains available for admins who prefer it.

### 2. Admins can choose the status

`budgets.exceeded_status_code` accepts `402`, `422` or `429`, and defaults to `422`. Setting it to `429` restores the old behavior for clients that were built to back off on budget errors. Other values fail config validation at startup. `BudgetExceededStatus` in `gateway-core` is the typed allowlist, so an out-of-range status cannot reach the HTTP layer.

### 3. The status travels with the error

`BudgetGuard` holds the configured status and copies it into each `GatewayError::BudgetExceeded`. `http_status_code()` reads it from there, so there is no global state:

- the OpenAI envelope (`AppError`) and the Anthropic envelope (`anthropic_error_response`) both use `http_status_code()` and need no route-specific mapping;
- the chat-request metric (`http.response.status_code`, outcome `budget_error`) records the same status the client received. Pre-provider rejections do not write a request-log row;
- tests can run gateways with different statuses in parallel.

`BudgetGuard::enforce_pre_provider_budget` is the only hard-budget gate. That covers every enforcing surface: `/v1/chat/completions`, `/v1/responses`, `/v1/messages`, `/v1/embeddings`, `/v1/decisions`, batch creation, and limits raised by temporary budget increases. MCP tool calls do not enforce budgets.

### 4. The message says what happened without leaking data

The message names the cadence, the scope kind (`user`, `service account` or `user+model`), the spend and the limit, and says that retrying will not succeed until the window resets or an administrator raises the limit. For example:

```text
daily service account budget exhausted: spent $12.5000 of the $12.0000 limit. Retrying will not succeed until the budget window resets or an administrator raises the limit
```

It does not include budget ids, ownership keys or other principals' identifiers. The previous message embedded the internal ownership-scope key. That key still travels on `GatewayError::BudgetExceeded` for server-side use, but it is no longer in the response. The caller already owns the budget that rejected it, so its spend and limit are not someone else's data.

## Verification

- Router-level tests send an over-budget request to every enforcing route, check the status and envelope, and run once each for 422, 429 and 402 (`crates/gateway/src/http/budget_status_tests.rs`).
- Config tests accept 402, 422 and 429, and reject other numbers and non-numeric values.
- The OpenAI Python SDK (`openai` 3.31.0) and the Anthropic Python SDK (`anthropic` 1.14.0), both with the default `max_retries=2`, were pointed at a stub that returned the gateway's budget body. Each made 1 HTTP attempt for 422 and for 402, and 3 attempts for 429. Other SDKs, languages and retry middleware were not tested.

## Consequences

Benefits:

- the tested SDKs, and agent tools that use them with default retry settings, fail fast with a clear message instead of retrying a request that cannot succeed;
- 429 again means only "rate limited" for callers of this gateway;
- admins whose clients depend on the old behavior can opt out with a single setting.

Trade-offs:

- This is a breaking change for clients that match on status 429 to detect budget exhaustion. They need to check `error.code == "budget_exceeded"` or set `budgets.exceeded_status_code: 429`.
- Clients that parsed the old message text need to read `code` instead.
- Clients that check status only now see a generic 422. They must read `type` or `code` to tell a budget rejection from a validation error.
- The setting is global. Per-team or per-key statuses are not supported.

## Follow-up work

- Consider sending `x-should-retry: false` on budget rejections. The OpenAI SDKs honor it, which would keep them from retrying even when an admin chooses 429.
- Consider a `Retry-After` header that points to the start of the next budget window when an admin chooses 429.

## Attribution

This ADR was prepared through collaborative human + AI implementation and design work.
