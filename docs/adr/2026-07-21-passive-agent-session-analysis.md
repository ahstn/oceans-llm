# Passive, Versioned Agent Session Analysis

- Status: Accepted
- Date: 2026-07-21
- Issue: [#255](https://github.com/ahstn/oceans-llm/issues/255)

## Decision

Oceans derives operational agent session diagnostics passively from authenticated gateway request facts. Correlation, inferred observations, reports, and recomputation state are persisted in dedicated append-oriented tables. The scoring formula lives in a dependency-light Rust crate. Numeric presentation is separated from collection by runtime capabilities: passive collection may run while only platform admins can inspect shadow diagnostics, and team access requires an explicitly calibrated rollout.

## Implementation

- Authenticated ownership scope and bounded harness metadata produce deterministic, owner-scoped session-source and session candidates.
- Requests without sufficient source evidence form source-less session windows; the gateway does not invent an external session identity.
- Raw prompts, responses, file paths/content, tool arguments/outputs, arbitrary headers, hosts, and IP addresses are excluded from default analysis facts.
- Session-request ordinals are assigned atomically by the repository.
- Idle finalization appends a versioned recomputation job. The leased worker appends an immutable analysis and marks the previous current analysis stale.
- `agent-session-analysis` owns deterministic outcome, active-time, cohort-rank, confidence, and score calculations without gateway/store/provider dependencies.
- LibSQL and PostgreSQL implement one repository contract and paired V41 schemas.
- Admin list/detail APIs apply one server-side platform/team scope. The authenticated session projects the runtime capability matrix used by UI guards and navigation.
- The admin route reuses ReUI Data Grid and Filters with URL-backed server pagination, filtering, and detail selection.
- Cache-aware normalized usage and cost are retained alongside legacy cost with a versioned pricing policy. Legacy cost remains authoritative during the shadow phase.

## Why

Request-scoped observability cannot explain the operational cost and time of a multi-request agent session. Provider or harness conversation IDs are also not reliable user identity and cannot safely be joined globally. Dedicated session facts provide a bounded, auditable unit while preserving request-level lineage.

Append-only, versioned reports avoid silently changing historical interpretation when parser, boundary, cohort, pricing, or score policies change. A pure analysis crate makes the formula reproducible and testable independently of storage and runtime behavior. Runtime gates prevent an experimental score from being presented as authoritative before calibration evidence exists.

## Trade-offs

- Passive boundaries are operational estimates. Confidence, coverage, and limitations must travel with every score.
- Duplicate schema and SQL work is required for LibSQL and PostgreSQL.
- Immutable versions and recomputation queues consume more storage than overwriting one report.
- Shadow collection produces data that is not immediately visible to team admins.
- Aggregate monitoring is delayed until grouping, sensitivity, cohort, and pricing reviews are complete.
- Missing provider usage or cache rates remains unavailable rather than being imputed, so some sessions cannot receive a score.

## Rejected Alternatives

### Reuse request logs as the session model

Rejected because request logs lack a canonical multi-request session boundary and immutable analysis-version history.

### Treat external session values as identity

Rejected because values are harness-specific, optional, reusable, and not authenticated identities. They are only bounded correlation evidence within an ownership scope.

### Store raw prompts or tool/file payloads for richer inference

Rejected because the operational metrics do not justify the privacy and secret-retention risk. Optional classifications use bounded metadata and opaque IDs.

### Calculate scores in SQL or the admin UI

Rejected because backend-specific formulas and client-side calculations would drift. One pure Rust contract is the reproducible authority.

### Enable the numeric score and aggregates immediately

Rejected because no deployment has yet supplied the shadow calibration evidence required to establish grouping error, stable ordering, cohort sufficiency, and pricing agreement.

## Follow-up

- Review shadow grouping samples without retaining raw content.
- Run score-weight sensitivity analysis and validate fallback behavior at minimum cohort size 6.
- Review normalized-versus-legacy provider cost discrepancies and approve a dated authoritative pricing cutover.
- Enable calibrated and team-admin capabilities only after those reviews are recorded.
- Add the separately gated aggregate endpoint and accessible admin monitoring only after list/detail calibration.
