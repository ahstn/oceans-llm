# Observability and Request Logs

`Owns`: the OTLP observability model, request-log storage shape, payload redaction and truncation boundaries, and admin observability API behavior.
`Depends on`: [data-relationships.md](data-relationships.md), [model-routing-and-api-behavior.md](model-routing-and-api-behavior.md)
`See also`: [request-lifecycle-and-failure-modes.md](request-lifecycle-and-failure-modes.md), [admin-control-plane.md](admin-control-plane.md), [deploy-and-operations.md](deploy-and-operations.md), [adr/2026-03-15-otlp-observability-and-request-log-payloads.md](adr/2026-03-15-otlp-observability-and-request-log-payloads.md)

This document describes the live observability contract for the gateway.

## Source of Truth

- observability bootstrap:
  - [../crates/gateway/src/observability.rs](../crates/gateway/src/observability.rs)
- HTTP request instrumentation:
  - [../crates/gateway/src/http/handlers.rs](../crates/gateway/src/http/handlers.rs)
- request-log lifecycle:
  - [../crates/gateway-service/src/request_logging.rs](../crates/gateway-service/src/request_logging.rs)
- redaction policy:
  - [../crates/gateway-service/src/redaction.rs](../crates/gateway-service/src/redaction.rs)
- admin APIs:
  - [../crates/gateway/src/http/observability.rs](../crates/gateway/src/http/observability.rs)

## OTLP-First Model

The gateway exports tracing spans and metrics through OpenTelemetry.

Current config knobs:

- `server.otel_endpoint`
- `server.otel_metrics_endpoint`
- `server.otel_export_interval_secs`

The intended deploy path is collector-friendly OTLP export rather than an in-process Prometheus endpoint.

## What Gets Recorded

The runtime emits bounded request-level signals for:

- chat request totals
- request latency
- token totals
- operational cost totals
- usage-record totals by pricing status
- caller request tags for filtering and attribution

Request correlation is anchored on `x-request-id`.

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

Streaming requests persist a bounded transcript payload rather than raw transport bytes.

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

Operators and maintainers should stop expecting:

- fallback metadata columns to appear in new request rows
- nullable detail lookups for missing rows

The remaining stream and ledger mismatch still lives in a smaller rough edge, not in the old fallback-era contract.

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

For the full request path across both systems, use [request-lifecycle-and-failure-modes.md](request-lifecycle-and-failure-modes.md).
