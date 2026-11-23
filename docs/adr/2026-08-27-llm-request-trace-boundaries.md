# LLM Request Trace Boundaries

- Status: Accepted
- Date: 2026-08-27

## Decision

Oceans traces an LLM request through the gateway phases that own its work. The inbound request is an OpenTelemetry `SERVER` span. Each outbound provider request is a `CLIENT` span. Internal work uses stable `gateway.*` span names.

The provider operation and provider stream are separate spans. The provider operation ends after the provider returns a response or stream handle. The stream span starts before that provider call and stays open until the upstream stream completes, fails, or the downstream client disconnects.

Trace attributes must not contain prompts, message content, tool arguments, credentials, full request headers, or URL query values. Outbound URLs remove user information, query strings, and fragments before export.

## Trace Shape

A model request can include these spans:

- `http.server.request`
- `gateway.auth.authenticate`
- `gateway.auth.verify_api_key`
- `gateway.request.resolve`
- `gateway.model.access`
- `gateway.model.alias_resolution`
- `gateway.route.plan`
- `gateway.route.select`
- `gateway.request.prepare`
- `gateway.request.redact_and_bound`
- `gateway.mcp.telemetry`
- `gateway.budget.precheck`
- `gateway.provider.operation`
- `gateway.provider.prepare_request`
- `gateway.provider.credentials`
- `http.client.request`
- `gateway.provider.stream`
- `gateway.usage.accounting`
- `gateway.usage.ledger`
- `gateway.request_log.persist`

Not every request has every span. For example, a non-stream request has no provider stream span. Payload capture settings can also make request-log persistence a no-write operation.

## Stream Milestones

The stream span records the time to the first received chunk and the first semantic output. A role-only event is not semantic output. Text, refusal, tool-call, function-call, Responses API delta, and Anthropic content delta events are semantic output.

The span also records chunk count, byte count, total duration, terminal-event presence, and termination reason. Transport failures, error events, and client cancellation set an error status. A client cancellation creates a best-effort request log with status `499` and error code `client_cancelled`. It does not create usage cost from an incomplete stream.

## Sampling

The gateway keeps parent-based head sampling with the configured root sample ratio. Tail sampling belongs in the OpenTelemetry Collector because the gateway does not know the final trace outcome when it makes a head-sampling decision.

A collector can retain slow traces, error traces, stream cancellations, and budget failures only from traces that it receives. Gateway head sampling drops unsampled root traces before export, so collector tail sampling cannot recover them. To make all gateway root traces eligible for collector tail sampling, keep the gateway sample ratio at `1.0` and reduce volume in the collector. This collector policy is an operating choice and is not part of the gateway process.

## Why

The former trace showed the complete HTTP duration and only the short provider stream setup. It did not show where the remaining stream time was spent. The new boundaries show control-plane work, provider network time, stream delivery, accounting, and persistence without placing timer code throughout the HTTP handler.

Stable ownership also reduces trace drift. Authentication spans live with authentication code. Routing spans live with routing code. Provider request spans live with provider code. Stream milestones use the existing SSE parser, so tracing does not parse the stream a second time.

## Trade-offs

- More spans increase trace volume and exporter work.
- High-volume stream events are limited to first chunk, first output, usage arrival, and termination. The gateway does not create one event per chunk.
- A cancellation log uses status `499`, although the gateway can already have sent HTTP status `200` for the stream.
- Cancellation persistence is best effort because a dropped response body cannot wait for database work.
- The stream byte count measures normalized bytes forwarded by the gateway, not the provider's compressed network bytes.

## Follow-up

The LLM HTTP handlers remain in one large file. This change keeps request logging and stream finalization in their existing workflow to avoid a mixed tracing and handler refactor. A separate change should extract operation-specific chat, Responses, embeddings, and stream-lifecycle modules after focused handler tests exist. The shared trace vocabulary and provider HTTP helper added here are already separate from the handlers.

## Rejected Alternatives

### Add all phase timers to HTTP handlers

Rejected because it duplicates ownership and makes the large handlers harder to change.

### Mark the provider operation as the network client span

Rejected because stream setup, request mapping, and the actual HTTP exchange have different lifetimes. A dedicated `CLIENT` span gives correct network semantics.

### Export request and response content in trace attributes

Rejected because trace backends are not the approved payload store. Request logs apply the configured redaction, bounding, access, and retention rules.

## Amendment: one owner span for each wait

On 2026-11-22, the stream span became the parent of the provider operation, and three timing gaps were closed:

- The `http.client.request` span is entered on every body poll, so it ends when the upstream body ends. Since tracing-opentelemetry 0.32, a span ends when it is last exited, not when it is dropped. Before this change, a streaming client span ended when the response headers arrived.
- `gateway.provider.operation` is created inside `gateway.provider.stream`. A backend that divides time between concurrent sibling spans, such as Datadog's execution-time breakdown, then gives upstream time to the client span and not to the gateway's stream span.
- The stream span now ends when the upstream stream ends. Usage accounting, the routing update and the request-log write run in a `gateway.stream.finalize` span that is a child of the request span. A routing-state failure is recorded on that span, not on the stream span. The `inference stream terminated` log still reports `routing_state_error`.

These spans were added for steps that had no span:

- `gateway.request.read_body` and `gateway.request.decode` for the inference endpoints' JSON bodies, which separate the client's upload time from decoding
- `gateway.rate_limit.check`
- `gateway.route.select` for credential filtering and session-affinity lookup, and `gateway.route.complete` for the response-origin and affinity writes
- `gateway.guardrails.prompt`, `gateway.guardrails.redact` and `gateway.guardrails.response`, and `gateway.guardrails.stream` for the evaluation of a buffered stream

`gateway.guardrails.stream` covers only the evaluation. The time spent buffering the upstream stream belongs to the client span.
