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
- operational cost totals
- usage-record totals by pricing status
- caller request tags for filtering and attribution

Request correlation is anchored on `x-request-id`.

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

The summary row stores:

- request identity
- owner identity
- requested and resolved model identity
- provider key
- universal caller tags
- status, latency, and usage totals
- truncation flags
- metadata such as `operation` and `stream`

`operation` is the public API family. Current values include `chat_completions`, `responses`, and `embeddings`.

Streaming requests persist a bounded transcript payload rather than raw transport bytes.

The stream payload contract is incremental rather than chunk-local:

- UTF-8 is reassembled across transport chunk boundaries
- SSE `data:` frames are reassembled across chunk boundaries
- both `data:` and `data: ` forms are accepted
- the latest coherent `usage` object is retained for request-log and ledger work
- Responses streams also retain usage from `response.usage` on completed response events

Request-log payloads are user-visible artifacts. They do not persist the transformed outbound provider request body produced by route compatibility profiles.

Provider stream transcripts can include normalized compatibility output, such as promoted usage or canonical reasoning deltas, because that normalized stream is what the gateway returns to callers. Responses streams preserve `response.*` event names and payloads rather than being rewritten into Chat Completions chunks.

## Redaction and Truncation Boundaries

Current redaction is key-driven and header-driven.

Sensitive headers include:

- `authorization`
- `cookie`
- `set-cookie`
- `x-api-key`

Sensitive JSON keys include:

- `token`
- `access_token`
- `refresh_token`
- `secret`
- `password`

Current payload policy is still heuristic and bounded. It is not operator-configurable yet.

## Recent Contract Cleanup

Recent cleanup changed the contract in a few important ways.

- fallback-era request metadata is gone
- missing request-log detail rows return strict `404 not_found`
- stream payload parsing is more boundary-safe than the earlier chunk-by-chunk behavior
- budget-rejected chat requests record a `budget_error` request outcome without executing the provider

Operators and maintainers should stop expecting:

- fallback metadata columns to appear in new request rows
- nullable detail lookups for missing rows

## Admin Observability APIs

Platform admins can inspect request logs through:

- `GET /api/v1/admin/observability/request-logs`
- `GET /api/v1/admin/observability/request-logs/{request_log_id}`

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
- deploy examples do not ship an OTLP collector by default
- request-log payload policy is not operator-configurable yet
  - [issue #18](https://github.com/ahstn/oceans-llm/issues/18)
- stream and non-stream chat paths still differ on post-provider ledger-write failure behavior
  - [issue #49](https://github.com/ahstn/oceans-llm/issues/49)

## Relationship to Spend Reporting

Request logs and spend accounting are related, but intentionally separate.

- request logs describe the user-visible request outcome
- `usage_cost_events` is the canonical spend ledger

For the full request path across both systems, use [request-lifecycle-and-failure-modes.md](../reference/request-lifecycle-and-failure-modes.md).
