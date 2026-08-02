# Agent Session Analysis Architecture

`See also`: [Agent Session Analysis](../../operations/agent-session-analysis.md), [Admin API Contract Workflow](admin-api-contract-workflow.md), [Data Relationships](data-relationships.md), [Passive, Versioned Agent Session Analysis](../../adr/2026-07-21-passive-agent-session-analysis.md)

This page is the maintainer reference for the passive correlation, immutable analysis, storage, API, and admin-UI pipeline tracked by [issue #255](https://github.com/ahstn/oceans-llm/issues/255). Admin-facing behavior and runtime flags belong in [Agent Session Analysis](../../operations/agent-session-analysis.md).

## Ownership Boundaries

| Area | Owner |
| --- | --- |
| Pure formulas, bounded diagnostic contracts, and report types | `crates/agent-session-analysis/src/{lib,extended}.rs` |
| Stable IDs, persistence records, and repository trait | `crates/gateway-core/src/agent_analysis.rs` |
| Passive request metadata, correlation, and response observations | `crates/gateway-service/src/agent_analysis.rs` |
| Provider usage normalization | `crates/gateway-service/src/usage_normalization.rs` |
| LibSQL/PostgreSQL persistence | `crates/gateway-store/src/{libsql_store,postgres_store}/agent_analysis.rs` |
| Backend dispatch | `crates/gateway-store/src/any_store_agent_analysis.rs` |
| Paired schema | `crates/gateway-store/migrations/{,postgres/}V41__agent_session_analysis.sql`, `crates/gateway-store/migrations/{,postgres/}V42__agent_analysis_configuration_version.sql` |
| Background finalization and queue worker | `crates/gateway/src/main.rs` |
| Admin HTTP contract and handlers | `crates/gateway/src/http/{admin_contract,observability}.rs` |
| Runtime authorization capability matrix | `crates/gateway/src/http/{state,admin_auth,identity}.rs` |
| Generated contract | `crates/gateway/openapi/admin-api.json`, `crates/admin-ui/web/src/generated/admin-api.ts` |
| Runtime configuration and effective policy | `crates/gateway/src/{config,main}.rs`, `gateway*.yaml`, `deploy/config/gateway.yaml` |
| Admin route | `crates/admin-ui/web/src/routes/observability/agent-sessions.tsx` |

Keep formulas in the dependency-light analysis crate. Do not move store, provider, HTTP, or configuration dependencies into it. Keep backend SQL in the backend modules rather than branching inside the domain model.

## Request-to-Report Flow

1. Request logging builds bounded `PassiveRequestMetadata` from authenticated ownership, recognized session headers, and only payload-policy-permitted request metadata. Optional `metadata.agent_analysis` input can carry bounded skill and opaque file-operation facts. Before persistence, every file identifier is hashed with the ownership scope so equal raw identifiers cannot be correlated across owners. The parser also records a hash of reasoning settings and whether the request asked for cache control; it never stores skill bodies, file paths, or tool payloads.
2. Successful or failed request finalization prepares bounded observations, including provider finish reasons, and dispatches passive persistence to a Tokio task only when analysis collection is active. A 64-permit non-blocking semaphore bounds in-flight persistence; saturation skips analysis with a warning instead of delaying the model response. Background failures never change response delivery or authoritative billing.
3. Correlation derives an ownership scope, bounded normalized external session identifier, deterministic session ID, harness, and a stable boundary-group key over canonical request tags. Model, operation, and caller class remain analysis dimensions but do not split a session. Open-session lookup requires the same boundary key and compatible observed session.
4. The repository inserts the session-request link, persists its determinate or explicitly unknown terminal gateway outcome independently of request logs, and assigns its session-local ordinal atomically. Repeating the same canonical request is idempotent; reusing its session/request key with different facts returns a store conflict.
5. Each request appends a deterministic, parser-versioned observation set. Non-stream responses and bounded stream-event snapshots use the same response observation classifier. Trace loading aggregates retained observations across all sets through the latest watermark and joins each observation to the parser version that produced it.
6. The background loop finalizes session windows whose input watermark is older than the versioned 30-minute idle gap. Finalization advances the session watermark because the lifecycle transition is report input.
7. Finalization enqueues a versioned recomputation request.
8. A leased worker loads the immutable session trace, request-attempt records, direct tool invocation facts, and a cohort from successful reports with matching report/analyzer/score/pricing/parser/configuration versions. The worker re-derives the effective configuration version before generation and re-enqueues stale leased work when it differs. Selection follows the fixed exact-boundary → harness/model/operation/caller → harness/model → harness cascade, requires at least six peers at every level, and records the selected fallback level. The sorted peer-analysis IDs and values produce a persisted cohort-snapshot digest, so a changed peer population produces a distinct immutable report identity. A late fact or changed configuration makes an older in-flight insert a no-op.
9. While a report is being generated, the worker renews its one-minute lease on a 20-second heartbeat. Queue completion and failure still require the current lease owner. Expired leases at the attempt limit become terminal failures rather than remaining leased forever.
10. Admin list/detail handlers load only the latest non-stale report, expose a typed report/coverage/identity contract, enforce the same platform/team scope, cap list pages at 200 sessions, and cap request and observation histories at 1,000 rows each with explicit truncation flags.

The session link's ordinal is repository-assigned. Callers must not calculate `MAX(ordinal) + 1`; both stores serialize the assignment with a transaction or database lock.

## Data Model and Invariants

- `agent_session_sources`: observed, owner-scoped session correlation. The bounded normalized external identifier is retained for exact admin filtering and display; the deterministic internal UUID remains the relational key.
- `agent_sessions`: open/finalized session boundary, stable boundary-group key, watermarks, confidence, and ownership dimensions.
- `agent_session_requests`: ordered request correlations and bounded limitation codes.
- `agent_inferred_observation_sets` and `agent_inferred_observations`: parser-versioned, append-only classifications derived without raw content. Observation queries preserve the originating set's parser version.
  Parser version `passive-observations-v3` may retain up to 256 supplied tool definitions, supplied skills, and opaque file interactions per request from payload-policy-permitted request metadata. A session trace retains at most 2,048 nested observation facts across those categories; report diagnostic maps retain at most 512 distinct items. Hitting either cap records `payload_truncated`. Tool facts keep a bounded name, optional server key, and token estimate. Skill facts keep bounded names, token estimates, and used/abandoned state. File facts keep only an owner-scoped opaque hash, operation class, bounded tool name, success state, and bounded error code. Response facts can retain bounded finish or incomplete reasons. Raw descriptions, schemas, bodies, paths, arguments, results, and arbitrary attributes are excluded.
- `agent_session_analyses`: immutable versioned reports. The configuration version, session watermark, cohort-snapshot digest, and direct-MCP-invocation snapshot are part of uniqueness, so configuration changes, lifecycle revisions, and different evidence populations cannot collide.
- `agent_analysis_recompute_queue`: leased work with attempts, failure state, desired version tuple, and owner-checked terminal transitions.

Request-log cutoff pruning deletes request links and inferred observation sets. A minimal session shell remains while a report or queue row references it, keeping reports queryable through their independent expiry. Reports retain `ownership_scope_key`, `user_id`, and `service_account_id`; owner deletion still cascades through the shell and reports. An hourly loop enforces report expiry and terminal queue retention even when passive analysis is disabled. The gateway defaults these windows to 90 and 7 days through `AGENT_ANALYSIS_REPORT_RETENTION_DAYS` and `AGENT_ANALYSIS_QUEUE_RETENTION_DAYS`.

Stable IDs are UUIDs derived from canonical bounded inputs and explicit namespaces. Queue IDs include an event-specific deduplication key, observations include their canonical bounded facts, and analyses include the cohort-snapshot digest. A parser or policy change must change the corresponding version input rather than silently reinterpret an existing ID.

The paired V41 and V42 migrations must remain behaviorally equivalent. Validate apply, reapply, and rollback for LibSQL and PostgreSQL. V42 rebuilds the LibSQL analysis table because LibSQL cannot replace the V41 uniqueness constraint in place. New foreign keys must use each backend's canonical `users` key (`user_id`), not an assumed `id` column.

## Passive Session Adapter Registry

`gateway-service::agent_analysis` owns the versioned allowlist. The current adapters are `claude-code-v1`, `codex-v1`, `opencode-v1`, `pi-v1`, and `oh-my-pi-v1`. Each adapter declares accepted session-header aliases and whether bounded body metadata may supply a session candidate. Every accepted alias for a request must resolve to the same normalized value; conflicting aliases are rejected rather than resolved by precedence. Stored provenance records the accepted canonical header or body path.

Unknown harnesses and known harnesses with stripped or policy-blocked metadata remain sessionless. Conversation IDs, request IDs, telemetry IDs, and cache keys are never fallback session candidates. Add or change an alias only with synthetic agreement, conflict, policy-blocking, and unsupported-harness fixtures; bump the adapter version whenever the accepted evidence changes.

## Analysis Contract

`SessionEfficiencyReport` is explainable by construction:

- report, analyzer, score-policy, and pricing-policy versions;
- maturity and confidence;
- gateway outcome;
- optional score;
- outcome, cost-efficiency, and active-time components;
- raw successful/determinate/incomplete request counts;
- actual normalized cost, active time, wall time, summed work time, and excluded gap time;
- cohort version, fallback level, sample size, and snapshot digest;
- explicit limitations;
- a configuration-version hash and the metric groups used to create the report;
- provider-correct total input, visible output, cache-lifetime, write-amplification, threshold-miss, and cache-key diagnostics;
- context boundary, peak utilization, growth, compaction, reset, and score-penalty diagnostics;
- per-server tool exposure/cost, skill loading, attempt waste, fallback routing, tool failures, finish reasons, and coverage-aware outcome signals.

The score is the outcome-weighted geometric combination of outcome, lower-cost cohort rank, and lower-active-time cohort rank. The nominal weights are 0.5, 0.3, and 0.2 respectively; when cost or time is unavailable, the available weights are re-normalized rather than suppressing the whole score. A determinate all-failure outcome returns zero. No cohort means both efficiency ranks and the numeric score are unavailable. Missing evidence must not be represented by a neutral midpoint.

An exact cohort is score-eligible only with at least six successful sessions for both cost and active-time samples. Smaller exact cohorts leave the score unavailable. Versioned fallback cohorts remain usable at reduced confidence and disclose their fallback level and sample size.

Active time is the union of request intervals with the fixed orchestration-gap allowance. Wall time is retained separately. Do not substitute one for the other in UI or aggregates.

Direct MCP evidence comes from invocation records for the session's API key whose completion timestamp falls within the finalized session window. Stored latency reconstructs each call interval; missing latency produces a zero-duration point interval so the invocation still contributes to the count. The analysis identity includes a deterministic snapshot of the matched invocation IDs, timestamps, and latencies. This is temporal attribution rather than a session identifier join, so overlapping sessions sharing one API key can remain ambiguous.

Request attempts are grouped by request and remain in attempt order. An attempt is wasted when it did not produce the final response. A fallback is a provider or upstream-model change between adjacent attempts. Attempt latency is exact where recorded. Per-attempt usage is not available, so the report leaves wasted-attempt cost unknown instead of allocating the final request cost.

File outcome metrics use opaque identifiers. Rework counts repeated writes after the first write to one file. Verification rate divides verification events by writes. Both values remain absent until file-signal coverage is complete. Tool failures and result truncation come from direct tool invocation records; post-error token use is the next measured request input after a failed call.

## Analysis Configuration

`AgentAnalysisConfig` is parsed with `deny_unknown_fields` and converted once at startup into `AgentAnalysisRuntimeCapabilities`, retention durations, and an `AnalysisPolicy`. The policy owns metric switches, context limits, and cache profiles. `desired_versions_for_policy` hashes the serialized policy into `configuration_version`; the queue, report identity, stale check, manual recompute command, and cohort compatibility all use that version. Do not read metric settings from the process environment inside the analysis crate.

Checked-in examples in `gateway.yaml`, `gateway.prod.yaml`, and `deploy/config/gateway.yaml` must stay aligned with `AgentAnalysisConfig`. Environment overrides remain only for the earlier collection, access, approval, and retention settings. New diagnostic settings belong in YAML.

`cache_profiles` are ordered. The first profile whose optional provider and model substrings both match supplies the minimum cacheable tokens and default lifetime. Deployment-specific profiles run before the built-in OpenAI, Anthropic, and Bedrock defaults. Profile changes alter the configuration version and therefore require recomputation; they never rewrite old reports.

## Runtime Capability Matrix

`AgentAnalysisRuntimeCapabilities` is loaded once at gateway startup. The authenticated session projects effective per-user capabilities:

- platform admins remain admins whether or not analysis presentation is enabled;
- platform analysis access requires calibration data access or calibrated score visibility;
- calibration data access is platform-only;
- team Owner/Admin access requires both calibrated-score and team-admin settings;
- ordinary members and inactive users have no analysis access.

`require_agent_analysis_scope` is the server-side authority. UI route checks and sidebar filtering are convenience and must mirror, never replace, that check.

## Admin Contract Workflow

For Rust DTO or endpoint changes:

```bash
mise run admin-contract-generate
mise run admin-contract-check
```

Use generated aliases from `src/types/live-api.ts`. Do not duplicate response interfaces in the UI. Server adapters use `createGatewayApiClient` and `unwrapGatewayResponse`.

The backend list route is `/api/v1/admin/observability/agent-sessions`; detail is `/api/v1/admin/observability/agent-sessions/{session_id}`, and selected detail uses the `session_id` URL search parameter.

The maintenance command `gateway recompute-agent-analysis` scans retained finalized sessions with missing or stale latest reports and enqueues the desired version tuple. `--session-id` narrows to one UUID and `--limit` is bounded to `1..=1000`. Queue IDs preserve idempotency; the command does not calculate reports inline.

## ReUI Contract

The route reuses the installed ReUI `DataGrid`, `DataGridTable`, `DataGridPagination`, `Filters`, and `DateSelector` components. The application-owned `agent-session-date-filter.tsx` composes `DateSelector` in a shadcn Popover, stages changes until Apply, supports single-sided and bounded date filters, and converts local calendar days to UTC query boundaries. The TanStack table is manually paginated from the gateway response. Filters and selection are URL-backed; text-filter drafts update locally and debounce only navigation so controlled inputs never drop keystrokes. Keep registry component source intact except for upstream compatibility fixes; application-specific layout and copy belong outside registry files.

ReUI references:

- [Data Grid API and preview](https://reui.io/components/data-grid)
- [Filters API and preview](https://reui.io/components/filters)
- [Date Selector API and preview](https://reui.io/components/date-selector)
- [Dense grid example](https://reui.io/preview/base/components/c-data-grid-3)

## Verification

Focused checks:

```bash
cargo test -p agent-session-analysis
cargo test -p gateway-service agent_analysis::tests
cargo test -p gateway-store libsql_agent_analysis_repository_round_trips_and_cascades
cargo test -p gateway-store migrations_apply_and_are_idempotent
mise run admin-contract-check
bun run --cwd crates/admin-ui/web build
```

Before handoff, run the repository-required lint command:

```bash
mise run lint
```

Browser QA must cover platform calibration mode, calibrated score mode, denied team access, calibrated team access, deep-linked detail, back/forward navigation, keyboard row activation, filters, empty/error/loading states, and a mobile-width sheet. Do not use raw prompts, source content, tool payloads, credentials, or production identifiers in fixtures.

## Deliberate Gate

Aggregate monitoring remains future work. It requires session-grouping review, score-sensitivity analysis, minimum comparison-group validation, and a dated pricing cutover before an aggregate contract or runtime capability is introduced.
