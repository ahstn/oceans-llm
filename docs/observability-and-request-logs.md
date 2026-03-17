# Observability and Request Logs

`Owns`: OTLP observability model, request-log storage shape, payload redaction and truncation boundaries, and admin observability API behavior.
`Depends on`: [data-relationships.md](data-relationships.md), [model-routing-and-api-behavior.md](model-routing-and-api-behavior.md)
`See also`: [admin-control-plane.md](admin-control-plane.md), [budgets-and-spending.md](budgets-and-spending.md), [deploy-and-operations.md](deploy-and-operations.md), [adr/2026-03-15-otlp-observability-and-request-log-payloads.md](adr/2026-03-15-otlp-observability-and-request-log-payloads.md)

This document describes the live observability contract for the gateway.

## Source of Truth

- Observability bootstrap: [../crates/gateway/src/observability.rs](../crates/gateway/src/observability.rs)
- HTTP request instrumentation: [../crates/gateway/src/http/handlers.rs](../crates/gateway/src/http/handlers.rs)
- Request-log lifecycle: [../crates/gateway-service/src/request_logging.rs](../crates/gateway-service/src/request_logging.rs)
- Redaction policy: [../crates/gateway-service/src/redaction.rs](../crates/gateway-service/src/redaction.rs)
- Admin APIs: [../crates/gateway/src/http/observability.rs](../crates/gateway/src/http/observability.rs)

## OTLP-First Model

The gateway exports tracing spans and metrics through OpenTelemetry.

Current config knobs:

- `server.otel_endpoint`
- `server.otel_metrics_endpoint`
- `server.otel_export_interval_secs`

The intended deployment path for this slice is collector-friendly OTLP export rather than an in-process Prometheus endpoint.

## What Gets Recorded

The runtime emits bounded request-level signals for:

- chat request totals
- request latency
- provider attempts
- token totals
- operational cost totals
- usage-record totals by pricing status

The request path also records tracing spans enriched with routing and ownership context.

Request correlation is anchored on `x-request-id`:

- the gateway generates or propagates it at the HTTP edge
- public `/v1/*` responses return it
- provider adapters forward it upstream
- admin request-log lookup can use it as the operator-visible correlation key

## Request Log Storage Shape

Request logs are intentionally split:

- `request_logs`: hot summary row
- `request_log_payloads`: sanitized payload bodies

The summary row stores:

- request identity
- owner identity
- requested and resolved model identity
- provider key
- status, latency, and usage totals
- truncation flags
- metadata

The payload row stores:

- sanitized request JSON
- sanitized response JSON

Streaming requests persist a bounded transcript payload rather than raw transport bytes.

## Redaction and Truncation Boundaries

Current redaction is key- and header-driven.

Sensitive headers include:

- `authorization`
- `proxy-authorization`
- `cookie`
- `set-cookie`
- `x-api-key`

Sensitive JSON keys include common fields such as:

- `token`
- `access_token`
- `refresh_token`
- `secret`
- `password`

Current payload policy is intentionally heuristic and bounded. It is not yet a deployment-configurable policy surface. That hardening follow-up is tracked in [issue #18](https://github.com/ahstn/oceans-llm/issues/18).

## Current Limits and Gaps

Operationally important limits that are not configurable today:

- there is no documented retention or archival policy yet for `request_log_payloads`
- deploy examples do not ship an OTLP collector by default
- request-log payload policy is bounded and heuristic rather than operator-configurable

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

## Important Current Rough Edges

Current behavior that operators and maintainers should know plainly:

- request-log detail lookups currently return `200` with nullable `data` for missing rows instead of `404`
- stream and non-stream chat paths still differ on post-provider ledger-write failure behavior
- streamed request-log capture still has known follow-up work around chunk-boundary parsing and observability correctness

Tracked follow-ups:

- [issue #50](https://github.com/ahstn/oceans-llm/issues/50): missing request-log detail should become `404`
- [issue #49](https://github.com/ahstn/oceans-llm/issues/49): unify post-provider ledger failure semantics
- [issue #54](https://github.com/ahstn/oceans-llm/issues/54): harden stream metrics and streamed request-log parsing

## Relationship to Spend Reporting

Request logs and spend accounting are related but intentionally separate:

- request logs describe the user-visible request outcome and payload context
- `usage_cost_events` is the canonical spend ledger

For spend policy and budget windows, see [budgets-and-spending.md](budgets-and-spending.md).
