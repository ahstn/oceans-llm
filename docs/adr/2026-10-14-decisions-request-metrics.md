# ADR: Decisions Request Metrics

- Date: 2026-10-14
- Status: Accepted

## Current state

- [Observability and Request Logs](../operations/observability-and-request-logs.md#decisions-request-metrics)
- [Export Traces and Metrics](../operations/observability/export-traces-and-metrics.md#verify-the-export)

## Context

`POST /v1/decisions` and `POST /v1/systemone` wrote request logs, provider attempts and usage, but recorded no request-rate or latency metric. A request that found no compatible route returned before recording anything, so an operator could not see a misconfigured Decisions model in metrics at all.

Chat Completions, Responses and Anthropic Messages record `gateway.chat.requests` and `gateway.chat.request.duration` on every exit, and they add tool-cardinality samples. Those samples always carry `http.route=/v1/chat/completions` and a `stream` label.

## Decision

Decisions requests record their own metric pair, `gateway.decisions.requests` and `gateway.decisions.request.duration`. They do not reuse the chat request metrics.

- **Labels.** `http.route` (`/v1/decisions` or `/v1/systemone`), `http.method`, `request_format` (`openai` or `system_one`), `requested_model`, `resolved_model`, `provider`, `http.response.status_code` and `outcome`. `outcome` is `success` or `GatewayError::error_type()`, the same values the chat metric uses.
- **Coverage.** `execute_decisions` resolves the model, then calls `dispatch_decisions` for everything after that. It records one sample from the result, so every later exit is covered: format filtering, routing errors, no compatible route, redaction, budget rejection, provider errors (including `PartialUsage` failures after billable usage), and success. `dispatch_decisions` reports the selected provider key back to the caller. Until a route is selected, `provider` is `unavailable`, which matches the chat no-route label.
- **Not recorded.** Authentication, body parsing and model resolution failures happen before the model labels exist. The chat metric does not record these failures either.
- **Usage metrics stay shared.** Decisions token, cost and usage-record totals still go to `gateway.chat.tokens`, `gateway.chat.cost.usd` and `gateway.chat.usage_records`. Spend dashboards already sum those totals across every priced operation.

### Why not reuse `gateway.chat.requests`

- The chat metric's fixed `http.route=/v1/chat/completions` would mislabel Decisions traffic. Making that label dynamic would change the series of every existing chat dashboard.
- `stream` is always false for Decisions, and tool cardinality does not apply. Recording either would add meaningless series.
- Decisions latency is a short classification call. Mixing it into the chat duration histogram would skew chat latency percentiles and SLOs.

## Trade-offs

- A dashboard that wants total inference traffic must add the two request counters together.
- Decisions request counts and Decisions token totals live under different metric prefixes. Use the `resolved_model` and `provider` labels, which both share, to join them.

## Follow-up

- `/v1/embeddings` has the same gap. It records no request metric on success, provider failure or the no-route exit, but it does record `gateway.chat.requests` and tool cardinality on a guardrail block (and on a budget rejection since #481). Fixing it means moving those existing series, so it needs its own change. That change should reuse this pattern, either with a dedicated embeddings metric or by turning this metric into a shared non-chat request metric with an `operation` label.
