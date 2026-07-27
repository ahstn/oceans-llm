# Agent Session Analysis

`See also`: [Admin Control Plane](../access/admin-control-plane.md), [Agent Harness Usage](agent-harness-usage.md), [Request Logs](observability/request-logs.md), [Pricing Catalog and Accounting](../configuration/pricing-catalog-and-accounting.md)

Oceans can passively correlate gateway requests into agent task windows and calculate explainable operational diagnostics. The feature measures gateway-observed delivery signals: request outcomes, normalized cost, active request time, confidence, coverage, cohort position, and formula versions. It does not read or claim to verify the semantic quality of an agent's answer.

## Availability

Passive fact collection is enabled by default. The admin surface is separately gated because the score is experimental:

| Runtime variable | Default | Effect |
| --- | --- | --- |
| `AGENT_ANALYSIS_ENABLED` | `true` | Correlate new requests, finalize idle tasks, and run the analysis queue. |
| `AGENT_ANALYSIS_SHADOW_DIAGNOSTICS_ENABLED` | `false` | Let platform admins inspect task diagnostics while withholding the headline score. |
| `AGENT_ANALYSIS_CALIBRATED_SCORE_ENABLED` | `false` | Show the compact score after calibration approval. |
| `AGENT_ANALYSIS_CALIBRATION_APPROVAL_ID` | unset | Immutable deployment approval identifier embedded in newly queued calibrated reports; required when calibrated score visibility is enabled and limited to 256 bytes. |
| `AGENT_ANALYSIS_TEAM_ADMIN_ENABLED` | `false` | Permit active team Owners/Admins to inspect their current team's tasks, but only when calibrated scores are also enabled. |
| `AGENT_ANALYSIS_REPORT_RETENTION_DAYS` | `90` | Retain immutable reports independently after request-aligned task facts expire. |
| `AGENT_ANALYSIS_QUEUE_RETENTION_DAYS` | `7` | Retain completed and terminally failed queue rows for operational diagnosis. |

Boolean variables accept `1`, `true`, `yes`, and `on` or `0`, `false`, `no`, and `off`, case-insensitively; other values fail startup. Retention variables accept unsigned day counts, fall back to their defaults when invalid, and are capped at 36,500 days. Restart the gateway after changing a variable.

The safe initial deployment is passive collection with all presentation flags disabled. Enable shadow diagnostics for platform admins only after confirming the retention and access policy for the deployment.

## Local Development

`mise run dev-stack` refreshes the local demo request and agent-session fixtures, enables shadow diagnostics by default, and starts the report worker. The **Observability → Agent Sessions** page becomes available to the seeded platform admin; reports populate as the queued demo analyses are processed. The curated data includes a five-request operations workflow and a nine-request repository workflow with seven read, write, or edit calls. Set `AGENT_ANALYSIS_SHADOW_DIAGNOSTICS_ENABLED=false` before starting the stack to exercise the hidden-surface state.

Running `mise run gateway-seed-local-demo` separately seeds the same finalized session fixtures and queues their analyses, but does not start the gateway worker or change presentation flags. Start the gateway with shadow or calibrated access afterward to expose the page and process the queue.

## Admin Workflow

When shadow or calibrated access is enabled, open **Observability → Agent Sessions** at `/admin/observability/agent-sessions`.

The session explorer supports:

- server-side pagination capped at 200 sessions per page;
- lifecycle, confidence, user, service-account, harness, model, operation, caller-class, outcome, score-maturity, coverage, normalized-session, request-tag, and start-date range filters;
- URL-backed filters and `task_id` selection, so refresh, back/forward navigation, and shared links preserve the selected session;
- a dense session table with confidence, cost, active time, request, tool-call, direct-MCP-call, and explicit data-quality values;
- a detail sheet with score components, outcome evidence, cohort metadata, typed coverage, immutable analysis identity, complete version boundaries, and bounded request and inferred-observation histories.

In shadow mode, the normal headline score is replaced with **Shadow** or **Withheld in shadow**. Components and evidence remain visible so platform admins can validate grouping and coverage without presenting an experimental number as authoritative.

## Scope and Authorization

- Platform admins retain access to all existing control-plane routes.
- Shadow session diagnostics are platform-admin only.
- Team Owners/Admins receive session access only when both calibrated-score and team-admin capabilities are enabled.
- Team scope is enforced by the gateway for both list and detail requests; a list filter never grants detail access.
- Team Members and inactive users are denied.

The authenticated session response exposes the gateway's effective capabilities. The UI uses those values for route guards and navigation; it does not infer access from a browser-only environment variable.

## How Agent Sessions Are Formed

Each displayed Agent Session is backed by an internal task window. Oceans first uses bounded, authenticated correlation evidence supplied by supported harnesses. Known session headers or payload-policy-permitted metadata are normalized and stored only within the authenticated ownership and harness scope. Disabled and summary-only payload policies permit known session headers but leave body-derived observations unavailable. Requests without sufficient session evidence still form harness-specific, owner-scoped windows instead of receiving a fabricated session identifier.

An open task is reused only within the same ownership scope, harness, explicit request-tag set, and compatible observed session. Model, operation, and caller class remain report dimensions but do not split a semantic task when an agent switches models or request modes. The boundary dimensions are represented by a stable opaque key; raw prompt or response content is never part of the grouping key. Passive correlation is dispatched after request logging and does not delay the model response. Idle tasks are finalized after a versioned 30-minute gap, then queued for analysis. A task link stores a determinate success/failure outcome or an explicit unknown outcome for partial usage-only failures independently of request-log retention. Late evidence invalidates the current report and queues a new immutable report version; an older in-flight worker cannot publish after the task watermark advances or complete work owned by a newer lease.

## Reading the Diagnostics

- **Gateway outcome** is based on determinate HTTP request outcomes. Unknown outcomes remain explicitly unknown.
- **Normalized cost** separates fresh input, cache reads, cache creation, and output when provider usage and rates permit it.
- **Active time** unions overlapping request intervals and caps orchestration gaps. It is not wall-clock elapsed time.
- **Confidence** describes the quality of the available evidence, not the quality of the answer.
- **Coverage** records whether bounded request metadata and response-derived evidence were available and whether the payload was truncated.
- **Observations** in session detail include retained request-level observation sets through the latest parser watermark, preserve each observation's parser version, and never silently relabel historical facts with a newer parser. Request and observation histories are each capped at 1,000 rows; the detail response and UI identify either truncated history explicitly.
- **Cohort** selects prior successful, version-compatible tasks in a fixed fallback cascade: exact boundary, same harness/model/operation/caller, same harness/model, then same harness. Every level requires at least ten peers. The chosen level, sample size, and immutable snapshot digest remain visible; otherwise the score stays unavailable.
- **Limitations** are first-class output. Missing facts are unavailable; they are not converted to neutral or zero values.
- **Versions and identity** expose the immutable analysis ID, input watermark, observation-set ID, cohort-snapshot digest, report schema, analyzer, score policy, pricing policy, observation parser, and task-boundary policy needed to reproduce an interpretation.

A failed determinate gateway outcome scores zero. An unknown outcome uses the disclosed fixed prior and remains distinguishable from success. A numeric zero is therefore not interchangeable with unavailable data.

The score combines outcome, lower-cost cohort rank, and lower-active-time cohort rank with nominal weights of 0.5, 0.3, and 0.2. When one efficiency component is unavailable, it re-normalizes the available weights instead of discarding the remaining valid rank. The component values and limitations disclose which evidence contributed.

## Recompute Retained Tasks

Queue up to 100 retained finalized tasks whose latest report is absent or stale:

```bash
mise run gateway-recompute-agent-analysis
mise run gateway-recompute-agent-analysis-prod
```

Set `LIMIT` to a value from `1` through `1000` to bound one run. To target one retained task directly, run:

```bash
mise exec -- cargo run -p gateway --bin gateway -- recompute-agent-analysis --task-id <UUID> --limit 1
```

The command prints separate `matched_count` and `enqueued_count` values. It is idempotent for the same task watermark and desired version tuple, and it only queues work; the normal gateway analysis worker produces reports.

## Privacy and Retention

Default analysis facts do not store prompts, response content, tool arguments or outputs, source code, file contents, paths, hosts, IP addresses, or arbitrary request headers. Inferred file/tool observations retain only bounded classifications and opaque identifiers. Request payload policy still controls whether enough sanitized metadata exists to infer optional observations.

Request links and inferred observations follow request-log-aligned retention. A minimal task shell remains while an immutable report references it, so reports stay queryable until their independent expiry, which defaults to 90 days. Reports are deleted when their owner is deleted. Completed or terminally failed queue rows default to seven days. An hourly retention loop enforces both independent windows even when passive analysis is disabled; both are configurable with the runtime variables above.

## Current Limits

- The score is experimental until a deployment completes shadow grouping review, sensitivity analysis, cohort validation, and pricing discrepancy review.
- Aggregate monitoring is intentionally not exposed before that calibration gate.
- Provider-specific cache fields can be missing or semantically incompatible; the UI shows the resulting coverage and limitation instead of estimating a value.
- Session correlation is operational grouping, not user identity and not semantic conversation truth.
