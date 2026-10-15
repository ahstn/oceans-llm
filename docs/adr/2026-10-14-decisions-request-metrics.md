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

- **Resolved: `/v1/embeddings` gets a dedicated metric pair.** Embeddings recorded no request metric on success, provider failure or the no-route exit. It recorded `gateway.chat.requests` and tool cardinality only on a guardrail block, and on a budget rejection since #481, always under the chat route. It now records `gateway.embeddings.requests` and `gateway.embeddings.request.duration` with the same coverage and labels as Decisions, minus `request_format`, and no longer emits chat request or tool-cardinality samples. See [Embeddings Request Metrics](../operations/observability-and-request-logs.md#embeddings-request-metrics).

  We chose a dedicated pair over one shared non-chat request metric with an `operation` label, for these reasons:

  - Embeddings has no `request_format`. A shared metric would carry a label that is empty for every embeddings sample.
  - Embeddings latency depends on input batch size, and Decisions latency on question count. One histogram would blend the two, so each operation's percentiles would need an `operation` filter to mean anything.
  - The two metrics already match the chat metric's naming, `gateway.<operation>.requests`, so dashboards query them the same way.

  The cost is one more pair to add up for total inference traffic. If a third non-chat operation needs a request metric, reconsider a shared `operation`-labelled metric then.
