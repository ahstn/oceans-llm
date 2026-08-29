# Observability and Request Logs

`See also`: [Export Traces and Metrics](observability/export-traces-and-metrics.md), [Request Logs](observability/request-logs.md), [MCP Invocations](../mcp/mcp-invocations.md), [MCP Registry and Discovery](../contributing/mcp/mcp-registry-and-discovery.md), [Tagging](tagging.md), [Agent Harness Usage](agent-harness-usage.md), [Data Relationships](../contributing/reference/data-relationships.md), [Service Accounts](../access/service-accounts.md), [Model Routing and API Behavior](../configuration/model-routing-and-api-behavior.md), [Provider API Compatibility](../reference/provider-api-compatibility.md), [Request Lifecycle and Failure Modes](../reference/request-lifecycle-and-failure-modes.md), [Admin Control Plane](../access/admin-control-plane.md), [Deploy and Operations](../setup/deploy-and-operations.md), [ADR: Team Service Accounts for Non-Human Gateway Access](../adr/2026-05-10-team-service-accounts.md), [ADR: OTLP-First Observability and Payload-Backed Request Logs](../adr/2026-03-15-otlp-observability-and-request-log-payloads.md), [ADR: Route-Level Provider API Compatibility Profiles](../adr/2026-04-23-route-level-provider-api-compatibility-profiles.md), [ADR: MCP Tool Cardinality Observability](../adr/2026-04-28-mcp-tool-cardinality-observability.md), [ADR: External MCP Registry and Discovery Boundary](../adr/2026-05-26-external-mcp-registry-and-discovery.md)

This document describes the live observability contract for the gateway.

## Observability Pages

- [Request Logs](observability/request-logs.md): request-scoped admin list/detail, payload policy state, provider attempts, and request-level tool cardinality.
- [MCP Invocations](../mcp/mcp-invocations.md): per-tool MCP audit rows, request correlation, owner context, authorization policy result, latency, and redacted argument/result metadata.
- [MCP Registry and Discovery](../contributing/mcp/mcp-registry-and-discovery.md): platform-admin registry management, recommended catalog import flow, Streamable HTTP discovery, auth declarations, and stable MCP server/tool ids.

## Source of Truth

- observability bootstrap:
  - [../crates/gateway/src/observability.rs](../../crates/gateway/src/observability.rs)
- HTTP request instrumentation:
  - [../crates/gateway/src/http/handlers.rs](../../crates/gateway/src/http/handlers.rs)
- request-log lifecycle:
  - [../crates/gateway-service/src/request_logging.rs](../../crates/gateway-service/src/request_logging.rs)
- request-attempt persistence:
  - [../crates/gateway-store/migrations/V19__request_log_attempts.sql](../../crates/gateway-store/migrations/V19__request_log_attempts.sql)
- redaction policy:
  - [../crates/gateway-service/src/redaction.rs](../../crates/gateway-service/src/redaction.rs)
- admin APIs:
  - [../crates/gateway/src/http/observability.rs](../../crates/gateway/src/http/observability.rs)
  - [../crates/gateway/src/http/mcp_registry.rs](../../crates/gateway/src/http/mcp_registry.rs)

## OTLP-First Model

The gateway exports tracing spans and metrics through OpenTelemetry.

Current config knobs:

- `server.otel_endpoint`
- `server.otel_metrics_endpoint`
- `server.otel_trace_sample_ratio`
- `server.otel_export_interval_secs`

`server.otel_trace_sample_ratio` defaults to `1.0` and accepts values from `0.0` through `1.0`. It uses parent-based sampling for traces and does not sample exported metrics. The intended deploy path is collector-friendly OTLP export rather than an in-process Prometheus endpoint.

Use [Export Traces and Metrics](observability/export-traces-and-metrics.md) to configure an OpenTelemetry Collector or Datadog Agent. The guide covers Helm wiring, shared-Agent sampling, and export checks.

## What Gets Recorded

The runtime emits bounded request-level signals for:

- API request totals
- request latency
- request outcomes
- token totals
- priced spend metric totals
- usage-record totals by pricing status
- request tool-cardinality histograms
- caller request tags for filtering and attribution; see [Tagging](tagging.md)

Request correlation is anchored on `x-request-id`. The HTTP middleware boundary owns request-id generation and propagation: caller-provided values are preserved, and missing values are generated once before handlers run.

Request outcomes are emitted once per request with bounded labels. Important examples in this slice are:

- `budget_error` for pre-provider hard-limit rejection
- `invalid_request` for capability mismatch
- `upstream_error` for upstream execution or stream failure

## Tagging and Attribution

The request-log surface records caller-supplied request tags for filtering and attribution. The tag header contract, validation rules, examples, and identity tag guidance live in [Tagging](tagging.md).

Request tags are request-scoped. User and team tags are durable identity metadata managed by admins. Keep those two surfaces distinct when exporting or reconciling observability data.

## Request Log Storage Shape

Request logs are intentionally split:

- `request_logs`
  - hot summary row
- `request_log_payloads`
  - sanitized request and response bodies
- `request_log_tags`
  - bounded bespoke caller tags
- `request_log_attempts`
  - ordered upstream provider execution attempts

The summary row stores:

- request identity
- owner identity
- service-account identity for non-human callers
- requested and resolved model identity
- provider key
- universal caller tags
- status, latency, and usage totals
- typed MCP and tool cardinality counts
- bounded raw `User-Agent` and normalized agent harness key/label
- truncation flags
- metadata such as `operation`, `stream`, and `payload_policy`

`operation` is the public API family. Current values include `chat_completions`, `responses`, and `embeddings`.

Request-attempt rows describe upstream provider execution only. Pre-provider failures such as authentication rejection, capability mismatch, route unavailability, or budget hard-limit rejection have zero attempts. In the current runtime, successful provider-backed requests record one terminal attempt. Retry and fallback execution remain disabled until the configurable policy tracked in issue #118 is implemented.

Native Vertex embeddings use the same request-log surfaces. Successful or failed provider-backed embedding requests record `operation: embeddings`; provider execution details appear as request-attempt rows when a request-log summary is written; sanitized request and response payloads follow the configured payload policy. Embedding inputs are text-only for native Vertex routes, but the redaction and byte-limit policy still applies before storage.

Tool-cardinality fields are explicit nullable columns on `request_logs`.

- `exposed_tool_count`: shallow count of OpenAI-compatible request tools.
- `invoked_tool_count`: count of tool-call artifacts observed in normalized provider output.
- `referenced_mcp_server_count`: nullable until an MCP access/filtering layer records server exposure.
- `filtered_tool_count`: nullable until an MCP access/filtering layer records filtered or denied tools.

New Chat Completions and Responses rows record `0` for exposed and invoked counts when no tools are present. Historical rows and unavailable MCP-specific dimensions remain `null`. Admin surfaces render `null` as `n/a` and preserve real zeroes.

Streaming requests persist a bounded transcript payload rather than raw transport bytes.

The stream payload contract is incremental rather than chunk-local:

- UTF-8 is reassembled across transport chunk boundaries
- SSE `data:` frames are reassembled across chunk boundaries
- both `data:` and `data: ` forms are accepted
- the latest coherent `usage` object is retained for request-log and ledger work
- Responses streams also retain usage from `response.usage` on completed response events
- streaming tool-call artifacts increment `invoked_tool_count` while SSE frames are parsed for request logging

Request-log payloads are user-visible artifacts. They do not persist the transformed outbound provider request body produced by route compatibility profiles.

Provider stream transcripts can include normalized compatibility output, such as promoted usage or canonical reasoning deltas, because that normalized stream is what the gateway returns to callers. Responses streams preserve `response.*` event names and payloads rather than being rewritten into Chat Completions chunks.

## Request Log Retention and Purge

Request-log retention is admin-controlled. The supported retention windows are intentionally small and explicit:

- `1d`
- `3d`
- `7d`

The default retention window is `7d`. Admins can run the purge command manually before enabling any recurring cleanup:

```bash
mise run gateway-purge-request-logs-dry-run
mise run gateway-purge-request-logs
```

Use `--dry-run` first in production-shaped environments. A dry run reports how many parent request-log rows are older than the selected retention cutoff without deleting data.

When the command runs without `--dry-run`, it deletes matching `request_logs` rows and their request-log children:

- `request_log_payloads`
- `request_log_tags`
- `request_log_attempts`

Admins should not hand-delete only one request-log table. Manual partial deletion can leave observability detail misleading even when database constraints prevent direct orphan rows.

Recurring purge is disabled by default and must be opted into from config. Use a standard cron expression and keep the schedule daily or less frequent:

```yaml
request_logging:
  purge:
    enabled: false
    retention: 7d
    schedule: "0 0 * * *"
```

Runtime safety rules:

- `enabled` defaults to `false`
- `retention` defaults to `7d`
- only `1d`, `3d`, and `7d` are valid windows
- `schedule` uses standard 5-field cron syntax
- recurring schedules must not be more frequent than daily
- each gateway process starts its own recurring worker when enabled
- the runtime keeps a UTC-day guard so a recurring worker cannot purge more than once per day even if a bad schedule is supplied

Retention only affects operational request-log tables. It does not delete spend ledger rows in `usage_cost_events`, budget history, provider config, model config, users, teams, or API keys.

## Payload Policy

Chat-completion request-log payload persistence is controlled by `request_logging.payloads` in `gateway.yaml`.

Default config:

```yaml
request_logging:
  payloads:
    capture_mode: redacted_payloads
    request_max_bytes: 131072
    response_max_bytes: 65536
    stream_max_events: 128
    redaction_paths: []
```

Capture modes:

- `disabled`: skip request-log persistence for chat completions
- `summary_only`: write `request_logs` summary rows with `has_payload=false`; do not write `request_log_payloads`
- `redacted_payloads`: write summary rows and sanitized payload rows

The policy is read from YAML only. The admin UI displays the policy used for each row, but does not edit it.

Owner behavior also matters:

- user-owned API keys honor `users.request_logging_enabled`
- service-account credentials always persist request-log summary rows

This is why a user-owned request can be absent from request logs while a service-account request with the same payload policy is still visible.

Validation rules:

- `request_max_bytes` must be greater than zero
- `request_max_bytes` must not exceed `262144`, the absolute inline request ceiling
- `response_max_bytes` must be greater than zero
- `stream_max_events` must be greater than zero
- `redaction_paths` must use dot-separated object keys, with `*` as a full-segment wildcard
- paths are anchored from the wrapped payload root, for example `body.messages.*.content.*.image_url.url`

Each request-log row persists lightweight policy metadata in `request_logs.metadata_json`:

```json
{
  "operation": "chat_completions",
  "stream": false,
  "payload_policy": {
    "capture_mode": "redacted_payloads",
    "request_max_bytes": 131072,
    "response_max_bytes": 65536,
    "stream_max_events": 128,
    "version": "builtin:v2"
  }
}
```

## Redaction and Truncation Boundaries

Payloads are wrapped before policy application:

- requests: `{ "headers": ..., "body": ... }`
- responses: `{ "body": ... }`
- streams: `{ "stream": true, "events": ..., "usage": ..., "error": ... }`

Redaction applies one explicit built-in policy plus additive admin-configured paths from `request_logging.payloads.redaction_paths`.

Stored request headers use an explicit diagnostic allow-list. The gateway keeps `session-id`, `session_id`, `thread-id`, `x-claude-code-agent-id`, `x-claude-code-parent-agent-id`, `x-claude-code-session-id`, `x-client-request-id`, `x-codex-turn-metadata`, `x-opencode-session`, `x-parent-session-id`, `x-session-affinity`, and `x-session-id`. It drops all other headers from the stored payload, including unknown credential headers. Within `x-codex-turn-metadata`, it keeps only string-valued session and lineage fields used by Agent Session Analysis.

Sensitive built-in JSON keys include:

- `token`
- `access_token`
- `refresh_token`
- `api_key`
- `anthropic_api_key`
- `client_secret`
- `credentials`
- `private_key`
- `secret`
- `password`

Built-in URL query redaction preserves the scheme, authority, and path while replacing query components in retained media URL fields and HTTPS URLs echoed in retained error messages.

Known bulky provider fields are shape-preserving truncated before the whole-payload byte budget is applied. Built-ins cover OpenAI-compatible image/audio/file payloads, Vertex Gemini inline data, and Vertex Anthropic base64 source data.

Request budgets use the uncompressed serialized JSON byte count. Database compression does not reduce gateway serialization work, API transfer size, JSON parsing work, or admin UI rendering work. The default persisted-request cap is 128 KiB, and 256 KiB is the absolute inline ceiling even for policies constructed outside YAML validation. These are gateway engineering levels, not database limits:

- at or below 64 KiB is the preferred interactive and debugging range, with an operational P95 target at or below this level
- 64-128 KiB is accepted and uses structured content budgeting when the configured cap requires it
- 128-256 KiB is exceptional and must be reduced to the configured persisted cap
- above 256 KiB is never retained inline in full

These levels are an evidence-based gateway design inference, not a limit copied from one vendor. [PostgreSQL TOAST](https://www.postgresql.org/docs/current/storage-toast.html) and [SQLite limits](https://www.sqlite.org/limits.html) show why database capacity is not a useful operational target. [Datadog](https://docs.datadoghq.com/logs/log_collection/) recommends much smaller individual logs than its maximum. [Google Cloud Logging](https://docs.cloud.google.com/logging/quotas), [CloudWatch Logs](https://docs.aws.amazon.com/AmazonCloudWatch/latest/logs/cloudwatch_limits_cwl.html), and [Sentry](https://develop.sentry.dev/sdk/foundations/envelopes/event-payloads/) use lower limits for structured or interactive paths. The [OpenTelemetry log data model](https://opentelemetry.io/docs/specs/otel/logs/data-model/) also requires structured semantics while accounting for serialization cost and space.

The request remains one bounded JSON value. This policy does not add split payload columns, a tool-schema table, or tool-schema deduplication.

Request processing uses a structured analysis projection and a separate storage representation:

1. wrap the request and apply built-in and admin-configured redaction rules
2. extract Agent Session Analysis metadata from the structured redacted request, including permitted session headers, original prompt size, reasoning settings, and supplied-tool facts
3. truncate known bulky binary fields while preserving JSON shape
4. count the serialized size and return without adaptive work when the request is within `request_max_bytes`
5. reserve the diagnostic envelope before allocating input-content bytes; it retains sanitized session and lineage headers, model and reasoning configuration, tool names and choice, bounded tool schema shape, stream and include settings, cache keys, metadata, and message or item identity fields
6. aim for a 16 KiB essential envelope, with 32 KiB as a soft limit; when an oversize request has an envelope above the target, compact verbose tool and schema descriptions, examples, and defaults while retaining essential identifiers, tool names, and schema shape
7. allocate at most 96 KiB in total to retained input content; ordinary messages use an adaptive target of 8-16 KiB each, while a solitary message can retain up to 32 KiB when the total request budget permits
8. for Chat requests, bound `body.messages[*].content` text leaves; for Responses requests, bound a string-valued `body.instructions`, a string-valued `body.input`, or the text leaves under `body.input[*].content`; preserve every message, item, content array, unknown item type, and non-bulky field
9. truncate text-bearing leaves independently with a UTF-8-safe head and tail plus an explicit omitted-byte marker, then enforce the final serialized request cap
10. use the complete-payload `{ truncated, size_bytes, preview }` marker only when the bounded essential envelope cannot fit

Structured request truncation adds a top-level `truncation` object beside `headers` and `body`. It records the strategy version, original and stored serialized sizes, omitted bytes, truncated-field count, a bounded affected-path list, known-large-field count, and tool-field compaction count. `request_payload_truncated=true` means that at least one request field was truncated or the hard fallback was required. Analysis metadata does not depend on this stored representation, so storage truncation does not remove an observed external session source or reduce the original prompt and tool facts used by Agent Session Analysis.

Response storage keeps the existing 64 KiB default and behavior: known bulky fields are reduced first, then `response_max_bytes` applies the complete-payload hard guardrail. The existing `response_payload_truncated=true` signal records a response that hit the whole-response or stream storage limit. Agent session reports count affected responses, and the admin UI shows that count separately from permanent analysis capability notices.

The adaptive request path mutates the already redacted storage value and uses `serde_json::to_writer` with a counting writer when it only needs a size. It does not clone the full JSON tree again. The manual measurement harness covers 8 KiB, 64 KiB, 256 KiB, and 1 MiB requests with long messages, many small messages, tool-heavy envelopes, content arrays, binary blocks, and multibyte UTF-8. Run it with `mise exec -- cargo test -p gateway-service measures_payload_helper_and_request_setup_matrix -- --ignored --nocapture`. The harness reports uncompressed input and stored sizes, the stress-fixture P95 stored size, the pure bounding-helper time, and complete gateway request-setup time. The equal-weight stress matrix is not a production traffic distribution, so its P95 is a guard against the 128 KiB normal cap rather than proof of the operational 64 KiB P95 target. It does not enforce a machine-dependent latency threshold.

For streams, the gateway keeps parsing every frame for usage and provider errors. Only stored event payloads are capped by `stream_max_events`; if the cap is hit, `response_payload_truncated=true`.

## Recent Contract Cleanup

Recent cleanup changed the contract in a few important ways.

- fallback-era request metadata is gone
- provider execution attempts now live in `request_log_attempts` instead of summary metadata
- missing request-log detail rows return strict `404 not_found`
- stream payload parsing is more boundary-safe than the earlier chunk-by-chunk behavior
- budget-rejected chat requests record a `budget_error` request outcome without executing the provider

Admins and maintainers should stop expecting:

- fallback metadata columns to appear in new request rows
- nullable detail lookups for missing rows

## Admin Observability APIs

Platform admins can inspect request logs through:

- `GET /api/v1/admin/observability/leaderboard`
- `GET /api/v1/admin/observability/harness-usage`
- `GET /api/v1/admin/observability/request-logs`
- `GET /api/v1/admin/observability/request-logs/{request_log_id}`

Request-log list and detail responses include the row metadata, so admins can see the public operation for each row, such as `chat_completions`, `responses`, or `embeddings`, alongside the typed payload policy. They also include truncation fields.

The MCP invocation admin UI consumes these generated admin API endpoints:

- `GET /api/v1/admin/observability/mcp-invocations`
- `GET /api/v1/admin/observability/mcp-invocations/{mcp_tool_invocation_id}`

Validate documentation-only edits to this page with `mise run //docs:build` before handoff.

## Usage Leaderboard

The leaderboard is a separate admin observability surface from spend reporting.

Endpoint:

- `GET /api/v1/admin/observability/leaderboard?range=7d|31d`

Current semantics:

- ranked by total spend over the selected range
- ties sort by request count, then user name
- chart cohort is the top 5 ranked users
- table is the top 30 ranked users
- per-user tool-cardinality averages use only rows where each dimension was recorded, so historical nulls do not dilute averages
- time buckets are 12-hour UTC buckets and are zero-filled for chart stability
- dominant model is chosen by request count, then spend, then model key

Use the leaderboard to identify recent high-usage users. Use spend reporting when the question is about owner totals, budgets, or pricing status counts.

## Agent Harness Usage

Harness usage is a separate admin observability surface from the user leaderboard.

Endpoint:

- `GET /api/v1/admin/observability/harness-usage?range=7d|31d`

Current semantics:

- ranked by request count over the selected range
- chart cohort is the top 5 ranked harnesses
- table is the top 30 ranked harnesses
- time buckets are 12-hour UTC buckets and are zero-filled for chart stability
- aggregation groups by `agent_harness_key`, not raw `User-Agent`
- bounded raw `User-Agent` values remain available in request-log detail for debugging
- harness classification is self-reported from `User-Agent` and is not authenticated client identity

Use [Agent Harness Usage](agent-harness-usage.md) for the classifier contract and page behavior.

Request-log list filters:

- `page`
- `page_size`
- `request_id`
- `model_key`
- `provider_key`
- `status_code`
- `user_id`
- `team_id`
- `service`
- `component`
- `env`
- `tag_key`
- `tag_value`

## Current Gaps

- deploy examples do not ship an OTLP collector by default

## Relationship to Spend Reporting

Request logs and spend accounting are related, but intentionally separate.

- request logs describe the user-visible request outcome
- `usage_cost_events` is the canonical spend ledger

For the full request path across both systems, use [request-lifecycle-and-failure-modes.md](../reference/request-lifecycle-and-failure-modes.md).
