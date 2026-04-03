# ADR: Chat Observability Hardening and Incremental Stream-Parse Contract

- Date: 2026-03-31
- Status: Accepted
- Related Issues:
  - [#54](https://github.com/ahstn/oceans-llm/issues/54)
  - [#49](https://github.com/ahstn/oceans-llm/issues/49)
- Builds On:
  - [2026-03-15-otlp-observability-and-request-log-payloads.md](2026-03-15-otlp-observability-and-request-log-payloads.md)
  - [2026-03-15-v1-runtime-simplification.md](2026-03-15-v1-runtime-simplification.md)
  - [2026-03-17-post-success-accounting-and-strict-request-log-lookups.md](2026-03-17-post-success-accounting-and-strict-request-log-lookups.md)

## Current state

- [../operations/observability-and-request-logs.md](../operations/observability-and-request-logs.md)
- [../reference/request-lifecycle-and-failure-modes.md](../reference/request-lifecycle-and-failure-modes.md)
- [../operations/budgets-and-spending.md](../operations/budgets-and-spending.md)

## Context

Issue [#54](https://github.com/ahstn/oceans-llm/issues/54) exposed two correctness problems in the chat runtime that mattered for operators, maintainers, and future architectural discipline:

1. pre-provider budget rejection and provider execution were not described or measured as separate things,
2. streamed request-log capture treated raw transport chunks as if they were already valid UTF-8 and complete SSE frames.

Those problems were dangerous because they made the gateway look more reliable than it really was. A budget-rejected request could appear to have attempted provider execution, and a streamed response could appear successfully logged while silently dropping payload detail or retaining stale usage. Both outcomes undermine trust in observability, and observability is only valuable if operators can rely on it when the system is behaving badly.

The earlier runtime work had already moved the codebase away from compatibility-era fallback behavior:

- [2026-03-15-v1 runtime simplification](2026-03-15-v1-runtime-simplification.md) removed fallback-style request metadata and tightened the streaming contract.
- [2026-03-15 OTLP observability and payload-backed request logs](2026-03-15-otlp-observability-and-request-log-payloads.md) established request logs and payloads as explicit runtime contracts instead of incidental handler side effects.
- [2026-03-17 fail-open post-success accounting](2026-03-17-post-success-accounting-and-strict-request-log-lookups.md) clarified that user-visible success and downstream accounting integrity are different concerns.

Issue [#54](https://github.com/ahstn/oceans-llm/issues/54) sat directly on top of those decisions. If left unresolved, it would have reintroduced the same class of problem in a subtler form: strict-looking runtime contracts implemented on top of chunk-local parsing and ambiguous metrics semantics.

## Decision

We make two architectural decisions for chat observability:

### 1. Budget rejection is a request outcome, not a provider attempt

Hard-limit enforcement that rejects a request before provider execution must be represented as a request-level outcome only.

This means:

- the budget guard runs before provider execution in [../../crates/gateway/src/http/handlers.rs](../../crates/gateway/src/http/handlers.rs),
- a `budget_exceeded` rejection returns `429` and stops before any provider call,
- observability records that path as a bounded request outcome such as `budget_error`,
- we do not keep or reintroduce compatibility shims that imply a provider attempt happened when it did not.

Why:

- “provider attempted” and “request rejected before provider” are different operational facts,
- conflating them damages metric integrity and makes dashboards harder to trust,
- strict sequencing in the handler is simpler than compensating later with special-case corrections.

### 2. Stream payload capture is incremental and protocol-aware, not chunk-local

Streamed chat request logging must parse the upstream stream as an SSE protocol stream, not as independent transport chunks.

This means:

- UTF-8 is reassembled across chunk boundaries,
- SSE frames are reassembled across chunk boundaries,
- both `data:` and `data: ` forms are accepted,
- incomplete UTF-8 or incomplete final SSE frames are treated as parse failure,
- the latest coherent `usage` snapshot wins instead of the first snapshot winning forever.

The core implementation lives in:

- shared SSE parsing: [../../crates/gateway-core/src/streaming.rs](../../crates/gateway-core/src/streaming.rs)
- request-log stream collection: [../../crates/gateway-service/src/request_logging.rs](../../crates/gateway-service/src/request_logging.rs)
- chat handler integration: [../../crates/gateway/src/http/handlers.rs](../../crates/gateway/src/http/handlers.rs)

Why:

- transport chunk boundaries are not protocol boundaries,
- relying on chunk alignment is a hidden fallback that only “works” when upstream behavior happens to be convenient,
- keeping the latest usage snapshot matches how providers often emit interim then final totals,
- moving the parser into shared core code prevents divergent stream-parsing rules between providers, handlers, and request logging.

## How It Was Implemented

### Handler sequencing

The chat handler in [../../crates/gateway/src/http/handlers.rs](../../crates/gateway/src/http/handlers.rs):

- resolves routing first,
- constructs bounded request labels,
- enforces pre-provider budget policy before the provider is called,
- records a request outcome immediately on budget rejection.

This is the important architectural boundary: once the provider execution path begins, the request has crossed into a different phase of the lifecycle. Before that point, the runtime should not emit signals that imply provider work occurred.

### Shared SSE parsing in gateway-core

The parser in [../../crates/gateway-core/src/streaming.rs](../../crates/gateway-core/src/streaming.rs) now owns:

- split UTF-8 reassembly,
- split SSE frame reassembly,
- delimiter handling across `\n\n`, `\r\n\r\n`, and `\r\r`,
- acceptance of both `data:` and `data: ` line forms,
- explicit finalization errors for incomplete UTF-8 or incomplete terminal events.

This intentionally removes any need for handler-local or provider-local fallback parsing.

### Request-log collector semantics

The stream collector in [../../crates/gateway-service/src/request_logging.rs](../../crates/gateway-service/src/request_logging.rs):

- consumes parsed SSE events instead of raw chunks,
- stores the latest observed `usage` object,
- records stream failure metadata when an error payload or parse failure appears,
- produces a bounded, sanitized transcript payload for request-log storage.

This keeps the request-log contract aligned with the actual stream protocol rather than with the transport layer.

### Contract tests

The behavior is covered in:

- gateway integration tests in [../../crates/gateway/src/main.rs](../../crates/gateway/src/main.rs)
- request-logging unit tests in [../../crates/gateway-service/src/request_logging.rs](../../crates/gateway-service/src/request_logging.rs)
- shared streaming parser unit tests in [../../crates/gateway-core/src/streaming.rs](../../crates/gateway-core/src/streaming.rs)

These tests cover:

- budget rejection before provider execution,
- request outcome recording for budget rejection,
- split UTF-8 codepoints,
- split SSE frames,
- `data:` without a space,
- latest-usage retention,
- incomplete terminal frame handling.

## Documentation Consequences

The runtime behavior was already strict in code, but the docs needed to say that clearly.

The canonical pages now describe:

- budget rejection as a pre-provider `429 budget_exceeded` path,
- observability request outcomes for budget rejection,
- incremental stream parsing and latest-usage retention,
- removal of fallback-era expectations from this code path.

Canonical docs:

- [../operations/observability-and-request-logs.md](../operations/observability-and-request-logs.md)
- [../reference/request-lifecycle-and-failure-modes.md](../reference/request-lifecycle-and-failure-modes.md)
- [../operations/budgets-and-spending.md](../operations/budgets-and-spending.md)

We want future readers to learn the real contract from the docs, not from spelunking tests or reconstructing intent from issue threads.

## Consequences

Positive:

- request outcomes and provider execution are no longer semantically conflated,
- streamed request logs are more trustworthy under realistic network chunking,
- the parser contract is shared instead of being reimplemented in multiple layers,
- the docs now match the strict runtime behavior instead of hinting at legacy or fuzzy semantics.

Tradeoffs:

- malformed or incomplete upstream streams fail more explicitly,
- the stream collector is more stateful than a naive chunk-by-chunk implementation,
- operators must treat stream parse failures as real upstream/runtime integrity incidents rather than as ignorable logging noise.

## What We Explicitly Rejected

- Reintroducing provider-attempt or fallback-era compatibility metadata for pre-provider budget rejection.
- Preserving chunk-local parsing with best-effort heuristics.
- Keeping separate handler-local parsing rules alongside the shared parser.
- Treating the first observed usage object as canonical when a later coherent total is available.

Those would all preserve older, weaker patterns that make future maintenance harder.

## Follow-up Work

- Track and resolve the remaining post-provider stream versus non-stream rough edge in [#49](https://github.com/ahstn/oceans-llm/issues/49).
- Consider whether additional dashboards or alerting should key directly off stream parse failures and budget-error outcomes.
- Keep future observability changes routed through the shared parser and request-log lifecycle instead of adding new transport-local shortcuts.

## Attribution

This ADR was prepared through collaborative human + AI implementation and documentation work.
