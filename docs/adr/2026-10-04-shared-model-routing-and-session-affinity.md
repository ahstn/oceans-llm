# ADR: Shared Model Routing and Session Affinity

- Date: 2026-10-04
- Status: Accepted
- Issue: [#409](https://github.com/ahstn/oceans-llm/issues/409)

## Context

One Oceans model ID can expose several provider routes. A deployment can route the same logical model through Copilot, OpenRouter, or Bedrock. The existing planner uses priority and weight for each request. This can move a conversation between providers and reduce prompt cache reuse.

Admins need explicit provider preference, weighted random placement, and round robin. Conversations need stable placement across gateway replicas, while callers without a session ID must continue to work. Provider-owned Responses identifiers also need stronger ownership than an idle session binding.

## Decision

### Configure policy on the resolved model

Add an optional YAML `routing` policy to each provider-backed model. It supports `preferred`, `weighted_random`, and `round_robin`. Omitted policy preserves existing weighted selection. Aliases inherit their target's policy, but each requested model keeps its own session scope.

All routes in a configured pool need an explicit stable ID. Priority, weight, and YAML position do not form that ID. Provider or upstream model changes create a different internal route identity. A route fingerprint also tracks its execution context. It uses the initialized provider's endpoint, authentication, and headers, rather than a stored configuration snapshot. It includes API family, request-affecting route settings, and the caller's linked provider credential ID. Display, pricing, timeout, and batch settings are excluded. Changing API family, replacing static credentials, or relinking a user credential invalidates the fingerprint. Normal token refresh within a provider adapter does not.

AWS `default_chain` and GCP `adc` or `service_account` authentication require an explicit provider `routing_account_scope` when used in a configured pool. The label represents the account or principal behind credentials that can change outside YAML. Admins must change it and restart the gateway when that identity changes. Temporary token refresh for the same principal does not require a new label. The gateway does not perform cloud identity discovery and cannot detect an incorrect label or an unreported identity change.

Pool membership is the administrator's assertion that the models are equivalent. This feature adds no model-name compatibility test. Existing authorization, endpoint, feature, and provider support checks remain part of route eligibility. User-owned Copilot routes require the caller's provider connection before selection.

### Separate eligibility from placement

First build the eligible route set for the request. An existing valid session binding can select any eligible route, even if a route with a lower priority value has since been added. Otherwise, select within the lowest eligible priority tier.

Preferred placement chooses the first route by stable internal route ID within that tier. Weighted random preserves the existing weighted ordering. Round robin uses a shared database cursor for each execution model and eligible lowest-priority route-ID set. It sorts that set by stable route ID. Separate cursors prevent callers with different eligible sets from consuming each other's turns. Only a new placement advances the cursor. All strategies exclude disabled routes and routes with non-positive weight.

The HTTP edge owns protocol requirements and provider-client eligibility. The service owns policy, session scope, route fingerprints, and continuation rules. The store owns atomic placement, binding refresh, shared cursors, and response-origin records. The domain module holds the policy and repository contracts.

### Store caller-scoped session affinity

Use the existing PostgreSQL or libSQL database for routing state. No extra cache service is required. The store atomically reuses a valid binding or creates one, so concurrent first requests for one session agree across replicas.

Affinity is opt-in through `routing.affinity`. Its default sliding idle timeout is one hour. The scope includes the caller's API key, requested gateway model, and session namespace. Hash bounded identifiers before storing routing keys; do not inspect prompt text to derive a session.

Accept `x-oceans-session-id` and supported harness session identifiers through the shared session parser. Conflicting or malformed routing identifiers fail before provider execution. Requests without a session ID follow the ordinary routing strategy.

A successful response extends the binding deadline. A stream must complete cleanly before it refreshes the binding. Errors and cancellation do not extend the deadline. Each binding has a unique receipt token; refresh uses that token and route ID so a late request cannot extend a newer binding. A refresh cannot shorten the current deadline. If the soft affinity refresh fails, return the successful result and record a warning; cache placement is not a reason to discard completed provider work.

When a binding expires or its route becomes ineligible, the next request receives a new placement. Priority and weight changes do not invalidate an otherwise valid binding. Session affinity applies to chat, messages, and Responses. Embeddings and decisions use the policy without session affinity. Durable batches retain their existing admission and stored-route behavior.

### Track Responses origin separately

An opaque `previous_response_id` must return to the route that created it. Store response ownership separately from the idle session binding, scoped to the caller and requested model. Retain it for 30 days. A matching origin write can extend retention; a conflicting origin cannot replace ownership. Origin persistence is required before successful completion. A failed origin write reports a request or stream error, even when the upstream provider succeeded.

Origin selection takes precedence over soft session affinity. When affinity is enabled and a session ID is present, a known continuation binds that session to its origin without advancing the round-robin cursor. Successful completion refreshes the binding. Unknown, expired, changed, or ineligible origins fail before provider execution. The gateway does not replay a continuation on another provider. Configured pools reject opaque `conversation` references until their ownership can be tracked. Callers can start a new request with complete history when a continuation is unavailable.

### Keep one provider attempt

This foundation does not add failover, retries, or adaptive selection based on cost, latency, quota, or health. An upstream failure remains a failure of the selected route. The current usage accounting and request logs continue to describe the actual selected provider.

## Consequences

The shared database makes session placement consistent across replicas and process restarts. Stable IDs allow admins to reorder routes or change preference without moving active conversations. Atomic placement and receipt checks protect concurrent creation and late completion.

Affinity adds database work to session requests. Shared round robin also requires a database operation for each new placement. Database contention, retention, and request latency need production observation. PostgreSQL and libSQL must implement the same selection and refresh contract.

The one-hour timeout is a gateway routing choice, not a provider cache guarantee. Provider cache eviction, credential and region boundaries, prompt changes, and OpenRouter's internal routing can still reduce cache hits. No live cache-hit improvement is claimed by this decision.

API-key scope means that rotating the caller's key starts a separate session binding. Requested-model scope means aliases do not share pins. Provider-owned IDs remain non-portable even when administrators consider the underlying models equivalent. The 30-day origin retention is a gateway limit and does not extend provider-side response retention.

Configuration is YAML-only for this implementation. The admin UI does not expose policy editing. Existing deployments must reseed configuration to apply policy changes.

## Follow-up work

- Define failover and retry behavior, including streaming, duplicate work, and session rebinding.
- Add admin controls and operational views for routing policy and binding state.
- Measure database contention, provider distribution, cache reads, and cost under representative traffic.
- Evaluate adaptive routing only after the required signals and policy boundaries are defined.
- Extend ownership tracking before supporting other opaque provider resources across pools.
- Review retention and operational cleanup with production usage data.

## References

- [Model Routing and APIs](../configuration/model-routing-and-api-behavior.md#control-route-selection)
- [Capability-Aware Route Gating](2026-03-13-capability-aware-route-gating.md)
- [Durable Provider Batch Processing](2026-08-17-durable-provider-batch-processing.md)
