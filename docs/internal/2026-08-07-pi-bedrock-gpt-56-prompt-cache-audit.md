# Pi and Bedrock GPT-5.6 prompt-cache audit

Audit date: 2026-08-07

Repository revision inspected: `8a5678cf04666c920364918931d82177115221da`

Related work:

- [Issue #266: Preserve and verify harness prompt-cache semantics end to end](https://github.com/ahstn/oceans-llm/issues/266)
- [PR #256: Agent task efficiency analysis](https://github.com/ahstn/oceans-llm/pull/256)
- PR #256 head inspected: `de1024d174b3399dc77473b577df58739cf9d2dd`

## Executive conclusion

Prompt caching already works for the observed Pi to Oceans to AWS Bedrock Mantle path. The evidence is provider-reported token counters, not latency or HTTP cache headers. Of 198 sampled requests with usable provider usage, 182 had positive cache reads and all 198 had cache writes. Across the sample, 94.03% of input tokens were cache reads.

The main confirmed defect on the current `main` revision is accounting. Oceans retains the provider counters but charges all prompt tokens at the ordinary input rate. This substantially overstates repeated GPT-5.6 Sol request costs and understates first-turn cache-write costs.

PR #256 materially overlaps this issue. It adds provider-aware usage normalization, persists normalized cache buckets, calculates cache-aware component costs, and exposes them to agent-session analysis. It is a partial fix, not a complete fix:

- its default `shadow_legacy` policy keeps the incorrect legacy charge authoritative;
- it does not add the request and route capability contract required by issue #266;
- it does not include a live first-write, second-read provider canary;
- it does not expose the normalized buckets in the general usage-cost interface;
- it is currently based on an old `main`, conflicts with current `main`, and uses migration numbers that now collide.

Do not create a second independent normalization design. Extract or port the cache-accounting layer from PR #256 into a small prerequisite change on current `main`. Then rebase PR #256 so its session analysis consumes that shared result.

## Implementation status on this worktree

The prerequisite cache-accounting slice was implemented on current `main` after this audit. The implementation:

- keeps raw provider usage without modification;
- normalizes Responses input into disjoint uncached, cache-read, and cache-write buckets;
- marks malformed or inconsistent counters unpriced;
- marks a positive cache bucket unpriced when its required rate is missing;
- calculates authoritative cost from uncached input, cache reads, cache writes, and output;
- persists the three normalized buckets in migration V42 for LibSQL and PostgreSQL;
- exposes aggregate cache tokens in spend reports, the admin usage-cost page, FOCUS CSV exports, and OpenTelemetry token metrics;
- changes the checked-in Pi integration model to `openai-responses`;
- adds an opt-in external canary that requires one stable cache key, a first-turn write, a second-turn read, and the same provider, route, and upstream model.

This bounded implementation does not add gateway-selected breakpoints or other cache controls. It also does not add a shadow-cost policy. Valid normalized Responses counters become authoritative because retaining the known aggregate-input overcharge would continue the confirmed defect. Requests without cache detail keep the prior aggregate-input calculation.

The unit, store, gateway, admin UI, generated-contract, harness typecheck, and repository lint gates pass. The external Bedrock canary was not run on this device because no external cache-canary deployment and credentials were supplied. AWS billing reconciliation is still a rollout gate.

PR #256 must now rebase onto this prerequisite. Its duplicate cache normalization, pricing, and persistence must be removed. Its agent-session analysis should consume the ledger buckets added here. Its remaining migrations must start after V42.

## Scope

This audit covers inference prefix and KV-token caching. It does not cover HTTP response caching.

The observed traffic used:

- Pi as the agent harness;
- the OpenAI Responses operation;
- GPT-5.6 Sol and Luna;
- the Oceans `bedrock-mantle-openai` provider;
- AWS Bedrock Mantle Responses endpoints.

The source export contained complete request and response payloads. This document does not reproduce prompts, tool schemas, encrypted reasoning content, user data, credentials, or project content.

## Evidence source and method

The input was a 200-row PostgreSQL export from July 27 to August 3, 2026, plus a small full-payload sample. The rows correlate:

- `request_logs`;
- `request_log_payloads`;
- `request_log_attempts`;
- `usage_cost_events`.

A positive `input_tokens_details.cached_tokens` value is treated as proof of a provider cache read. A positive `input_tokens_details.cache_write_tokens` value is treated as proof of a provider cache write. A key in the request or lower latency alone is not proof of a hit.

The sample is a filtered operational export, not a random workload sample. It can verify the observed path and identify accounting errors. It cannot establish a fleet-wide cache-hit rate.

## Observed cache behaviour

| Measure | Result |
| --- | ---: |
| Requests | 200 |
| Requests with provider usage | 198 |
| Requests with a positive cache read | 182 / 198, or 91.9% |
| Requests with a cache write | 198 / 198 |
| Input tokens | 15,272,849 |
| Cache-read tokens | 14,360,638, or 94.03% |
| Cache-write tokens | 911,815, or 5.97% |
| Remaining input tokens | 396 |
| Failed attempts | 0 |

### Model-specific results

| Model | Valid usage rows | Rows with cache reads | Cache-read token share | Cache-write token share |
| --- | ---: | ---: | ---: | ---: |
| GPT-5.6 Sol | 183 | 169, or 92.35% | 94.40% | 5.60% |
| GPT-5.6 Luna | 15 | 13, or 86.67% | 77.64% | 22.35% |

For Sol, this is strong caching. The data does not support a conclusion that the cache is performing poorly.

Every valid request had a cache write. This is expected for a growing agent conversation in implicit mode: the provider can read the prior stable prefix and write the new suffix in the same request. A row with both positive read and write counters is not a cache miss.

AWS documents the following GPT-5.6 behaviour:

- implicit caching is the default;
- a cacheable prefix must be at least 1,024 tokens;
- cache entries have a minimum 30-minute TTL;
- cache writes cost 1.25 times ordinary input;
- cache reads receive a 90% discount;
- `cached_tokens` and `cache_write_tokens` are the authoritative response counters.

See [AWS prompt caching](https://docs.aws.amazon.com/bedrock/latest/userguide/prompt-caching.html) and [AWS Bedrock pricing](https://aws.amazon.com/bedrock/pricing/).

## Request and route findings

The observed live path already has the minimum request behaviour needed for implicit caching:

- requests use the Responses operation;
- Pi supplies a stable `prompt_cache_key` for a session;
- Oceans preserves the request field;
- Bedrock returns positive read and write counters;
- all 185 sampled Sol requests used one Sol route and upstream model;
- all 15 sampled Luna requests used one Luna route and upstream model.

The request sample did not contain `prompt_cache_options` or explicit `prompt_cache_breakpoint` fields. This does not disable caching because Bedrock uses implicit mode by default.

Do not inject explicit breakpoints at the gateway based on this evidence. Sol already read 94.40% of input tokens from cache. Gateway-selected breakpoints would impose policy that belongs to the harness and could reduce reuse of the growing conversation prefix.

The checked-in Pi integration fixture still needs to use the Responses API so it represents the observed production path. This is a test correction, not a production cache-enablement change.

## Confirmed accounting defect on current main

At revision `8a5678c`, [`apply_token_rates`](https://github.com/ahstn/oceans-llm/blob/8a5678cf04666c920364918931d82177115221da/crates/gateway-service/src/service.rs#L758-L802) sends aggregate prompt and completion tokens to `compute_usage_cost`. The function applies only ordinary input and output rates. It does not use the cache-read and cache-write rates stored on [`UsageLedgerRecord`](https://github.com/ahstn/oceans-llm/blob/8a5678cf04666c920364918931d82177115221da/crates/gateway-core/src/domain.rs#L957-L990).

This has opposite errors:

- cache reads are charged too highly;
- cache writes are charged too cheaply.

For the 198 rows with provider usage, Oceans recorded approximately `$77.81`. Applying the current public AWS cache rates to the exported provider counters gives an estimated `$16.31`.

This estimate demonstrates the scale and direction of the defect. It is not an invoice reconciliation because:

- event-time route rates can differ from current public rates;
- per-request Money4 rounding affects totals;
- AWS Cost and Usage Report data was not inspected;
- the export does not contain an AWS invoice request identifier.

## PR #256 overlap analysis

PR #256 is open at `de1024d174b3399dc77473b577df58739cf9d2dd`. On 2026-08-07, GitHub reported it as conflicting with current `main`.

### What PR #256 already implements

The branch introduces [`NormalizedUsageAccounting`](https://github.com/ahstn/oceans-llm/blob/de1024d174b3399dc77473b577df58739cf9d2dd/crates/gateway-core/src/domain.rs#L949-L1026). It keeps the following disjoint or provider-qualified facts:

- fresh input tokens;
- cache-read tokens;
- cache-creation tokens;
- TTL-specific cache-creation tokens;
- output and reasoning tokens;
- provider total tokens;
- component costs, normalized cost, legacy cost, discrepancy, and authority;
- semantics version, coverage, and normalization errors.

The new [`usage_normalization` module](https://github.com/ahstn/oceans-llm/blob/de1024d174b3399dc77473b577df58739cf9d2dd/crates/gateway-service/src/usage_normalization.rs#L203-L263) parses provider-specific token shapes. Its OpenAI Responses paths include the exact Bedrock Mantle fields observed in this audit:

- `input_tokens_details.cached_tokens`;
- `input_tokens_details.cache_write_tokens`.

The module subtracts cache read and creation buckets from the Responses aggregate input count and rejects inconsistent or negative buckets. Its unit test covers an OpenAI Responses example with both cached and written tokens.

The pricing path calculates ordinary input, cache-read, cache-write, and output components separately. Missing required cache rates make normalized pricing unavailable rather than treating the missing rate as zero. See [`apply_token_rates`](https://github.com/ahstn/oceans-llm/blob/de1024d174b3399dc77473b577df58739cf9d2dd/crates/gateway-service/src/service.rs#L1149-L1258).

Migration V41 persists the result as `usage_cost_events.normalized_usage_json`, and the agent-session analysis consumes those normalized token and cost facts.

### What PR #256 does not fix by default

The branch defaults to [`UsageCostPolicy::ShadowLegacy`](https://github.com/ahstn/oceans-llm/blob/de1024d174b3399dc77473b577df58739cf9d2dd/crates/gateway-service/src/service.rs#L51-L56). Under this policy:

- normalized component costs are calculated and persisted;
- session diagnostics can display the better accounting;
- `computed_cost_usd`, budgets, and general spend reports still use the legacy aggregate prompt cost.

Changing to `normalized` requires `GATEWAY_USAGE_COST_POLICY=normalized` and an explicit `GATEWAY_NORMALIZED_COST_CUTOVER_APPROVAL` value. This is a useful rollout gate, but it means merging PR #256 alone does not correct authoritative billing.

PR #256 also does not complete these issue #266 requirements:

- declare route capabilities for cache controls and usage counters;
- reject a route-level `extra_body` value that conflicts with harness cache intent;
- preserve or translate cache controls for all supported provider families;
- test request-shape preservation for Pi and Bedrock Mantle;
- run a live first-write, second-read provider canary;
- expose normalized cache buckets in the general usage-cost API and interface;
- reconcile normalized totals against provider billing data.

### Current integration conflicts

PR #256 branched from `981f0d58c9411aa044a6d856649ab81fb3023620`. Current `main` is `8a5678cf04666c920364918931d82177115221da`.

Current `main` already uses migration V41 for MCP OAuth state. PR #256 uses V41 and V42 for agent-session analysis. A rebase must renumber or split those migrations. The branch also overlaps files changed on current `main`, including:

- `crates/gateway-core/src/domain.rs`;
- `crates/gateway-service/src/service.rs`;
- both usage ledger store implementations;
- `crates/gateway/src/main.rs`;
- generated admin contracts and UI navigation.

## Recommended integration order

### 1. Extract a cache-accounting prerequisite on current main

Port the following coherent slice from PR #256:

- `usage_normalization.rs` and its provider-shape tests;
- `NormalizedUsageAccounting` and `UsageCostAuthority`;
- normalized usage persistence;
- cache-aware component pricing and shadow discrepancy;
- the guarded shadow-to-normalized policy switch.

Use the next available migration number on current `main`. At the time of this audit, that is V42.

Add the production evidence case used in this audit as a redacted test fixture:

```json
{
  "input_tokens": 3618,
  "output_tokens": 303,
  "total_tokens": 3921,
  "input_tokens_details": {
    "cached_tokens": 3340,
    "cache_write_tokens": 276
  },
  "output_tokens_details": {
    "reasoning_tokens": 95
  }
}
```

The expected input buckets are two fresh tokens, 3,340 cache-read tokens, and 276 cache-write tokens.

### 2. Complete the small issue #266 request contract

In a separate bounded change:

- keep Pi on Responses for the Bedrock Mantle model;
- preserve `prompt_cache_key`, `prompt_cache_options`, and nested breakpoints;
- do not inject cache controls;
- reject conflicting route overrides rather than silently replacing harness intent;
- declare that the pinned Bedrock Mantle route can preserve GPT-5.6 cache controls and counters;
- add request-shape and streaming usage tests.

### 3. Run shadow accounting and a live canary

Before authoritative cutover:

- compare legacy and normalized event costs;
- verify missing-rate and malformed-counter behaviour;
- run the same stable-prefix request twice;
- require first-turn `cache_write_tokens > 0`;
- require second-turn `cached_tokens > 0`;
- verify the same provider, model, region, account or project, endpoint, route, and key;
- reconcile aggregate normalized cost with AWS billing data where possible.

### 4. Rebase PR #256 onto the prerequisite

After the cache-accounting slice lands:

- rebase `codex/analyze-codeburn-session-efficiency` onto current `main`;
- drop its duplicate normalization, accounting, and persistence changes;
- keep agent-session analysis as a consumer of `normalized_usage`;
- remove the duplicate `normalized_usage_json` alteration from its agent-session migration;
- renumber the remaining agent-session migrations after the prerequisite migration;
- regenerate contracts after resolving current admin UI changes.

If the cache prerequisite uses V42, the remaining PR #256 migrations should start at V43.

## Rollout gates

Do not make normalized cost authoritative until all of these conditions pass:

1. OpenAI Responses read and write counters normalize into disjoint buckets.
2. Required cache prices resolve for the selected Bedrock route.
3. Malformed or inconsistent counters do not become zero-cost input.
4. Shadow discrepancy matches a manual calculation on redacted production samples.
5. Budget and reporting queries use the selected authority consistently.
6. A live repeated-request canary proves a write followed by a read.
7. AWS billing reconciliation is within the agreed tolerance.

## Limitations and unknowns

- Two sample rows had no provider usage.
- 166 request payloads were truncated, so the export cannot group every request by visible cache key.
- The sample does not test reuse beyond the 30-minute minimum TTL.
- It does not compare implicit and explicit breakpoints.
- It does not prove behaviour for multiple weighted routes or cross-region routing.
- No live provider request was sent during this audit.
- PR #256 source and tests were inspected, but its branch was not rebuilt during this audit.
- A successful unit test does not prove deployed billing behaviour.
