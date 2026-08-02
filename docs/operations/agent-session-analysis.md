# Agent Session Analysis

`See also`: [Admin Control Plane](../access/admin-control-plane.md), [Agent Harness Usage](agent-harness-usage.md), [Request Logs](observability/request-logs.md), [Pricing Catalog and Accounting](../configuration/pricing-catalog-and-accounting.md)

Oceans groups related gateway requests into agent session windows. It calculates request outcome, normalized cost, active request time, confidence, data coverage, comparison position, and formula-version data. It does not read or verify the quality of an agent's answer.

## Availability

Use `agent_analysis` in `gateway.yaml`. It controls collection, access, retention, and metric groups:

```yaml
agent_analysis:
  enabled: true
  shadow_diagnostics_enabled: false
  calibrated_score_enabled: false
  calibration_approval_id: null
  team_admin_enabled: false
  report_retention_days: 90
  queue_retention_days: 7
  context_input_boundary_tokens: 220000
  context_reserved_output_tokens: 128000
  context_penalty_points_per_repeated_excess: 2
  metrics:
    tokens: true
    cache: true
    context: true
    tools: true
    skills: true
    reliability: true
    outcomes: true
    finish_reasons: true
  cache_profiles: []
```

A safe first deployment collects facts. It hides the admin page. Enable `shadow_diagnostics_enabled` after platform admins approve access and retention. A calibrated score needs two fields. Set `calibrated_score_enabled` to true. Add a non-empty `calibration_approval_id`. Team admin access also needs calibrated scores.

Each metric switch controls one report section. A disabled section says **Disabled**. A section with no data says **Unknown**. Missing data never becomes zero. Metric, context, and cache-profile changes create a new report version. Recompute retained sessions against that version.

Older `AGENT_ANALYSIS_*` environment variables can override some fields. They cover access, collection, approval, and retention. Invalid boolean values stop startup. Use YAML as the main source for new deployments. Restart the gateway after any change.

## Local Development

`mise run dev-stack` refreshes the local demo request and agent-session fixtures, gives platform admins access to calibration data, and starts the report worker. Sign in as the seeded platform admin and open **Observability → Agent Sessions**. The worker processes the queued demo reports. The demo includes a ten-request Jira release session with file operations, six available Jira tools, and two direct MCP calls. It also includes a ten-request repository session with eight read, write, or edit calls. Set `AGENT_ANALYSIS_SHADOW_DIAGNOSTICS_ENABLED=false` before you start the stack to test the disabled page.

`mise run gateway-seed-local-demo` seeds the same finalized sessions, direct MCP invocation records, and queued analyses. It does not start the report worker or change access settings. Start the gateway with calibration data access or calibrated score access to show the page and process the queue. Restart the gateway after you pull backend changes. Frontend hot reload does not replace the Rust process. Use `mise run gateway-reset-local-demo` to replace old demo reports with the current report schema.

## Admin Workflow

Enable calibration data access or calibrated score access. Then open **Observability → Agent Sessions** at `/admin/observability/agent-sessions`.

The session explorer supports:

- It uses server-side pagination. Each page holds up to 200 sessions.
- It filters by session state, confidence, owner, harness, model, operation, caller, outcome, score status, coverage, session ID, request tag, and start date.
- Its URL stores filters and the selected `session_id`. Refresh, browser history, and shared links keep the same view.
- Its dense table shows confidence, cost, active time, requests, tool calls, direct MCP calls, and data quality.
- Its detail sheet shows score parts, outcomes, comparison data, coverage, analysis identity, formula versions, requests, and detected activity.

In calibration mode, the UI shows **Score not shown** instead of the session score. Platform admins can review the score components and calibration data before the score becomes available.

## Scope and Authorization

- Platform admins retain access to all existing control-plane routes.
- Calibration data is available only to platform admins.
- Team Owners/Admins receive session access only when both calibrated-score and team-admin capabilities are enabled.
- Team scope is enforced by the gateway for both list and detail requests. A list filter never grants detail access.
- Team Members and inactive users are denied.

The authenticated session response lists the gateway's active capabilities. The UI uses them for route guards and navigation. It does not infer access from a browser-only environment variable.

## How Agent Sessions Are Formed

Each displayed Agent Session uses an internal session window. Oceans first uses authenticated session data from supported harnesses. It normalizes known session headers and permitted request metadata. It stores this data only in the authenticated owner and harness scope. A payload policy can prevent the system from using body metadata. If a request does not contain sufficient session data, Oceans groups it in an owner-specific and harness-specific window. Oceans does not create an external session identifier.

Oceans reuses an open session only when the owner, harness, request tags, and observed session are compatible. A model, operation, or caller change does not start a new session. The grouping key does not contain prompt or response content. Session analysis runs after request logging and does not delay the model response. Oceans finalizes an idle session after a versioned 30-minute gap and queues it for analysis. The session record keeps a successful, failed, or unknown request outcome after request logs expire. New data makes an older report out of date and queues a new report.

## Read Session Data

- **Request outcome** uses the final HTTP request results. An unknown outcome stays unknown.
- **Normalized cost** separates fresh input, cache reads, cache writes, and output when provider data and prices are available.
- **Active time** combines overlapping request periods and includes permitted short gaps. It is different from elapsed time.
- **Score confidence** describes the quality of the available data. It does not describe the quality of the answer.
- **Data coverage** shows which request and response data was available and whether response data was incomplete.
- **Detected activity** contains retained request-level classifications. Each activity record keeps the parser version that created it. Request history and detected activity are each limited to 1,000 records. Nested tool, skill, and file facts are additionally limited to 2,048 per session trace. The UI tells you when it does not show all records.
- **Comparison group** selects earlier successful sessions that use compatible analysis versions. The selection starts with the most specific group and then uses broader harness, model, operation, and caller groups. Each group requires at least six sessions. The report shows the selected group, the number of sessions, and the snapshot identifier. The system does not show a score when no comparison group is available.
- **Data limits** identify missing or incomplete data. The system does not replace missing values with zero or a neutral value.
- **Analysis versions** identify the report, analyzer, score policy, pricing policy, observation parser, session-boundary policy, input time, detected-activity set, and comparison snapshot.

A failed request outcome gives an outcome score of zero. An unknown outcome uses the documented fixed prior. Therefore, zero and **Not available** have different meanings.

The session score combines the request outcome, cost rank, and active-time rank. Their nominal weights are 0.5, 0.3, and 0.2. If cost or time is missing, the formula adjusts the other weights. The score parts and data limits show which data contributed.

## Read Diagnostic Groups

The detail sheet lists request events first. A request can show route attempts and model changes. It can also show finish reasons, tool or skill activity, and opaque file activity. Use its link to open the matching request log.

### Tokens and cache

- **Total input** adds fresh input, cache reads, and cache creation. Some providers report cached tokens outside their normal input field. The full sum avoids rewarding that split.
- **Visible output** removes reasoning tokens only when output includes them. The report never subtracts reasoning twice.
- **Cache read to write ratio** compares reads with all input.
- **Cache write amplification** compares cache creation with cache reads. A high value can mean many writes with little reuse.
- **Cache lifetime buckets** split five-minute, 30-minute, and one-hour writes. Provider/model profiles set the minimum cacheable prefix and default lifetime. They do not invent token counts.
- **Threshold misses** count requests that asked for caching but stayed below the provider minimum. The request must also show no cache activity.

Provider rules differ. Anthropic supports explicit five-minute and one-hour cache lifetimes. It also sets model-specific minimum prefixes. OpenAI applies automatic prefix caching above its minimum. It reports cached input tokens. Amazon Bedrock reports cache reads and writes as extra input buckets. Check the current [Anthropic prompt-caching guide](https://platform.claude.com/docs/en/build-with-claude/prompt-caching), [OpenAI prompt-caching guide](https://platform.openai.com/docs/guides/prompt-caching), and [Amazon Bedrock prompt-caching guide](https://docs.aws.amazon.com/bedrock/latest/userguide/prompt-caching.html) before changing a cache profile.

### Context health

The report shows peak input use against the set input boundary. It also shows growth per turn and active minute. Other values cover possible compactions and repeat requests above the boundary. A warning is not a claim about answer quality. Long context can reduce retrieval and reasoning quality even when a model accepts the request. See [Chroma's Context Rot evaluation](https://github.com/chroma-core/context-rot).

### Reliability and tools

- **Wasted attempts** did not produce the final response. The report includes their measured latency. Cost stays unknown until providers supply usage for each attempt.
- **Fallback attempts** count provider or model changes between attempts for one request.
- **Tool reliability** groups failures, truncation, latency, and input tokens used after a failure.
- **Tool servers** compare exposed and invoked tools for each server. They also estimate uncached schema cost from the active price policy. Exposed but unused tools can still consume input tokens.

### Skills and outcomes

Skill data splits always-loaded description tokens from selected bodies and resources. It can show a selected skill that was later abandoned. The report stores only bounded names, token estimates, and state flags. It does not store skill files.

Outcome evidence uses opaque file IDs. It shows cost per file, repeat edits, checks after writes, and failed file operations. It can also show a session with no detected write or check. These are operating signals. They do not prove that a change was correct.

### Finish reasons

Finish reasons split normal completion from length or output-limit stops. A length-limited request can use input and reasoning tokens but return little visible output. This is a diagnostic, not a model-quality judgment.

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

The command prints separate `matched_count` and `enqueued_count` values. It is idempotent for the same session watermark and desired version tuple. It only queues work. The normal gateway analysis worker produces reports.

## Privacy and Retention

Default analysis data does not store prompts, responses, tool arguments, tool results, source code, file contents, file paths, hosts, IP addresses, or arbitrary request headers. Optional diagnostics keep bounded tool and skill names, token estimates, attempt status, provider/model keys, finish reasons, operation classes, and opaque file identifiers. File identifiers are hashed with the owning user, team, or service-account scope before persistence, so the same input cannot be correlated across owners. Diagnostics do not keep skill bodies, schemas, request payloads, tool payloads, or a reversible path-derived identifier. The request payload policy controls whether the system has enough permitted metadata to detect optional activity.

Request links and detected activity use the request-log retention period. A minimal session record remains while a report refers to it. Reports use a separate retention period, which is 90 days by default. Oceans deletes reports when it deletes their owner. Completed or failed queue records use a seven-day retention period by default. An hourly process applies both retention periods, even when session analysis is disabled.

## Current Limits

- The system does not show the session score until a deployment completes session-grouping review, score-sensitivity analysis, comparison-group validation, and pricing-discrepancy review.
- Aggregate monitoring is not available before this calibration is complete.
- Provider cache fields can be missing or can use different meanings. The UI shows the related data coverage and data limit instead of estimating a value.
- Session grouping is an operational estimate. It does not identify a user or prove that requests belong to the same conversation.
