**Research: one Oceans model ID across several providers**

Research date: 4 October 2026. Repository baseline: `292351072f7234d37528792bbaf2f16208ca10cb`. Status: research and proposed design; no routing behaviour changed. `gpt-6-sol` is an illustrative public model ID. This note does not verify its availability or equivalence across Copilot, OpenRouter, and Bedrock.

Exa returned 58 search results across three research tracks: provider cache behaviour, Copilot and conversation continuity, and gateway routing policies. That count includes duplicate results. The claims below use fetched primary documentation and current repository source. Two additional searches used the official OpenAI documentation connector.

Oceans already supports several provider routes for one model ID. Extend that design with conversation affinity, selectable routing policies, and bounded failover. Use a one-hour sliding idle timeout as the initial affinity default. Keep provider cache retention and provider-owned conversation state separate from that timeout.

The key rule is: check access and request compatibility, honour any required provider state, reuse a valid session binding, then apply the placement policy. A routing policy selects where a new conversation starts. It should not select a new provider on every turn of an active conversation.

The current implementation provides a useful base:

| Area | Verified behaviour | Design effect |
| --- | --- | --- |
| Route configuration | A model has a route list. Routes carry provider, upstream model, priority, weight, capabilities, and compatibility settings. | Keep the existing public model ID and route structure. |
| Selection | Lower priority values come first. Equal priorities use weighted random ordering. Disabled routes and non-positive weights are excluded. | Provider preference and weighted placement already exist. Round robin would be new. |
| Execution | Inference handlers select one eligible route. They return its error without trying another route. | Priority is not currently operational failover. |
| Access | Model grants, principal restrictions, and allowlists run before route planning. | Preserve these checks on every request and fallback attempt. |
| Sessions | Passive analysis extracts client session IDs. Its idle boundary is 30 minutes. The route planner accepts only routes. | Reuse suitable parsing rules, but add a separate binding lifecycle before execution. |
| Discovery | Model limits use the minimum across routes. Feature summaries require support across routes. Unknown metadata can make the aggregate unknown. | Adding a weaker route can change the contract advertised to clients. |

Evidence: [route config](https://github.com/ahstn/oceans-llm/blob/292351072f7234d37528792bbaf2f16208ca10cb/crates/gateway/src/config/routes.rs#L31), [route planner](https://github.com/ahstn/oceans-llm/blob/292351072f7234d37528792bbaf2f16208ca10cb/crates/gateway-service/src/route_planner.rs#L45), [HTTP selection](https://github.com/ahstn/oceans-llm/blob/292351072f7234d37528792bbaf2f16208ca10cb/crates/gateway/src/http/handlers.rs#L1130), [planner interface](https://github.com/ahstn/oceans-llm/blob/292351072f7234d37528792bbaf2f16208ca10cb/crates/gateway-core/src/traits.rs#L1243), [session analysis](https://github.com/ahstn/oceans-llm/blob/292351072f7234d37528792bbaf2f16208ca10cb/crates/gateway-service/src/agent_analysis.rs#L57), [metadata aggregation](https://github.com/ahstn/oceans-llm/blob/292351072f7234d37528792bbaf2f16208ca10cb/crates/gateway-service/src/model_metadata.rs#L254). The [current routing guide](../configuration/model-routing-and-api-behavior.md#control-route-selection) states the execution limit explicitly.

**Provider cache evidence**

Affinity means that Oceans prefers the same route. A prompt cache contains reusable prefix state inside a provider. The first can improve the second, but it cannot prove that a cached prefix still exists. Cache reuse also depends on the model revision, account, serving location, supported controls, prefix contents, and minimum token threshold.

| Provider path | Documented facts | Consequence |
| --- | --- | --- |
| OpenRouter | Its internal provider affinity expires after 10 minutes of inactivity. Successful requests refresh it. Scope includes account, model, and conversation. `session_id` or `x-session-id` can provide identity; `prompt_cache_key` is a fallback. `provider.order` overrides stickiness. | A one-hour Oceans binding to OpenRouter cannot ensure one hour on the same inner provider. Choose who owns inner provider selection. |
| Bedrock | Cache scope includes AWS account and Region. Supported controls and TTLs vary by model and API. Cross-Region inference can increase cache writes when traffic moves between Regions. | Bind the account and endpoint/profile as well as the model. A stable inference profile still does not prove a stable serving Region. |
| Anthropic | Cached segments must match exactly, including tools, images, and text. Default retention is five minutes; a supported one-hour option costs more to write. Hits refresh retention. | Use a provider-aware cache policy. A one-hour route binding does not enable one-hour cache retention. |
| OpenAI API | The current guide distinguishes model families. GPT-5.6 and later document `prompt_cache_options.ttl: "30m"`, measured from write or reuse. Earlier models have different retention and cache-key routing controls. | Store cache capabilities by model and transport. Do not infer them from an OpenAI-shaped API or a familiar parameter name. |
| Copilot | GitHub and VS Code describe cache-aware product routing and cache controls. Copilot can use several underlying hosts. These product details do not establish a cache-retention guarantee for Oceans' direct HTTP adapter. | Validate the exact adapter path and observe returned cache usage. A pin to Copilot does not prove a fixed inner host. |

Sources: [OpenRouter prompt caching](https://openrouter.ai/docs/guides/best-practices/prompt-caching), [AWS cache scope](https://aws.amazon.com/blogs/machine-learning/optimizing-cost-and-latency-with-amazon-bedrock-prompt-caching/), [Bedrock prompt caching](https://docs.aws.amazon.com/bedrock/latest/userguide/prompt-caching.html), [Anthropic prompt caching](https://platform.claude.com/docs/en/build-with-claude/prompt-caching), [OpenAI prompt caching](https://developers.openai.com/api/docs/guides/prompt-caching). These are documented contracts, not measurements through this checkout.

Copilot evidence: [VS Code token efficiency](https://code.visualstudio.com/blogs/2026/06/17/improving-token-efficiency-in-github-copilot/), [GitHub cache-aware routing](https://github.blog/ai-and-ml/github-copilot/getting-more-from-each-token-how-copilot-improves-context-handling-and-model-routing/), [Copilot model hosting](https://docs.github.com/en/copilot/reference/ai-models/model-hosting). GitHub's Auto routing keeps the selected model between the first turn and the next compaction boundary. This supports placing work at conversation boundaries, though model choice and provider choice remain separate policies. OpenAI API cache rules must not be assumed to apply unchanged through Copilot, OpenRouter, or Bedrock.

For OpenRouter, support two explicit ownership modes. In delegated mode, Oceans sends a stable scoped session ID and accepts OpenRouter's internal routing and ten-minute timeout. In controlled mode, Oceans restricts the request to a validated endpoint and owns fallback. A base provider slug can cover several endpoints, so precise selection may need a full endpoint slug. If using strict endpoint selection, disable unrestricted inner fallback. Preserve any required data policy and parameter support filters. [OpenRouter provider selection](https://openrouter.ai/docs/guides/routing/provider-selection).

Preserve the caller's cache controls and stable prefix. Changing system text, tool order, message serialization, or route transforms can reduce cache reads while the route stays fixed. Cache-control insertion should be a separate, optional feature. This checkout's Bedrock Converse mapper flattens system content to text; it does not translate that path into native `cachePoint` entries. Its usage normalization also flags mixed TTL write classes as unsupported. Resolve those limits before claiming cache control or cost parity across transports. [Converse mapping](https://github.com/ahstn/oceans-llm/blob/292351072f7234d37528792bbaf2f16208ca10cb/crates/gateway-providers/src/bedrock/request/mod.rs#L20), [cache accounting](https://github.com/ahstn/oceans-llm/blob/292351072f7234d37528792bbaf2f16208ca10cb/crates/gateway-service/src/service.rs#L1366).

**Proposed affinity contract**

Use a canonical client conversation ID. Scope the binding by authenticated ownership/caller scope, client namespace, conversation ID, and requested Oceans model ID. Include an API/protocol dimension where continuity requires it. Do not use user ID alone: one user can have several unrelated conversations. Do not use IP address, individual request ID, or an unscoped caller-supplied string.

A dedicated `x-oceans-session-id` header would give plain SDK clients a stable contract. Existing harness session IDs can be accepted through documented adapters with clear precedence and conflict handling. A prompt-cache key may group a shared prefix across conversations, so do not silently treat every cache key as a conversation ID. Derive any forwarded provider session key from the authenticated namespace to avoid collisions between users. A session ID is a routing hint; it must never grant access.

| Proposed setting | Initial behaviour |
| --- | --- |
| Affinity enabled | Opt in for a model with several routes; preserve current behaviour when absent. |
| Idle timeout | `3600s`, sliding from successful upstream use. Rejected requests do not extend it. |
| Expired binding | Re-run placement. The same route may win again; expiry does not require a switch. |
| No conversation ID | Route each request using the configured policy. Report that affinity was unavailable. |
| Binding target | Stable logical route ID, upstream model/revision, transport, account identity, endpoint/Region/profile, and relevant configuration generation. |
| Token refresh | Keep the binding when the provider account and endpoint remain the same. Do not key it by raw token bytes. |
| Preferred provider recovers | Keep an eligible fallback binding until idle expiry by default. Offer explicit draining or rebalance controls. |
| Route disabled or access revoked | Recheck eligibility immediately. Use safe fallback or return an error. |
| Route draining | Stop new placements but allow valid existing bindings to finish. Keep this distinct from disabled. |

LiteLLM provides a useful precedent: affinity is applied before the routing strategy, its default idle timeout is one hour, and shared Redis state supports several gateway instances. Its documented recovery policy returns to the original pin after cooldown; the proposal above deliberately keeps a successful replacement to reduce repeated cache writes. Either policy is valid if it is explicit. [LiteLLM session affinity](https://docs.litellm.ai/docs/routing#session-affinity-sticky-sessions).

Provider-owned state needs stronger rules. A `previous_response_id`, uploaded file, conversation resource, or hosted tool session can belong to a specific endpoint/account. A cache timeout must not erase that ownership. If the owner is unavailable, return a clear continuation error unless Oceans has a complete portable transcript and an explicit replay path. Validate encrypted reasoning, signed thinking blocks, tool-call IDs, and historical content before crossing providers. Matching model names do not establish portability.

OpenAI documents full-history replay and response-ID chaining as separate ways to retain conversation state. Oceans should record the origin of any continuation ID it returns. Its existing replay-ID normalizer repairs ID syntax; it does not establish portability of provider state. [OpenAI conversation state](https://developers.openai.com/api/docs/guides/conversation-state), [Oceans replay IDs](https://github.com/ahstn/oceans-llm/blob/292351072f7234d37528792bbaf2f16208ca10cb/crates/gateway-providers/src/replay_id.rs#L11).

**Routing policies and precedence**

Apply hard access, data-location, capability, credential, budget, and request-size constraints first. Enforce provider-state ownership next. Reuse a healthy eligible affinity binding. For an unbound conversation, choose within the lowest eligible priority tier using the configured strategy. Execute one selected attempt, and apply a separate failover policy if it fails.

| Policy | Meaning | Main trade-off |
| --- | --- | --- |
| Preferred order | Prefer Copilot, then OpenRouter, then Bedrock, when each is eligible. | Concentrates traffic and quota use. Define preference separately from strict provider-only access. |
| Weighted random | Select a new binding using configured weights within a priority tier. | Simple and distributed; weights describe session share, not token or spend share. |
| Round robin | Rotate new bindings through eligible routes in a tier. | Equal conversation counts do not mean equal work. Strict global order needs shared coordination. |
| Weighted round robin | Rotate with configured shares. | More predictable placement counts, with the same workload imbalance risk. |
| Least loaded / quota aware | Select using active work or remaining RPM/TPM capacity. | Needs current shared state, capacity reservations, and request-size estimates. |
| Lowest expected cost | Estimate uncached input, cache reads/writes, output/reasoning, and retry costs. | Lowest list price can be more expensive than a warm route. Missing prices must remain unknown. |
| Lowest latency | Use recent time-to-first-token and generation throughput for comparable requests. | Can overload the current winner; requires minimum samples, smoothing, and slow policy changes. |

Begin with preferred order, existing weighted random, and round robin. Treat affinity as an independent option for all three. Add adaptive policies after enough route-level data exists. A client preference must be validated against admin policy; it must not bypass provider restrictions. Retain the current meaning of zero weight, which excludes the route, and add a separate drain control instead of overloading zero.

If hard affinity requires a route that fails a mandatory constraint, fail that request. Ordinary soft affinity can be broken for health, capacity, or quota limits. A long active session can otherwise keep its route indefinitely. Route weights will not guarantee equal spend or tokens, so set capacity limits independently.

**Failure handling, accounting, and state**

1. Classify errors before deciding to retry. Transient connection failures, selected 5xx responses, and 429 responses can permit fallback when replay is safe. Honour `Retry-After`, an overall deadline, and a small attempt limit. Invalid input, denied access, and policy refusal should not trigger a search for another provider. A credential failure should affect the relevant account/route, not every user's Copilot route.

2. Do not switch after client-visible stream output or after a response is committed. Before output, a timeout can still mean that the provider accepted and billed the request. A hosted tool could already have run. Automatic replay therefore needs both an output boundary and a replay-safety check. Coordinate client, gateway, SDK, and nested-provider retries to avoid multiplied attempts. [LiteLLM retry ownership](https://docs.litellm.ai/docs/routing#retries).

3. Track each upstream attempt under one logical client request. Store route, credential account identifier, provider request ID, status, timing, usage, and retry cause. Keep missing usage and unknown charges explicit. Recheck caller/model budgets before each attempt; add route/account spending limits if required. Record costs already incurred even when the client saw an error. Subscription consumption and cash cost can be separate dimensions; do not assume Copilot capacity is free.

4. The current ledger writes one route's usage and rejects a second `(request_id, owner)` record. The budget precheck also rejects that pair once usage exists. Failover cannot simply call either method again after recording attempt one. Separate logical-request deduplication from per-attempt budget checks. Add per-attempt records or a deliberate aggregate ledger design with preserved provider provenance and idempotent writes. [Usage record](https://github.com/ahstn/oceans-llm/blob/292351072f7234d37528792bbaf2f16208ca10cb/crates/gateway-service/src/service.rs#L911), [duplicate guard](https://github.com/ahstn/oceans-llm/blob/292351072f7234d37528792bbaf2f16208ca10cb/crates/gateway-service/src/budget_guard.rs#L45).

5. Use shared atomic binding state when Oceans has several replicas. Reserve initial placement before dispatch, so two first requests do not choose different routes. Use a bounded reservation lease and generation checks. An old attempt must not overwrite a newer successful fallback binding. Keep ownership of any returned continuation IDs even if the session binding has moved. A short cache refresh write alone is not a sufficient concurrency protocol.

6. A database store can fit the existing deployment; Redis can provide low-latency expiry and shared counters. Measure the added request latency and write rate before choosing. Local memory supports a single process but loses affinity on restart. Consistent hashing reduces shared-state needs, but does not by itself provide sliding idle expiry, explicit rebinding, or durable provider-state ownership.

7. Define store failure behaviour. A request carrying provider-owned state must not fall through to random routing when its owner cannot be resolved. A stateless, soft-affinity request may use a documented deterministic fallback, with a metric that affinity was lost. Bound binding count, identifier size, and retention to stop unused session IDs from exhausting storage.

8. Stabilise route identity before persisting bindings. The current seed UUID includes route priority and array index, so reordering routes or changing preference changes identity. Add a stable logical route key, or define a migration and invalidation policy. Separate harmless weight edits from changes to model revision, account, endpoint, or transport. [Route UUID](https://github.com/ahstn/oceans-llm/blob/292351072f7234d37528792bbaf2f16208ca10cb/crates/gateway-store/src/seed.rs#L32).

9. Stop the attempt chain when the caller cancels or disconnects. Release capacity and pending placement reservations, and stop upstream work where supported. Preserve incurred or unknown charges. Client cancellation must not start fallback or count as a provider-health failure.

**Provider equivalence and access**

For each candidate route, verify exact upstream model/revision, context and output limits, supported input types, tools, JSON Schema, reasoning controls, streaming, cache controls, and usage fields. Verify both the new request and its carried history. Keep the current conservative discovery contract initially; do not advertise a union of features unless execution can always route each advertised feature safely.

Copilot needs a credential preflight before placement. In the current per-user mode, its adapter requires a user-owned API key and that user's linked provider credential. A team key or service account cannot assume access to a user's Copilot subscription. A missing credential should remove that route from this caller's candidates before selection. Keep account-level cooldowns and quota state separate. [Copilot credential resolution](https://github.com/ahstn/oceans-llm/blob/292351072f7234d37528792bbaf2f16208ca10cb/crates/gateway-providers/src/copilot/mod.rs#L215).

GitHub separately documents organization installation tokens, organization policy, and installation-owner billing. Token expiry is independent of cache retention. Its SDK usage guide exposes account quota and billing information, but Oceans' direct HTTP adapter does not acquire those features merely by using Copilot endpoints. Validate any quota integration and account entitlement before making them placement inputs. [GitHub server authentication](https://docs.github.com/en/copilot/how-tos/copilot-sdk/auth/server-to-server-tokens), [Copilot usage and billing](https://docs.github.com/en/copilot/how-tos/copilot-sdk/features/usage-and-billing).

Apply data handling rules to the full fallback set, including OpenRouter's inner endpoints and Bedrock's destination Regions. AWS documents that global inference profile destinations can change, while geography-bound profiles have fixed destination sets. A source endpoint alone cannot establish allowed processing locations. [Bedrock inference profiles](https://docs.aws.amazon.com/bedrock/latest/userguide/inference-profiles-support.html).

**Implementation and validation proposal**

| Stage | Scope | Acceptance evidence |
| --- | --- | --- |
| 1 | Stable route keys, explicit policies, caller-scoped session identity, shared soft affinity, credential/capability filtering. Keep current no-failover behaviour by default. Limit the pilot to verified portable full-history requests; keep opaque-state flows on a fixed origin. | New sessions follow policy; active sessions remain stable across replicas; configuration edits and access revocation have defined effects. |
| 2 | Provider-state ownership, bounded failover, attempt accounting, route/account cooldowns, draining. | Inject 429, timeout, partial stream, and store failure. Show no unsafe replay, lost charges, or cross-owner binding. |
| 3 | Cache-aware cost and latency policies. | Compare against preferred and weighted baselines on fixed workloads. Report observed cache reads, total billed cost, errors, and latency. |

Keep policy types and binding contracts in `gateway-core`; selection and binding lifecycle in `gateway-service`; atomic persistence in `gateway-store`; wire controls and provider error facts in adapters. HTTP handlers should execute the approved plan and enforce stream boundaries. Use one execution policy across Messages, Chat Completions, and Responses. Define separate placement behaviour for embeddings and durable batch jobs, where conversation affinity may not apply.

Record requested model, resolved route, inner provider/serving Region when supplied, policy version, binding age, route-change reason, attempt count, cache read/write tokens, ordinary input tokens, time to first token, total time, and pricing status. Use bounded route labels for metrics and scoped session hashes in logs. Do not treat a session ID, a lower latency, or an estimated warm cache as proof of a cache hit.

The test matrix should cover equal and weighted placement, parallel first turns, expiry, no session ID, two users with the same session string, revoked credentials, removed routes, route reordering, tool-history portability, configuration changes during a request, fallback success, late old-attempt completion, cancellation, disconnects, and ledger idempotency. Cache tests should use fixed prefixes, changed tools, compaction, and idle gaps around provider limits and one hour. Require positive provider-reported cache counters where available. Keep unknown cache evidence distinct from a measured miss.

The first design decisions to settle are the canonical client session contract, preferred-provider recovery behaviour, strict versus delegated OpenRouter routing, provider-state replay scope, and the shared store. The defaults proposed here are enough to start a bounded implementation. Record accepted architecture choices in `docs/adr/` after review; this research note does not mark them as accepted.

Validation boundary: source inspection and primary-document research only. No paid inference, provider entitlement check, cache experiment, or production load test was run.
