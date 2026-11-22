# ADR Request-path database load and exact charges

- Date: 2026-11-22
- Status: Accepted

## Context

A load test sent 200 requests at a time to a self-hosted gateway, with one API key, through cheap OpenRouter chat and Decisions models. The gateway served about 83 requests a second. Every model, at every vendor, slowed by about 1.6 seconds. With only API key checks (`GET /v1/models`), the gateway stopped at 303 requests a second, with 6.7 CPU cores busy.

We found five causes:

1. Each request checked its API key with Argon2id (19 MiB, 2 passes, about 22 ms of CPU) on a Tokio worker thread.
2. Each request ran `UPDATE api_keys SET last_used_at = …` on the same row. Up to 8 of the 10 pool connections waited on `Lock:tuple` and `Lock:transactionid`.
3. Each priced request read the active budget and added up the whole budget window twice: before and after the provider call. That cost grows with ledger size.
4. Agent session reports ran a `COUNT` and a `SELECT` for each request in a session, up to 1,000 requests. After each new request, the worker rebuilt the whole session.
5. The ledger stored each charge in $0.0001 units. A $0.0000028 OpenRouter charge was stored as $0, so budgets and reports left it out.

## Decision

1. **Remember successful key checks** (`crates/gateway-service/src/api_key_verification.rs`). Stored hashes stay Argon2id. After a key passes Argon2, the process keeps the pair (stored hash, SHA-256 of the secret), up to 4,096 pairs. A later request with the same stored hash compares the SHA-256 digest in constant time. The key row is still read on each request, so revocation, status and owner checks apply at once. A cold check runs off the async executor, at most 4 at a time (`credential_verification.rs`, shared with legacy OAuth client secrets).
2. **Write `last_used_at` at most once a minute**. The authenticator skips the write when the row it read is less than 60 seconds old. The `UPDATE` also carries that condition, so requests that race on one key don't all write.
3. **Keep budget reads in memory** (`crates/gateway-service/src/budget_cache.rs`). Each process caches active budgets by scope key and window spend by scope, cadence and window start. A value answers for 5 seconds from the start of the database read that produced it. When an entry expires, one task reloads it and the others wait for that load, so the cache never answers with an expired value. A waiting task also takes the result of a read that started after it arrived, so a read slower than 5 seconds does not make each waiter read again. A window reload sums the ledger through the current time, capped at the window's end. Each entry has a gate: a reload holds it exclusively from its ledger read until it stores the total, and recording a charge holds it shared from before the ledger insert until the charge is added. A charge is therefore in the loaded total or added to it, exactly once. If the insert fails or is cancelled, the row may still have been committed, so those windows drop their cached totals. A map past 16,384 entries drops expired entries, then other entries no task is using, down to three quarters of that. It never drops an entry in use, so every task that uses a window shares its one gate, and the map exceeds the limit only by the entries of requests in flight. An admin budget save or removal clears the cached budgets in that process.
4. **Load MCP evidence in one query** (`list_mcp_tool_invocations_by_request_ids`). The query takes all request IDs in a session, matches the owner exactly (NULL matches NULL only), orders by request ID in byte order, then newest first, and stops at 2,048 rows.
5. **Store charges in nano-dollars** (`Money9`, V84). `usage_cost_events.computed_cost_10000` becomes `computed_cost_nanos`, and existing values are multiplied by 100,000. Provider-reported costs and token prices round once, to $0.000000001. Spend sums add nano-dollars and round to $0.0001 once, where an API field still uses `_10000`. Request logs add an exact `cost_usd_nanos`. Price rates and budget amounts stay `Money4`. A batch whose ledger row was written before V84 can be finalized again after it. The worker rebuilds the old value from the same results (each reported cost, or the batch-level reported cost, and the local total, each rounded half-up to $0.0001), and the retry accepts a row with that value.

## Why

- API keys that Oceans generates hold 244 random bits, so a slow hash adds no safety for them. But configured keys can be short strings, so a fast hash at rest could expose them if the database leaks. Remembering verified pairs gives fast checks and keeps Argon2 at rest. It needs no new secret, no data migration and no new dependency.
- A conditional `UPDATE` with a time window is the usual fix for this hot row. GitLab throttles `last_downloaded_at` the same way, and OpenHands debounces `last_used_at`.
- A per-request counter row for budgets would move the hot row, not remove it. LiteLLM found that per-entity spend updates limit throughput at about 500 requests a second. An in-process total, reloaded from the ledger, removes the request-path sums and keeps the ledger as the source of truth.
- Usage billing systems store small AI charges as integers in micro-dollar or nano-dollar units, and round only at the edge. Nano-dollars keep OpenRouter's reported cost exact for today's cheapest models. An `i64` still holds $9.2 billion.

## Trade-offs

- Spend recorded by another replica, a ledger row deleted outside the gateway, and a budget changed by another replica or by config seeding can take up to 5 seconds to count. Enforcement already allowed some overrun, because it doesn't reserve spend for requests in flight.
- When an entry expires, requests for that scope wait for one database read. A ledger insert for the scope waits while that read runs, and the read waits for inserts already running.
- The memo keeps SHA-256 digests of valid secrets in process memory. The process already handles the plain secrets.
- V84 rewrites every ledger row and needs reader version 84. A gateway older than this change can't start on a migrated database.
- Agent session analysis still rounds each request's cost to $0.0001 in its diagnostic metrics.

## Follow-ups

- A shared counter, for example Redis, if many replicas must enforce one budget more tightly than 5 seconds allows.
- Batch the other per-session reads in agent analysis (observation sets and session lists), and coalesce rebuild requests for the same session.
