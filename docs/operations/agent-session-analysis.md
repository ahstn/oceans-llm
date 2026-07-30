# Agent Session Analysis

`See also`: [Admin Control Plane](../access/admin-control-plane.md), [Agent Harness Usage](agent-harness-usage.md), [Request Logs](observability/request-logs.md), [Pricing Catalog and Accounting](../configuration/pricing-catalog-and-accounting.md)

Oceans groups related gateway requests into agent session windows. It calculates request outcome, normalized cost, active request time, confidence, data coverage, comparison position, and formula-version data. It does not read or verify the quality of an agent's answer.

## Availability

The system collects analysis facts by default. Separate settings control access to calibration data and calibrated scores:

| Runtime variable | Default | Effect |
| --- | --- | --- |
| `AGENT_ANALYSIS_ENABLED` | `true` | Correlate new requests, finalize idle sessions, and run the analysis queue. |
| `AGENT_ANALYSIS_SHADOW_DIAGNOSTICS_ENABLED` | `false` | Let platform admins inspect calibration data. The system does not show the session score. |
| `AGENT_ANALYSIS_CALIBRATED_SCORE_ENABLED` | `false` | Show the session score after calibration approval. |
| `AGENT_ANALYSIS_CALIBRATION_APPROVAL_ID` | unset | Identify the calibration approval in new reports. This value is required when calibrated scores are enabled and is limited to 256 bytes. |
| `AGENT_ANALYSIS_TEAM_ADMIN_ENABLED` | `false` | Let active team Owners and Admins inspect sessions for their current team. Calibrated scores must also be enabled. |
| `AGENT_ANALYSIS_REPORT_RETENTION_DAYS` | `90` | Retain immutable reports independently after request-aligned session facts expire. |
| `AGENT_ANALYSIS_QUEUE_RETENTION_DAYS` | `7` | Retain completed and terminally failed queue rows for operational diagnosis. |

Boolean variables accept `1`, `true`, `yes`, and `on` or `0`, `false`, `no`, and `off`, case-insensitively; other values fail startup. Retention variables accept unsigned day counts, fall back to their defaults when invalid, and are capped at 36,500 days. Restart the gateway after changing a variable.

The safe initial deployment collects analysis facts but does not show the admin page. Give platform admins access to calibration data only after you confirm the retention and access policy for the deployment.

## Local Development

`mise run dev-stack` refreshes the local demo request and agent-session fixtures, gives platform admins access to calibration data, and starts the report worker. Sign in as the seeded platform admin and open **Observability → Agent Sessions**. The worker processes the queued demo reports. The demo includes a ten-request Jira release session with file operations, six available Jira tools, and two direct MCP calls. It also includes a ten-request repository session with eight read, write, or edit calls. Set `AGENT_ANALYSIS_SHADOW_DIAGNOSTICS_ENABLED=false` before you start the stack to test the disabled page.

`mise run gateway-seed-local-demo` seeds the same finalized sessions, direct MCP invocation records, and queued analyses. It does not start the report worker or change access settings. Start the gateway with calibration data access or calibrated score access to show the page and process the queue. Restart the gateway after you pull backend changes. Frontend hot reload does not replace the Rust process. Use `mise run gateway-reset-local-demo` to replace old demo reports with the current report schema.

## Admin Workflow

When calibration data access or calibrated score access is enabled, open **Observability → Agent Sessions** at `/admin/observability/agent-sessions`.

The session explorer supports:

- server-side pagination capped at 200 sessions per page;
- session state, score confidence, user, service-account, harness, model, operation, caller-class, outcome, score-status, coverage, normalized-session, request-tag, and start-date filters;
- URL-backed filters and `session_id` selection, so refresh, back/forward navigation, and shared links preserve the selected session;
- a dense session table with score confidence, cost, active time, request, tool-call, direct-MCP-call, and data-quality values;
- a detail sheet with score components, outcome data, comparison-group data, coverage, analysis identity, formula versions, requests, and detected activity.

In calibration mode, the UI shows **Score not shown** instead of the session score. Platform admins can review the score components and calibration data before the score becomes available.

## Scope and Authorization

- Platform admins retain access to all existing control-plane routes.
- Calibration data is available only to platform admins.
- Team Owners/Admins receive session access only when both calibrated-score and team-admin capabilities are enabled.
- Team scope is enforced by the gateway for both list and detail requests; a list filter never grants detail access.
- Team Members and inactive users are denied.

The authenticated session response exposes the gateway's effective capabilities. The UI uses those values for route guards and navigation; it does not infer access from a browser-only environment variable.

## How Agent Sessions Are Formed

Each displayed Agent Session uses an internal session window. Oceans first uses authenticated session data from supported harnesses. It normalizes known session headers and permitted request metadata. It stores this data only in the authenticated owner and harness scope. A payload policy can prevent the system from using body metadata. If a request does not contain sufficient session data, Oceans groups it in an owner-specific and harness-specific window. Oceans does not create an external session identifier.

Oceans reuses an open session only when the owner, harness, request tags, and observed session are compatible. A model, operation, or caller change does not start a new session. The grouping key does not contain prompt or response content. Session analysis runs after request logging and does not delay the model response. Oceans finalizes an idle session after a versioned 30-minute gap and queues it for analysis. The session record keeps a successful, failed, or unknown request outcome after request logs expire. New data makes an older report out of date and queues a new report.

## Read Session Data

- **Request outcome** uses the final HTTP request results. An unknown outcome stays unknown.
- **Normalized cost** separates fresh input, cache reads, cache writes, and output when provider data and prices are available.
- **Active time** combines overlapping request periods and includes permitted short gaps. It is different from elapsed time.
- **Score confidence** describes the quality of the available data. It does not describe the quality of the answer.
- **Data coverage** shows which request and response data was available and whether response data was incomplete.
- **Detected activity** contains retained request-level classifications. Each activity record keeps the parser version that created it. Request history and detected activity are each limited to 1,000 records. The UI tells you when it does not show all records.
- **Comparison group** selects earlier successful sessions that use compatible analysis versions. The selection starts with the most specific group and then uses broader harness, model, operation, and caller groups. Each group requires at least six sessions. The report shows the selected group, the number of sessions, and the snapshot identifier. The system does not show a score when no comparison group is available.
- **Data limits** identify missing or incomplete data. The system does not replace missing values with zero or a neutral value.
- **Analysis versions** identify the report, analyzer, score policy, pricing policy, observation parser, session-boundary policy, input time, detected-activity set, and comparison snapshot.

A failed request outcome gives an outcome score of zero. An unknown outcome uses the documented fixed prior. Therefore, zero and **Not available** have different meanings.

The session score combines the request outcome, cost rank in the comparison group, and active-time rank in the comparison group. The nominal weights are 0.5, 0.3, and 0.2. If cost or active time is not available, the formula adjusts the remaining weights. The score components and data limits show which data contributed to the score.

## Recompute Retained Sessions

Queue up to 100 retained finalized sessions whose latest report is absent or stale:

```bash
mise run gateway-recompute-agent-analysis
mise run gateway-recompute-agent-analysis-prod
```

Set `LIMIT` to a value from `1` through `1000` to bound one run. To target one retained session directly, run:

```bash
mise exec -- cargo run -p gateway --bin gateway -- recompute-agent-analysis --session-id <UUID> --limit 1
```

The command prints separate `matched_count` and `enqueued_count` values. It is idempotent for the same session watermark and desired version tuple, and it only queues work; the normal gateway analysis worker produces reports.

## Privacy and Retention

Default analysis data does not store prompts, responses, tool arguments, tool results, source code, file contents, file paths, hosts, IP addresses, or arbitrary request headers. Detected file and tool activity keeps only bounded classifications and opaque identifiers. The request payload policy controls whether the system has enough permitted metadata to detect optional activity.

Request links and detected activity use the request-log retention period. A minimal session record remains while a report refers to it. Reports use a separate retention period, which is 90 days by default. Oceans deletes reports when it deletes their owner. Completed or failed queue records use a seven-day retention period by default. An hourly process applies both retention periods, even when session analysis is disabled.

## Current Limits

- The system does not show the session score until a deployment completes session-grouping review, score-sensitivity analysis, comparison-group validation, and pricing-discrepancy review.
- Aggregate monitoring is not available before this calibration is complete.
- Provider cache fields can be missing or can use different meanings. The UI shows the related data coverage and data limit instead of estimating a value.
- Session grouping is an operational estimate. It does not identify a user or prove that requests belong to the same conversation.
