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
- caller request tags in request-log storage for filtering and attribution

The request path also records tracing spans enriched with routing and ownership context.

Metric contract:

- `gateway.chat.requests` describes routed request outcomes, including budget rejections after model resolution
- `gateway.chat.provider.attempts` describes real upstream calls only and does not move when the request is rejected before provider execution

Request correlation is anchored on `x-request-id`:

- the gateway generates or propagates it at the HTTP edge
- public `/v1/*` responses return it
- provider adapters forward it upstream
- admin request-log lookup can use it as the operator-visible correlation key

## Request Tagging Contract

Callers can attach bounded attribution metadata to requests with these headers:

- `x-oceans-service`
- `x-oceans-component`
- `x-oceans-env`
- `x-oceans-tags`

Intended use:

- use the universal headers for the stable dimensions most teams will filter on repeatedly
- use `x-oceans-tags` only for a small number of extra exact-match tags

`x-oceans-tags` uses `key=value; key2=value2` formatting.

Examples:

- `x-oceans-service: checkout`
- `x-oceans-component: pricing_api`
- `x-oceans-env: prod`
- `x-oceans-tags: feature=guest_checkout; cohort=beta`

Current validation rules:

- universal headers may only be sent once each
- `x-oceans-tags` may only be sent once
- bespoke tags are capped at 5 entries
- bespoke keys must be unique inside the header
- bespoke keys may not reuse reserved universal names: `service`, `component`, `env`
- keys must start with a lowercase ASCII letter and then use only lowercase ASCII letters, digits, `.`, `_`, or `-`
- values must use only lowercase ASCII letters, digits, `.`, `_`, `-`, `/`, or `:`
- malformed or duplicate tags are rejected as `400 invalid_request`

## Request Log Storage Shape

Request logs are intentionally split:

- `request_logs`: hot summary row
- `request_log_payloads`: sanitized payload bodies

The summary row stores:

- request identity
- owner identity
- requested and resolved model identity
- provider key
- universal caller tags
- status, latency, and usage totals
- truncation flags
- metadata

The payload row stores:

- sanitized request JSON
- sanitized response JSON

The gateway also stores bespoke caller tags in a bounded side table:

- `request_log_tags`

Streaming requests persist a bounded transcript payload rather than raw transport bytes.
Stream payload capture is incremental and boundary-safe across UTF-8 and SSE chunk splits, and the stored `usage` snapshot always reflects the latest coherent usage frame seen before stream termination.

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
- `service`
- `component`
- `env`
- `tag` using `key=value`

## Important Current Rough Edges

Current behavior that operators and maintainers should know plainly:

- request-log detail lookups currently return `200` with nullable `data` for missing rows instead of `404`
- stream and non-stream chat paths still differ on post-provider ledger-write failure behavior

Tracked follow-ups:

- [issue #50](https://github.com/ahstn/oceans-llm/issues/50): missing request-log detail should become `404`
- [issue #49](https://github.com/ahstn/oceans-llm/issues/49): unify post-provider ledger failure semantics

## Relationship to Spend Reporting

Request logs and spend accounting are related but intentionally separate:

- request logs describe the user-visible request outcome and payload context
- `usage_cost_events` is the canonical spend ledger

For spend policy and budget windows, see [budgets-and-spending.md](budgets-and-spending.md).
