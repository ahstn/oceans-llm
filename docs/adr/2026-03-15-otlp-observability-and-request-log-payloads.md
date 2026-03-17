# ADR: OTLP-First Observability and Payload-Backed Request Logs

- Date: 2026-03-15
- Status: Accepted

## Context

The gateway already had:

- structured tracing via `tracing`,
- operational request logging persisted in `request_logs`,
- durable usage ledger accounting for spend and budget enforcement,
- an admin UI route for request logs that still depended on mock data.

Issues #24, #8, and #16 required the observability layer to move beyond baseline logs:

- runtime metrics needed to be emitted in a vendor-neutral way alongside traces,
- request logging needed to capture sanitized request and response payloads without overloading the hot summary table,
- the request-log write path needed to move out of the HTTP handler into a dedicated lifecycle service,
- the admin UI needed real gateway-backed read APIs for request-log inspection.

We also wanted the observability direction to stay aligned with the broader Rust ecosystem around `tracing` and OpenTelemetry rather than introducing a separate metrics model.

## Decision

### 1. Use OpenTelemetry OTLP as the primary observability export path

The gateway now initializes tracing and metrics together through one observability bootstrap:

- `tracing-opentelemetry` remains the bridge for spans,
- an OpenTelemetry `SdkMeterProvider` is initialized for metrics,
- both export over OTLP by default,
- `server.otel_endpoint` remains the shared endpoint,
- `server.otel_metrics_endpoint` is an optional metrics-only override,
- `server.otel_export_interval_secs` controls periodic metric export cadence.

Why:
- traces and metrics should share the same resource identity and export model,
- OTLP keeps the deployment path collector-friendly and vendor-neutral,
- this avoids building a separate Prometheus-only metrics surface into the gateway runtime for this slice.

### 2. Emit explicit gateway metrics in the chat execution path

Runtime metrics are emitted directly from the chat handler and stream completion path instead of deriving them indirectly from log events.

The gateway now emits:

- chat request totals,
- request latency histograms,
- provider attempt totals,
- fallback totals,
- token totals,
- operational cost totals,
- usage-record totals by pricing status.

Metric attributes are intentionally bounded to stable routing dimensions such as requested model, resolved model, provider, stream mode, status code, fallback usage, and pricing status.

Why:
- the handler already knows the logical request boundary and safe labels,
- one explicit emission per request is easier to reason about than event-derived metrics,
- bounded labels prevent cardinality explosions from request ids, API key ids, or raw upstream values.

### 3. Split request-log storage into summary and payload tables

We keep `request_logs` as the summary table and add `request_log_payloads` keyed by `request_log_id`.

The summary row stores:

- request and routing identity,
- status, latency, usage totals, and error code,
- payload presence and truncation flags,
- lightweight metadata such as stream mode, attempt count, and fallback usage.

The payload row stores sanitized request and response bodies:

- request payloads include normalized inbound body and safe headers,
- non-stream responses store the final normalized response body,
- stream responses store a bounded transcript of SSE `data:` events plus extracted usage and terminal error details when present.

Why:
- summary scans should stay fast without repeatedly reading large JSON payloads,
- payload inspection still needs to be possible from the admin UI,
- a split design keeps libsql and PostgreSQL behavior aligned while allowing PostgreSQL to use `JSONB`.

### 4. Centralize request-log assembly in a dedicated lifecycle service

The `RequestLogging` service now owns:

- request payload capture,
- header and JSON redaction,
- payload truncation,
- stream transcript collection,
- summary assembly,
- persistence into summary and payload tables.

The HTTP handler now orchestrates execution and reports outcomes into that service instead of constructing `RequestLogRecord` values inline.

Why:
- logging concerns were beginning to dominate the request handler,
- stream and non-stream flows needed one consistent sanitization and persistence model,
- moving persistence logic into the service layer keeps handler changes smaller as observability evolves.

### 5. Expose real admin observability read APIs and wire the UI to them

The gateway now exposes:

- `GET /api/v1/admin/observability/request-logs`
- `GET /api/v1/admin/observability/request-logs/{request_log_id}`

Both routes require an authenticated platform-admin session. The admin UI request-log page now consumes these APIs directly and provides a detail dialog for sanitized payload inspection.

Why:
- observability views should reflect real runtime data rather than placeholders,
- list/detail APIs match the split summary/payload storage model,
- keeping the admin UI behind the existing session gate avoids introducing a separate auth path for observability.

### 6. Prefer append-only indexing over partitioning in this slice

For PostgreSQL, request-log indexing now favors:

- BRIN on `occurred_at`,
- B-tree lookups on request and owner identifiers,
- a simple B-tree join path for payload rows.

We did not introduce native partitioning in this slice.

Why:
- the current repo does not yet have partition lifecycle tooling,
- BRIN plus append-only access patterns is a lower-complexity fit for the current workload,
- this keeps libsql and PostgreSQL semantics closer while still improving production-shaped scans.

## Consequences

Positive:

- the gateway exports traces and metrics through one OTLP-oriented observability model,
- request logs now retain sanitized payload detail without bloating the summary table,
- stream and non-stream request logging follow one lifecycle abstraction,
- the admin UI observability screen now reflects live gateway data.

Tradeoffs:

- observability bootstrap and shutdown logic are more complex than the earlier tracing-only setup,
- payload persistence increases storage volume and requires explicit truncation policy,
- request-log read APIs add another admin surface that must stay aligned with backend schema evolution.

## Follow-up Work

- Add dedicated metric-exporter assertions for the OpenTelemetry instruments.
- Add explicit PostgreSQL query-plan validation for request-log list and lookup paths.
- Introduce retention and archival policy for request-log payload data once production volume is clearer.
- Expand admin filtering and search only after the bounded summary fields prove sufficient in real usage.

## Attribution

This ADR was prepared through collaborative human + AI implementation/design work.
