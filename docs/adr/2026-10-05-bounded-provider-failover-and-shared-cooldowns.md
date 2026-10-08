# ADR: Bounded Provider Failover and Shared Cooldowns

- Date: 2026-10-05
- Status: Accepted
- Related issue: [#409](https://github.com/ahstn/oceans-llm/issues/409)
- Supersedes: the one-attempt restriction in [Shared Model Routing and Session Affinity](2026-10-04-shared-model-routing-and-session-affinity.md), when failover is enabled

## Context

A routing pool can contain several valid providers for one gateway model. The original routing policy selects one route and keeps sessions there. A temporary provider failure or exhausted Copilot quota can therefore stop a request even when another eligible route can serve it.

Retries can repeat remote work. A returned stream, reported partial usage, policy denial, or provider-owned Responses identifier makes unrestricted replay unsafe. Failover also needs a shared failure state so that another replica does not select the same failed route at once.

## Decision

### Make failover an explicit pool policy

Add optional `routing.failover` to provider-backed models. Use an empty object to enable the bounded defaults. If absent, the policy allows one provider attempt. Aliases inherit the resolved target's policy. Admins must apply this policy through YAML and seed the config. Durable batches keep their existing route lifecycle.

The [canonical routing guide](../configuration/model-routing-and-api-behavior.md#enable-bounded-provider-failover) owns all settings and runtime rules. Defaults allow two retries per route and six dispatches in total. Backoff starts at 250 ms and is capped at 2,000 ms, without jitter. Quota and credential cooldowns default to 300 seconds; transient cooldowns default to 30 seconds. The default cooldown cap is one day.

Hard limits allow at most five retries per route and 32 total attempts. Each retry delay is capped at 30 seconds. Each cooldown is capped at seven days. A parsed `Retry-After` can extend a retry delay within its cap. A larger value ends retries on that route. Cooldown takes the larger of its set duration and `Retry-After`, then applies the cooldown cap.

### Classify failures before retrying

HTTP `408`, `429`, and `5xx`, transport errors, and timeouts are transient. They can retry the same route before fallback. Recognized quota failures and HTTP `401` skip same-route retries and can use another eligible route. Invalid requests, unsupported operations, unknown HTTP failures, and explicit policy denials stop the request.

Policy denial codes take precedence over HTTP status and quota markers. The check reads only structured code and type fields. Free-text messages and other nested content cannot change the retry decision. Copilot HTTP `402` and a small set of exact quota codes have provider-specific meaning. These codes do not apply to other providers. The [Copilot provider page](../providers/github-copilot.md#quota-and-policy-errors) records the first-party source evidence and its limits.

The retry loop ends when a `ProviderClient` method returns a value or stream. It cannot replay a returned stream, even before its first event reaches the caller. It cannot retry a successful HTTP response with an unreadable body or a partial-usage failure. Gateway guardrail and budget failures also stop retries. Each fallback starts from the original input and applies that route's prompt policy. Each actual dispatch checks the gateway budget.

### Keep failure state shared and scoped

Use PostgreSQL or libSQL/SQLite for route cooldowns. The service constructs an opaque cooldown key from the route and its runtime provider and credential context. User-owned credentials add the user ID and credential generation. The user's API keys, aliases, endpoints, and sessions share this state. Shared provider credentials share the route cooldown across callers.

The scope is route-level. It does not assume the provider applies quota to an entire account or to one route. It does not borrow tokens across users. Credential replacement creates a new identity; normal short-lived token refresh does not. No quota reset is inferred from error messages or a default cooldown.

Store selection excludes cooldowns before it reuses affinity or applies priority and strategy. When all otherwise eligible candidates are cooled, return HTTP `503` with `routes_temporarily_unavailable`. A route can receive new placements after expiry. This is expiry-based recovery; it adds no active probe or adaptive health score.

### Preserve session and continuation ownership

After same-route retries end, record the cooldown and invalidate the failed affinity reservation in one transaction. Deletion checks the binding's receipt and route ID. A late failure cannot remove a newer reservation. Cooldown updates keep the later expiry, so a late update cannot shorten it.

The current request skips a route once it leaves that route. Fallback uses the remaining eligible candidates, preserving priority and the configured strategy. Selection checks the current provider and caller credential eligibility. It reserves the replacement binding, and success refreshes the deadline. A recovered preferred route does not move a session from a healthy fallback binding.

Responses with `previous_response_id` retain their recorded origin. They can retry that origin within the policy but cannot fail over to another route. Origin cooldown prevents selection until expiry. Existing origin retention and persistence rules remain unchanged.

### Record dispatches and account for completed work

The HTTP execution layer owns the attempt loop. The domain layer owns failure classes and policy validation. The service owns cooldown identities and routing receipts. The store owns shared expiry and receipt-checked state changes.

Each actual provider dispatch produces an ordered attempt record when request logging is enabled. Only the last dispatch is terminal, including when later route preparation fails before another dispatch. The successful result is accounted for once against the winning route. A partial-usage error stops retries and still records any usage supplied by the provider.

The gateway cannot guarantee exactly-once execution at the provider. A timeout or transport error can follow accepted work without reported usage. Attempt limits bound this risk but do not remove it. Retry latency includes provider timeouts and backoff; the attempt count is not a total request deadline.

## Storage and rollout

Migration `V56__model_route_cooldowns.sql` adds `model_route_cooldowns` in both database backends. The table has an opaque `cooldown_key` primary key, an `expires_at` timestamp stored as an integer, and an expiry index. It has no route foreign key because configuration seeding replaces route rows. Expired records do not affect selection.

Apply the migration before running the new gateway code. Enable failover only in selected pools through configuration seeding. Removing `routing.failover` stops use of this policy; retained cooldown rows do not affect models without it. Configuration rollback does not reverse the database migration. Follow the normal backup and release process when changing the schema.

## Alternatives and trade-offs

- Keep one attempt: this preserves the original behavior and avoids gateway replay, but every upstream failure reaches the caller. It remains the default.
- Keep cooldowns in memory: this avoids database work but loses state across replicas and restarts. Shared storage matches the existing affinity boundary.
- Treat all authorization errors as quota: this can improve apparent availability but can bypass provider policy. Exact provider signals and terminal unknown errors provide a narrower contract.
- Retry after stream start: this could recover some interrupted output but can duplicate tokens, tool calls, and remote charges. The dispatch boundary gives a clear stopping point.

Failover adds database operations and can increase request latency and remote cost. Route-level cooldowns can leave other routes on the same constrained provider account eligible. This trade-off avoids inferring an account-wide limit without evidence.

## Follow-up work

- Observe provider error classes, fallback frequency, cooldown duration, request latency, and remote cost under representative traffic.
- Add admin controls and cooldown views only after their access and recovery behavior are defined.
- Revisit quota scope and error codes when first-party evidence changes or live behavior shows a gap.
- Consider active recovery probes, jitter, and a total retry deadline if measured traffic requires them.

No live Copilot quota or provider billing behavior is established by this decision. Unit, store, and HTTP fixtures verify gateway behavior; live account behavior remains a separate validation step.

## References

- [Model Routing and APIs](../configuration/model-routing-and-api-behavior.md#enable-bounded-provider-failover)
- [GitHub Copilot quota and policy errors](../providers/github-copilot.md#quota-and-policy-errors)
- [Data Relationships](../contributing/reference/data-relationships.md#route-cooldowns)
- [Migration Authoring](../contributing/reference/migration-authoring.md)
- [Provider failure classification](../../crates/gateway-core/src/provider_failure.rs)
- [Online provider execution](../../crates/gateway/src/http/handlers/execution.rs)
