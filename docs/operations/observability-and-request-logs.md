# Observability and Request Logs

`See also`: [Data Relationships](../reference/data-relationships.md), [Model Routing and API Behavior](../configuration/model-routing-and-api-behavior.md), [Provider API Compatibility](../reference/provider-api-compatibility.md), [Request Lifecycle and Failure Modes](../reference/request-lifecycle-and-failure-modes.md), [Admin Control Plane](../access/admin-control-plane.md), [Deploy and Operations](../setup/deploy-and-operations.md), [ADR: OTLP-First Observability and Payload-Backed Request Logs](../adr/2026-03-15-otlp-observability-and-request-log-payloads.md), [ADR: Route-Level Provider API Compatibility Profiles](../adr/2026-04-23-route-level-provider-api-compatibility-profiles.md)

This document describes the live observability contract for the gateway.

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

## OTLP-First Model

The gateway exports tracing spans and metrics through OpenTelemetry.

Current config knobs:

- `server.otel_endpoint`
- `server.otel_metrics_endpoint`
- `server.otel_export_interval_secs`

The intended deploy path is collector-friendly OTLP export rather than an in-process Prometheus endpoint.

## What Gets Recorded

The runtime emits bounded request-level signals for:

- API request totals
- request latency
- request outcomes
- token totals
- priced spend metric totals
- usage-record totals by pricing status
- caller request tags for filtering and attribution

Request correlation is anchored on `x-request-id`. The HTTP middleware boundary owns request-id generation and propagation: caller-provided values are preserved, and missing values are generated once before handlers run.

Request outcomes are emitted once per request with bounded labels. Important examples in this slice are:

- `budget_error` for pre-provider hard-limit rejection
- `invalid_request` for capability mismatch
- `upstream_error` for upstream execution or stream failure

## Request Tagging Contract

Callers can attach bounded attribution metadata with:

- `x-oceans-service`
- `x-oceans-component`
- `x-oceans-env`
- `x-oceans-tags`

`x-oceans-tags` uses `key=value; key2=value2` formatting.

Validation rules:

- the universal headers may only be sent once each
- `x-oceans-tags` may only be sent once
- bespoke tags are capped at 5 entries
- bespoke keys must be unique
- reserved universal names cannot be reused as bespoke keys

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
- requested and resolved model identity
- provider key
- universal caller tags
- status, latency, and usage totals
- truncation flags
- metadata such as `operation`, `stream`, and `payload_policy`

`operation` is the public API family. Current values include `chat_completions`, `responses`, and `embeddings`.

Request-attempt rows describe upstream provider execution only. Pre-provider failures such as authentication rejection, capability mismatch, route unavailability, or budget hard-limit rejection have zero attempts. In the current runtime, successful provider-backed requests record one terminal attempt. Retry and fallback execution remain disabled until the configurable policy tracked in issue #118 is implemented.

Streaming requests persist a bounded transcript payload rather than raw transport bytes.

The stream payload contract is incremental rather than chunk-local:

- UTF-8 is reassembled across transport chunk boundaries
- SSE `data:` frames are reassembled across chunk boundaries
- both `data:` and `data: ` forms are accepted
- the latest coherent `usage` object is retained for request-log and ledger work
- Responses streams also retain usage from `response.usage` on completed response events

Request-log payloads are user-visible artifacts. They do not persist the transformed outbound provider request body produced by route compatibility profiles.

Provider stream transcripts can include normalized compatibility output, such as promoted usage or canonical reasoning deltas, because that normalized stream is what the gateway returns to callers. Responses streams preserve `response.*` event names and payloads rather than being rewritten into Chat Completions chunks.

## Payload Policy

Chat-completion request-log payload persistence is controlled by `request_logging.payloads` in `gateway.yaml`.

Default config:

```yaml
request_logging:
  payloads:
    capture_mode: redacted_payloads
    request_max_bytes: 65536
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
- team-owned API keys always persist request-log summary rows

This is why a user-owned request can be absent from request logs while a team-owned request with the same payload policy is still visible.

Validation rules:

- `request_max_bytes` must be greater than zero
- `response_max_bytes` must be greater than zero
- `stream_max_events` must be greater than zero
- `redaction_paths` must use dot-separated object keys, with `*` as a full-segment wildcard
- paths are anchored from the wrapped payload root, for example `body.messages.*.content.*.image_url.url`

Each request-log row persists lightweight policy metadata in `request_logs.metadata_json`:

```json
{
  "payload_policy": {
    "capture_mode": "redacted_payloads",
    "request_max_bytes": 65536,
    "response_max_bytes": 65536,
    "stream_max_events": 128,
    "version": "builtin:v1"
  }
}
```

## Redaction and Truncation Boundaries

Payloads are wrapped before policy application:

- requests: `{ "headers": ..., "body": ... }`
- responses: `{ "body": ... }`
- streams: `{ "stream": true, "events": ..., "usage": ..., "error": ... }`

Redaction applies one explicit built-in policy plus additive admin-configured paths from `request_logging.payloads.redaction_paths`.

Sensitive built-in headers include:

- `authorization`
- `anthropic-api-key`
- `cookie`
- `set-cookie`
- `x-goog-api-key`
- `x-api-key`

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

Known bulky provider fields are shape-preserving truncated before the whole-payload byte budget is applied. Built-ins cover OpenAI-compatible image/audio/file payloads, Vertex Gemini inline data, and Vertex Anthropic base64 source data.

Processing order:

1. wrap the payload
2. apply built-in and admin-configured redaction rules
3. truncate known bulky fields while preserving JSON shape where possible
4. apply `request_max_bytes` or `response_max_bytes` as a final guardrail

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
- `GET /api/v1/admin/observability/request-logs`
- `GET /api/v1/admin/observability/request-logs/{request_log_id}`

## Usage Leaderboard

The leaderboard is a separate admin observability surface from spend reporting.

Endpoint:

- `GET /api/v1/admin/observability/leaderboard?range=7d|31d`

Current semantics:

- ranked by total spend over the selected range
- ties sort by request count, then user name
- chart cohort is the top 5 ranked users
- table is the top 30 ranked users
- time buckets are 12-hour UTC buckets and are zero-filled for chart stability
- dominant model is chosen by request count, then spend, then model key

Use the leaderboard to identify recent high-usage users. Use spend reporting when the question is about owner totals, budgets, or pricing status counts.

Current list filters:

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

- no documented retention or archival policy yet for `request_log_payloads`
  - retention and purge work is tracked separately in [issue #105](https://github.com/ahstn/oceans-llm/issues/105)
- deploy examples do not ship an OTLP collector by default

## Relationship to Spend Reporting

Request logs and spend accounting are related, but intentionally separate.

- request logs describe the user-visible request outcome
- `usage_cost_events` is the canonical spend ledger

For the full request path across both systems, use [request-lifecycle-and-failure-modes.md](../reference/request-lifecycle-and-failure-modes.md).
