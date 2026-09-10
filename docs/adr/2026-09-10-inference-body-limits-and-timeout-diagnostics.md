# Request body limits and provider timeout diagnostics

## Decision

Use a 64 MiB (67,108,864 byte) Axum extractor limit on authenticated inference routes. Authenticate from request headers before reading or parsing JSON. Retain the existing 64 MiB batch limit and the existing limits on other API routes. Use a five-minute (300,000 ms) default total provider deadline. Explicit `providers[].timeouts.total_ms` settings continue to override it, including when a longer deadline is needed.

Provider HTTP connections have a separate 10-second connection timeout. Do not add a shorter idle-read timeout: a reasoning model can remain silent while it works. The total deadline bounds that wait and includes response streaming. Keep-alive events do not extend it. These defaults do not change MCP guardrail buffering or admin UI proxy timeouts.

## Why

Axum's implicit 2 MiB limit rejects large agent conversation requests before inference request logging starts. A retry can send the same history and fail again. The error gives no useful limit or recovery advice. A request can also receive response headers and later reach Reqwest's total deadline, which previously defaulted to two minutes. Reporting only the top-level body-read error hides the timeout cause.

The 64 MiB limit gives the gateway room for large conversation and tool payloads. It is a gateway acceptance limit, not a promise that each provider accepts this size. Anthropic documents a 32 MB Messages limit and 256 MB batch limit. Provider limits remain independent.

Five minutes is the default for this change. The OpenAI Python SDK documents ten minutes, and Anthropic discusses ten-minute non-streaming requests. These SDK settings are useful reference points, but their read-timeout semantics are not equivalent to Reqwest's total wall-clock deadline. A ten-minute default could help long reasoning requests, but would also retain stalled gateway requests longer. Use an explicit route-provider timeout override when production evidence supports it.

## Implementation and diagnostics

- The inference router sets `DefaultBodyLimit` explicitly. A request-parts extractor authenticates once before JSON extraction and passes the authenticated key to the handler. A body wrapper counts received data bytes without retaining another payload copy or removing trailers.
- The HTTP server span records received body bytes, the configured limit, and the declared content length when available. Received bytes are a lower bound if the body was rejected before consumption finished; content length is client-declared, not verified.
- A local body-limit rejection uses the canonical `PayloadTooLarge` error contract (with the Anthropic `type: error` envelope on both Messages aliases) with `request_body_too_large`, the limit, received byte count, and request ID. It emits a warning and marks the trace as failed before inference logging is available. Provider 413 responses retain their existing handling.
- Provider HTTP traces retain bounded error source chains with attached Reqwest URLs removed. They record elapsed time on stream completion, failure, or drop, and upstream `x-request-id` and `cf-ray` headers when present. Stream error messages identify timeout or transport classification without exposing the source chain to callers.
- Stored request payload limits remain separate from incoming request limits. Use byte-count fields to measure requests; do not infer their size from truncated request-log payloads.

## Trade-offs and follow-up

The higher limit increases the maximum per-request buffering cost. JSON parsing and provider translation can allocate additional memory. Check memory and concurrency under production load. A proxy or provider can still impose a smaller body limit or deadline.

Retrieve the incident traces and confirm the deployed provider settings. If long-running requests still need more time, choose a provider-specific deadline from observed durations. Review trace sampling and log retention so early 413 warnings and stream failures remain discoverable. Production deployment and incident confirmation are separate from local regression tests.

## Sources

Reviewed with Exa on 2026-09-10:

- [Axum default body limits](https://docs.rs/axum/latest/axum/extract/struct.DefaultBodyLimit.html).
- [Anthropic request size limits and long requests](https://docs.anthropic.com/en/api/errors).
- [OpenAI Python SDK timeouts](https://github.com/openai/openai-python#timeouts).
- [Reqwest total deadlines versus read timeouts](https://github.com/seanmonstar/reqwest/issues/2237).
