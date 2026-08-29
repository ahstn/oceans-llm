# Agent Harness Usage

`See also`: [Agent Session Analysis](agent-session-analysis.md), [Observability and Request Logs](observability-and-request-logs.md), [Data Relationships](../contributing/reference/data-relationships.md), [Admin Control Plane](../access/admin-control-plane.md)

Implementation references: [`request_logging.rs`](../../crates/gateway-service/src/request_logging.rs), [`observability.rs`](../../crates/gateway/src/http/observability.rs), [`agent-harnesses.tsx`](../../crates/admin-ui/web/src/routes/observability/agent-harnesses.tsx)

Agent harness usage is an admin observability surface for understanding which coding-agent clients self-report data-plane requests through the gateway. It is derived from inbound HTTP `User-Agent` headers, so it is operational classification evidence, not authenticated client identity or abuse attribution.

## What Gets Stored

For each persisted request-log row, the gateway stores:

- `user_agent_raw`: the inbound HTTP `User-Agent` value capped at 512 characters, or `null` when the header is missing or empty
- `agent_harness_key`: a stable low-cardinality key used for grouping
- `agent_harness_label`: the display label shown in admin surfaces

The bounded raw `User-Agent` is preserved for request-log debugging and future reclassification. It is not used as a metric label and is not used for chart grouping.

Agent task analysis may use the normalized harness key plus bounded, allowlisted request metadata as correlation evidence. `User-Agent` alone never establishes a session or identity. Raw `User-Agent` values remain request-log debugging data and are not copied into analysis observations or score reports.

## Classifier Contract

The classifier is explicit and conservative. Known coding-agent patterns map to stable keys:

| Raw signal | Key | Label |
| --- | --- | --- |
| `opencode/...`, `Agent/opencode` | `opencode` | Opencode |
| `pi/...` with platform/runtime metadata | `pi` | Pi |
| `claude-code/...`, `Claude-User (claude-code/...)`, `Agent/claude-code` | `claude_code` | Claude Code |
| `GeminiCLI/...`, `GeminiCLI-.../...`, `CloudCodeVSCode/...`, `Agent/gemini-cli` | `gemini_cli` | Gemini CLI |
| `Agent/copilot-cli` | `copilot_cli` | Copilot CLI |
| `GithubCopilot/...`, `GitHubCopilot/...` | `github_copilot` | GitHub Copilot |

Missing, empty, generic, or unmatched values map to:

- key: `unknown`
- label: `Unknown`

Generic runtime values such as `undici` remain `Unknown` unless a more specific signal is present.

## Admin API

Platform admins can query harness usage with:

- `GET /api/v1/admin/observability/harness-usage?range=7d|31d`

The endpoint uses the same range picker semantics as the usage leaderboard:

- default range is `7d`
- accepted ranges are `7d` and `31d`
- invalid ranges return the same validation style as the leaderboard endpoint
- time-series buckets are 12-hour UTC buckets
- the chart cohort is the top five harnesses by request count
- the table returns the top thirty harnesses by request count

Aggregation groups by `agent_harness_key`, not by `user_agent_raw`, so versioned user-agent strings do not fragment the report.

## Admin UI

The admin UI exposes the page at:

- `/admin/observability/agent-harnesses`

The page shows self-reported `User-Agent` classifications:

- a 7-day and 31-day range picker
- a request-count time-series chart for the top harnesses
- a ranked table of normalized harness usage

Request-log detail also shows the normalized harness label and raw `User-Agent` value for debugging classifier behavior.

## Shell guardrails

The first-party Pi and OpenCode harness adapters install a local pre-tool hook. Immediately before a `bash` tool starts a process, the hook sends an authenticated `POST /api/v1/guardrails/evaluate` request with `tool_name`, the command, and optional structured arguments.

An `allow` or `audit` response permits execution. The adapter retains the returned decision ID with the harness result. A `deny` response throws before the process runner is called. Network and invalid-response failures also stop execution so a missing policy decision cannot become a local bypass.

The hook does not contain a second copy of the built-in rules. The gateway remains the policy authority. Hook errors and logs contain stable decision or reason codes, not credentials or full sensitive command payloads. See [Gateway Guardrails](gateway-guardrails.md) for policy configuration and rollout.
