# Agent Task Analysis and Efficiency Interview

`See also`: [Agent Task Analysis and Efficiency](../plans/2026-07-20-agent-session-analysis-and-efficiency.md), [Agent Harness Usage](../operations/agent-harness-usage.md), [Request Logs](../operations/observability/request-logs.md), [Pricing Catalog and Accounting](../configuration/pricing-catalog-and-accounting.md)

- Date: 2026-07-20
- Tracking issue: [#255: Add outcome-aware agent session efficiency analytics](https://github.com/ahstn/oceans-llm/issues/255)
- Participants: project maintainer and coding agent
- Outcome: shared implementation direction captured in the linked plan
- Scope: product semantics, passive correlation, scoring, provider accounting, persistence, authorization, API, UI, documentation, rollout, and deferred work

## Purpose

Issue #255 and the original plan proposed a broad harness-neutral session-analysis subsystem with direct semantic events, offline imports, verified outcomes, a 0–100 score, storage, APIs, UI, calibration, and general availability.

The interview tested those assumptions against:

- the current Rust workspace and dependency direction;
- request logging, usage accounting, provider adapters, and cache pricing;
- LibSQL/PostgreSQL migration and repository conventions;
- admin API authorization and contract generation;
- the existing TanStack Start/shadcn admin UI;
- documentation ownership rules;
- current OpenTelemetry, OpenAI, Anthropic, ReUI, and Pi evidence;
- the practical constraint that Oceans cannot require every caller to configure its harness.

The resulting plan narrows v1 to passive gateway observation while preserving a saved direct-event design for later.

## Evidence Gathered Before Questions

### Repository Architecture

- The workspace is layered: `gateway-core` is foundational, `gateway-store` owns persistence, `gateway-service` orchestrates runtime behavior, and `gateway` owns HTTP/runtime integration.
- A pure `agent-session-analysis` crate can remain a foundational leaf. It must not depend on `gateway-core` if `gateway-core` depends on its stable IDs/contracts.
- Repository traits belong in `gateway-core`; LibSQL, PostgreSQL, and `AnyStore` must implement/forward new traits.
- Existing tests use deterministic table-driven/unit fixtures and in-memory repositories. There is no current property-test dependency.

### Request And Usage Accounting

- Request logging and usage accounting independently parse top-level prompt/input, completion/output, and total token fields.
- The two usage parsers have different overflow behavior.
- `UsageLedgerRecord` stores cache rates, but `service.rs::apply_token_rates` accepts only input/output rates and computes cost from prompt/completion tokens.
- Cache quantities are provider-specific, partially preserved, or dropped depending on the adapter.
- `UsageLedgerRecord` has no `request_log_id`; request logs, ledger rows, and MCP invocations correlate optionally through `request_id`.
- Current provider request attempts are always attempt `1`; no retry loop exists.

### MCP Semantics

- Effective grants are potential inventory, not proof of request-supplied schemas.
- Request tool cardinality can be overwritten with grant cardinality today.
- Direct MCP invocations, response tool calls, supplied request tools, and granted tools are distinct facts.
- MCP aggregate session IDs represent MCP transport/authentication state and cannot be agent session IDs.

### Storage And Migrations

- `V17` is the reset baseline; the active migration registry continues through `V40`.
- New migrations require paired LibSQL/PostgreSQL SQL and registry entries.
- `ACTIVE_APPLICATION_TABLES` omits several V29, V34, and V35 tables, weakening reset-history validation.
- Request-log purge does not automatically remove usage or MCP facts.
- No immutable analytical-report persistence exists today.
- No generic public asynchronous job resource exists; maintenance uses CLI commands, internal loops, or synchronous refreshes.

### Admin API And UI

- Current observability routes are platform-admin-only.
- The admin UI root rejects non-platform-admin sessions.
- The identity schema enforces one team membership per user.
- Request-log HTTP pagination accepts 500 while both stores clamp to 200.
- Rust/`utoipa` DTOs generate checked-in OpenAPI and TypeScript contracts.
- The UI uses `radix-nova`, Hugeicons, shadcn, TanStack Start, Recharts, and React Virtual. It has no ReUI configuration.
- Request-log detail uses a right-side Sheet; observability list/filter/chart patterns already exist.

### External Evidence

- OpenTelemetry GenAI conventions now live in the dedicated `semantic-conventions-genai` repository and include development-stage conversation, compaction, tool, evaluation, cache, and reasoning attributes.
- OpenAI GPT-5.6 and later report billable cache writes in addition to cached reads; earlier model families have different write pricing.
- Anthropic reports cache reads and cache creation separately with TTL-dependent write pricing.
- Pi upstream documents alignment of `prompt_cache_key`, `session_id`, and `x-client-request-id` from a session-derived UUIDv7 value.
- A user-supplied real Pi request capture corroborated the presence of a `session_id` header, but also contained deployment-injected headers and sensitive payload material unsuitable for repository fixtures.
- ReUI provides free Data Grid and Filters components with documented TanStack/server-side composition.

## Product And Rollout Decisions

### Canonical Score Name

**Question:** The draft scores one task but calls the headline `Session Efficiency Score`. What should be canonical?

**Decision:** `Task Efficiency Score`.

**Implication:** Session correlation may group tasks, but score/API/UI language is task-scoped.

### Delivery Shape

**Question:** Should issue #255 be one implementation PR, split into child issues first, or remain an epic with gated PRs?

**Decision:** Keep #255 as an epic with gated, independently reviewable PRs.

**Implication:** Source fixes, accounting, task facts, pure analysis, persistence, API, UI, calibration, and aggregates have separate exits.

### Initial Admin Audience

**Question:** Platform admins only, platform plus team admins, or internal maintainers only?

**Decision:** Platform and team admins.

**Follow-up evidence:** The current admin shell is platform-only and users can belong to at most one team.

**Refined decision:**

- preserve the single-team identity model;
- add a route capability matrix to admit active team Owners/Admins without exposing platform-only pages;
- authorize team analytics against current team membership;
- team admins may see user-owned tasks for current team members and team service-account tasks.

### Numeric Score Visibility

**Question:** When should the number appear in the admin UI?

**Decision:** After shadow calibration.

**Implication:** Platform admins may inspect shadow diagnostics behind a runtime capability. The normal headline remains withheld until grouping, cost, cohort, and sensitivity checks pass.

## Ingestion And Correlation Decisions

### Required Harness Setup

**Question:** Should trusted harnesses be required to send correlation headers and events?

**Maintainer constraint:** Oceans cannot control every user's harness setup and cannot rely on configuration being installed.

**Decision:** Do not require a harness integration in v1. Save and skip the direct event-ingestion design.

### V1 Ingestion Surface

**Question:** Passive gateway plus offline import, offline import only, passive gateway only, or defer?

**Decision:** Passive gateway only.

**Implications:**

- no Pi transcript importer;
- no browser upload;
- no watched directory;
- no event API;
- reports are sparse where gateway evidence cannot observe local behavior.

### Passive Report Semantics

**Question:** Should passive reports have no score, estimates, or sparse provisional metrics?

**Decision:** Provisional and sparse.

**Implication:** Missing dimensions are unavailable and lower coverage/confidence. They never receive hidden neutral values.

### Task Grouping Unit

**Question:** What should passive grouping be called?

**Decision:** `Inferred task window`.

**Implication:** The canonical UI resource is `Agent tasks`, but detail discloses that the boundary is inferred.

### Timing Defaults

**Question:** Fixed constants, per-harness config, learned thresholds, or versioned defaults?

**Decision:** Versioned configuration defaults.

**Selected defaults:**

- task split: 30 minutes of inactivity;
- active orchestration gap cap: 2 minutes.

### Session Grain

**Initial decision:** Do not invent a second grouping above task windows without evidence.

**Additional evidence:** Observed Pi requests include `session_id`, and Pi upstream documents session-derived affinity identifiers.

**Refined decision:**

- create an agent-session correlation resource when `session_id` is present;
- keep task windows as the scored grain;
- sessionless traffic produces task windows without fake sessions;
- investigate other harnesses later.

### Session Header Policy

**Question:** Recognize only harness adapters, any `session_id`, Pi only, or require corroboration?

**Decision:** Accept any bounded `session_id` header.

**Superseded during implementation:** Accept only bounded session identifiers supplied by a versioned, recognized harness adapter. Unknown or policy-blocked harnesses remain sessionless. See [Passive Agent Correlation](../contributing/reference/agent-session-analysis.md#passive-correlation-and-task-boundaries).

**Safety constraints added to the plan:**

- treat it as self-reported correlation, never identity;
- scope it by authenticated owner/caller;
- validate length/characters;
- reject/limit conflicts;
- never use infrastructure forwarding, CDN, cloud trace, or IP headers as session identity;
- never use it for authorization or billing.

### Session ID Storage

**Question:** Keyed hash, raw value, unkeyed hash, or memory-only?

**Decision:** Store the raw value.

**Implication:** Raw session values require bounded validation, owner-scoped uniqueness, role-scoped detail access, retention, and deletion controls.

## Outcome And Score Decisions

### Outcome Authority

**Question:** Typed trusted evidence, admin adjudication, delivery proxies, or external evaluator?

**Initial decision:** Typed trusted evidence only.

**Constraint discovered:** Passive gateway-only telemetry has no direct semantic outcome evidence and would produce no verified-success cohorts.

**Question:** Experimental provisional only, request success as outcome, admin adjudication, or no score?

**Decision:** Use request success as outcome.

**Terminology safeguard:** The API/UI calls this `gateway-observed outcome`, not semantic verification.

### Outcome Formula

**Question:** Final request wins, all requests must succeed, any success wins, or a weighted fraction?

**Decision:** Weighted terminal-request fraction.

**Question:** Equal, cost, token, or operation-specific request weights?

**Decision:** Equal request weight.

**Formula:**

```text
O = successful determinate terminal model requests
    / all determinate terminal model requests
```

Provider attempts are excluded.

### Incomplete Requests

**Question:** Failure, partial, ignored, or unknown?

**Decision:** Unknown and lower coverage.

**Implication:** Cancelled/disconnected/usage-only partial requests do not enter numerator or denominator and are reported separately.

### Outcome Label

**Question:** Gateway-observed outcome, verified outcome, or request reliability?

**Decision:** `Gateway-observed outcome`.

### State Model

**Question:** One overloaded score state or separate state axes?

**Decision:** Separate four axes:

- task lifecycle;
- gateway-observed outcome;
- score maturity;
- confidence.

`Verified` is reserved for future semantic evidence.

### Score Weights

**Question:** Keep 0.50/0.30/0.20, reduce proxy outcome weight, defer composite, or configure weights now?

**Decision:** Keep the draft weights as experimental v1 policy.

```text
Task Efficiency Score = round(100 × O^0.50 × C^0.30 × T^0.20)
```

Calibration and sensitivity analysis are required before normal UI exposure.

### Percentile Math

**Question:** Literal `1 − ECDF`, midrank survival, logistic, or p50/p90 piecewise?

**Decision:** Midrank survival percentile.

```text
E(x) = clamp((count(peer > x) + 0.5 × count(peer = x)) / n, 0.01, 1.0)
```

This applies independently to lower-is-better cost and active time.

### Cohort Features

**Question:** Observable request facts, payload archetype, model/harness only, or global?

**Decision:** Observable pre-execution request facts only:

- operation;
- requested model family/generation;
- harness key/version;
- caller class;
- explicit request tags when present.

No post-execution behavior may make a task appear more comparable.

### Minimum Cohort

**Question:** 10, 30, 100/no score, or any sample?

**Decision:** Minimum 10, then hierarchical fallback and versioned configured baselines.

### Active-Time Overlap

**Question:** Interval union, critical path, or sum all work?

**Decision:** Interval union for the headline; expose summed provider/tool work separately.

## Passive Inference And Privacy Decisions

### Unobservable Local Activity

**Question:** Move file/tool/verification/compaction criteria to follow-up, retain permanently unavailable types, infer from payloads, or remain blocked?

**Decision:** Infer them from policy-permitted structured payloads.

**Safeguard:** Persist separate inferred observations rather than direct canonical events.

Examples:

- `file_edit_suspected`;
- `verification_result_classified`;
- `compaction_suspected`.

Every observation carries source, confidence, parser version, coverage, and limitations.

### Payload Privacy Boundary

**Question:** Always inspect in memory, honor request-log capture policy, or add a separate setting?

**Decision:** Honor the existing capture policy.

**Implication:** Disabled and summary-only payload modes produce unavailable payload-derived metrics.

### Parser Evolution

**Question:** Replace observations, append versions, or never reparse?

**Decision:** Append parser-versioned observation sets.

### Real Pi Fixture

**Question:** Commit sanitized capture, handcrafted structural fixture, or no Pi fixture?

**Decision:** No Pi fixture.

**Implication:** The real capture informed the plan but is never copied into the repository. Generic synthetic provider/payload fixtures remain acceptable.

## Provider Accounting Decisions

### Provider Coverage

**Question:** OpenAI only, OpenAI/Anthropic first, or all current provider paths?

**Decision:** All current paths:

- OpenAI-compatible Chat and Responses;
- Anthropic-shaped Messages;
- Bedrock;
- Vertex;
- embeddings;
- streams, including partial/failure usage.

### Budget Cutover

**Question:** Immediate, shadow then cutover, analytics-only cost, or historical repricing?

**Decision:** Shadow then explicit cutover.

**Implications:**

1. persist normalized bucket cost beside legacy authoritative cost;
2. reconcile provider/model discrepancies;
3. switch new usage and budgets through a dated pricing-policy version;
4. avoid ambiguous historical repricing;
5. do not maintain two permanent cost truths.

## Persistence Decisions

### Report History

**Question:** Immutable versioned reports, mutable current report, or compute on read?

**Decision:** Immutable versioned reports.

Each report keys task ID plus input watermark, observation parser, analyzer, score policy, pricing policy, cohort, and report schema versions.

### Late Facts

**Question:** Ignore, new task, mutate, or invalidate/re-report?

**Decision:** Invalidate and append a new report.

### Retention

**Question:** Align all retention, facts align/reports longer, indefinite reports, or no purge?

**Decision:** Facts align with request logs; reports live longer.

**Selected report default:** 90 days.

### Owner Deletion

**Question:** Delete, anonymize, or retain with null owner?

**Decision:** Delete identifiable analytics and recompute aggregates. Existing authoritative billing retention remains separate.

### Recompute Execution

**Question:** Internal queue/CLI, synchronous read, public async job, or full batch?

**Decision:** Durable internal queue plus service loop and maintainer CLI. No public job API in v1.

## Authorization Decisions

### Team Visibility

**Question:** Explicit team-owned tasks only, team members' user tasks, or aggregates only?

**Decision:** Team members' user tasks too.

### Multiple Teams

**Initial answer:** Every current team.

**Repository evidence:** Users currently have exactly one team membership.

**Refined decision:** Preserve the single-team model. A current team Owner/Admin sees retained user-owned task analytics for current members and service-account tasks owned by that team.

### Historical Access

Current membership governs access to retained reports. Transferring a user changes which team Owner/Admin may inspect that user's retained analytics.

### Admin Shell

**Question:** API-only team access, separate portal, defer, or route access matrix?

**Decision:** Route access matrix.

Existing platform-only pages remain platform-only.

## API And UI Decisions

### Canonical Resource

**Question:** Agent tasks, task windows, task analyses, or session analyses?

**Decision:** `Agent tasks`.

### Detail Interaction

**Question:** Dedicated route, Sheet, inline expansion, or persistent split view?

**Decision:** Right-side Sheet.

### Sheet Navigation

**Question:** Local state, nested route, or URL search state?

**Decision:** URL search parameter `task_id` for refresh/back/forward/share behavior.

### ReUI Scope

**Question:** Existing shadcn only, ReUI Filters only, ReUI Data Grid and Filters, or later block?

**Decision:** ReUI Data Grid and Filters.

References reviewed:

- [Data Grid](https://reui.io/components/data-grid)
- [Dense grid preview](https://reui.io/preview/base/components/c-data-grid-3)
- [Filters with grid preview](https://reui.io/preview/base/components/c-filters-7)
- [Async filters with grid preview](https://reui.io/preview/base/components/c-filters-8)

The project remains `radix-nova`, Hugeicons, and shadcn for existing charts/cards/sheets/states.

### Shadow Access

**Question:** Platform only, platform plus team, no UI, or platform sees score too?

**Decision:** Platform admins only.

### Shadow Flag

**Question:** Runtime gateway capability, SSR environment, always visible, or secret URL?

**Decision:** Runtime gateway capability surfaced through authenticated admin bootstrap/session data.

### Aggregate Rollout

**Question:** With shadow UI, after list/detail calibration, or API-only later?

**Decision:** After list/detail calibration.

### Score Presentation

**Question:** Compact summary, hero metric, or badge only?

**Decision:** Compact summary card with outcome, maturity, confidence, coverage, and cost/time components adjacent.

## Documentation Decisions

Repository rules determined the split rather than a product preference question.

### Admin-Facing Canonical Page

The implementation publishes [Agent Session Analysis](../operations/agent-session-analysis.md) as the canonical admin-facing page for:

- agent tasks and inferred boundaries;
- gateway-observed outcome;
- score/maturity/confidence/coverage/cohort;
- shadow/calibrated behavior;
- privacy/retention summary;
- admin workflow.

### Maintainer-Facing Canonical Pages

The implementation publishes:

- [ADR: Passive Agent Task Analysis](../adr/2026-07-21-passive-agent-task-analysis.md) for identity, inference, score, persistence, privacy, and authorization;
- [Agent Session Analysis Reference](../contributing/reference/agent-session-analysis.md) for metrics, provider normalization, passive correlation, privacy, retention, and version policy.

Update existing request-log, harness, MCP, pricing, budget, data-relationship, admin-contract, E2E, and admin-control-plane pages without duplicating canonical policy.

Use `admins`, not `operators`, for people using the control plane.

## Recommendations Accepted

- Rename the score to Task Efficiency Score.
- Keep issue #255 as a gated epic.
- Hide the number until shadow calibration.
- Use provisional sparse reports for incomplete passive evidence.
- Use inferred task windows with versioned 30-minute/2-minute defaults.
- Call request-derived outcome gateway-observed.
- Separate state axes.
- Use midrank survival percentiles.
- Match cohorts only on pre-execution request facts.
- Require a minimum cohort of 10 and disclose fallback.
- Use interval union for active time.
- Normalize every current provider path.
- Shadow cache-aware cost before authoritative budget cutover.
- Keep reports immutable and versioned.
- Align fact retention and use 90-day report retention.
- Delete identifiable analytics on owner deletion.
- Append parser-versioned observations.
- Invalidate/re-report on late facts.
- Use an internal queue and CLI, not a public job API.
- Keep single-team identity and add a route capability matrix.
- Use Agent tasks, ReUI Data Grid/Filters, URL-backed Sheet state, and compact score presentation.
- Split adjacent source fixes into prerequisite PRs.

## Recommendations Overruled Or Narrowed

These decisions are retained because they materially change risk and implementation semantics.

1. **Direct trusted event contract**
   - Recommendation: optional correlation headers plus authenticated event API.
   - Decision: skip in v1 because Oceans cannot require harness configuration.

2. **Passive plus offline import**
   - Recommendation: passive gateway facts plus streaming Pi import.
   - Decision: gateway-proxied requests only.

3. **Move unobservable local metrics to follow-up**
   - Recommendation: do not implement edit/verification/compaction metrics without direct events.
   - Decision: infer them from policy-permitted payloads.
   - Added safeguard: inferred observation types and explicit limitations.

4. **Do not use request success as task outcome**
   - Recommendation: keep verified scoring deferred without semantic evidence.
   - Decision: use successful terminal model delivery as outcome input.
   - Added safeguard: canonical label `gateway-observed outcome`; never call it semantic verification.

5. **Harness-adapter-only session headers**
   - Recommendation: allowlisted known harness/version adapters.
   - Decision: accept any validated `session_id` header.
   - Added safeguards: owner scoping, self-reported evidence, no identity/security semantics.

6. **Hash session IDs**
   - Recommendation: deployment-keyed HMAC.
   - Decision: store raw bounded values.
   - Added safeguards: authorization, retention, deletion, and no global tenant join.

7. **Dedicated task detail route**
   - Recommendation: dedicated route because evidence depth is substantial.
   - Decision: right-side Sheet.
   - Added safeguards: wide/full-width responsive Sheet, progressive disclosure, required accessible title, and URL-backed selection.

8. **Explicit team context**
   - Recommendation: require team ID when one admin can manage multiple teams.
   - Decision: union of current teams.
   - Repository evidence then showed single-team membership, so the plan preserves that model and authorizes against the one current team.

## Saved Follow-Up Branch

The plan preserves, but does not implement in v1:

- direct trusted session/task/user-turn/model-turn identifiers;
- authenticated harness event ingestion;
- direct file/tool/verification/compaction/subagent/approval events;
- transcript import and online/offline equivalence;
- semantic verified outcomes and verified-success cohorts;
- learned task archetypes;
- multi-team membership;
- configurable score weights;
- cross-session provider cache-entry lifecycle attribution.

Future direct evidence must have explicit precedence over passive inference and must never silently reinterpret historical reports.

## Verification At Interview Completion

- The implementation plan was rewritten at `docs/plans/2026-07-20-agent-session-analysis-and-efficiency.md`.
- The plan includes source fixes, phased exits, tests, docs ownership, ReUI references, risks, acceptance criteria, and saved follow-up design.
- `mise run docs:check` passed before this interview record was added.
- `mise run docs:check` passed for 57 markdown files after this interview record was added.
