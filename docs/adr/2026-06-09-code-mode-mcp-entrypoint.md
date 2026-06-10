# ADR: Code Mode MCP Entrypoint

Date: 2026-06-09

Issue: [#172](https://github.com/ahstn/oceans-llm/issues/172)

## Status

Accepted

## Decision

Oceans adds a third gateway-owned MCP data-plane surface at the reserved route `/code-mode-mcp`. It exposes exactly two tools, `explore` and `execute`, each taking a single required `code` string interpreted as the body of a JavaScript async arrow function (the Cloudflare Code Mode shape). Instead of the agent issuing one MCP round trip per tool interaction, the model writes code that searches, describes, filters, composes, and calls granted tools inside a sandbox through an `oceans.*` host API.

The surface is layered as:

- A backend-neutral executor abstraction in `gateway-service` (`CodeExecutor`, `HostDispatcher`, `CapabilityProfile`, `CodeModeLimits`, `ExecutionOutcome`), so the sandbox technology is swappable without touching route, policy, or logging code.
- Capability-profiled host dispatch: `explore` may call `oceans.searchTools` and `oceans.describeTool`; `execute` additionally gets `oceans.callTool`. One sandbox path, two capability sets, enforced host-side per call.
- A first production backend in `crates/gateway-code-mode-wasmtime`: Wasmtime running a bespoke QuickJS guest (`crates/code-mode-guest`, rquickjs with quickjs-ng 0.15.0 compiled to `wasm32-wasip1`).

The sandbox is never the policy engine. Every `oceans.*` call re-enters the gateway and is re-authorized through the existing grant, schema-hash, and credential machinery used by aggregate `call_tool`. The sandbox never sees Oceans API keys, session IDs, upstream credentials, `secret_ref` values, environment variables, filesystem, processes, or network.

The surface is disabled by default. `mcp.code_mode.enabled: false` (or an absent block) makes `/code-mode-mcp` return 404, the same posture as unknown server keys. `server_key` registration rejects the reserved keys `mcp` and `code-mode-mcp` in registry validation so an upstream registration can never collide with gateway-owned routes.

Both `explore` and `execute` log a parent invocation row under the synthetic identity `server_display_key = "code-mode"`, `tool_display_key = "explore" | "execute"`. Nested `oceans.callTool` executions log ordinary invocation rows linked to the parent through the new nullable self-referencing `parent_invocation_id` column (migration V32), exposed as an admin observability list filter.

Code Mode has zero budget integration: no precheck, no ledger rows. MCP execution carries no provider cost in Oceans' accounting model; spend stays attributed to the LLM requests that drive the agent. Nested tool calls get invocation logging with parent linkage only.

No legacy HTTP+SSE fallback is added. `/code-mode-mcp` is Streamable HTTP only, same as the aggregate endpoint, and Code Mode tools are not mirrored onto `/mcp` — the surfaces stay disjoint.

## Implementation

- `crates/gateway/src/config.rs`: global-only `mcp.code_mode` block (`enabled`, `sandbox_backend`, `limits`). `wasmtime_quickjs` is the only accepted `sandbox_backend`; unknown values fail deserialization, zero limits fail validation. The deterministic test executor is not selectable from YAML.
- `crates/gateway/src/http/mcp_gateway/code_mode.rs`: the route. POST/DELETE only (GET is 405), query strings rejected, Oceans API-key auth for user and service-account owners, 404 when disabled. Session semantics are shared with `/mcp` through the extracted `mcp_gateway/session.rs` module over `mcp_aggregate_sessions` — shared machinery, not a copy of `aggregate.rs`.
- `crates/gateway-service/src/mcp_code_mode.rs`: `CodeModeService` orchestration, `OceansHostDispatcher` (grant re-check, schema-hash preflight, credential resolution, upstream call, nested logging), `HostCallPolicy` (capability profile, `max_host_calls`, 256 KiB per-call argument cap), centrally applied output/log limits, and the `DeterministicTestExecutor` for route/service tests.
- `crates/gateway-mcp/src/server.rs`: `explore`/`execute` tool definitions whose descriptions embed the `oceans.*` typings and worked examples; result helpers (`isError: true` + `Error: ...` text; `--- TRUNCATED ---` marker).
- `crates/gateway-code-mode-wasmtime`: Wasmtime engine and the embedded guest artifact, validated at startup (startup fails loudly if the artifact is invalid or declares unstubbed imports).
- `crates/code-mode-guest`: the QuickJS guest. Exports `alloc`/`dealloc`/`evaluate`; imports only `oceans.oceans_call` carrying the Cloudflare-compatible `{"result"}/{"error"}` JSON envelope. There is no event loop — every `await oceans.*()` completes synchronously because the host import blocks the guest, and host-call `{"error"}` envelopes surface as ordinary catchable exceptions. `console.log/warn/error` is captured into outcome logs. The compiled `.wasm` is checked in; `mise run code-mode-guest-build` refreshes it and `mise run code-mode-guest-check` is the CI drift gate.
- `crates/gateway-store/migrations/V32__mcp_code_mode_invocation_linkage.sql` (plus the Postgres twin): nullable self-referencing `parent_invocation_id` on `mcp_tool_invocations` with `ON DELETE SET NULL` and an index for the admin filter.

Stable structured tool errors (`credential_required`, `credential_expired`, `tool_schema_changed`, policy denials) pass through the envelope unchanged, so Code Mode callers see the same contracts as aggregate `call_tool`.

### Sandbox security posture

- No WASI context is linked. The guest's libc imports are satisfied by zero-capability stubs (`wasi_stubs.rs`): host CSPRNG bytes for hash seeds, wall-clock time for `Date`, empty environment, no-op stdio, and traps for everything else. The module is validated against the linker at startup, so a guest rebuild that grows new imports fails loudly instead of silently gaining capabilities.
- Fresh `Store` per execution; no state survives between runs and parallel executions are isolated.
- Memory is capped by a `ResourceLimiter` (default 64 MiB); table growth and instance counts are tightly bounded; `max_wasm_stack` is set below the async fiber stack.
- Epoch interruption (10 ms ticks) forces the guest fiber to yield back to the tokio executor on every tick, so CPU-bound guest code never pins a runtime worker thread; a `tokio::time::timeout` around the whole call is the sole terminator and also covers host-call hangs. A bounded semaphore (`limits.max_concurrent_executions`, default 4) caps concurrently running executions gateway-wide; queue time counts against the wall clock.
- Cranelift is the only compilation strategy — never Winch for hostile code.
- Trap mapping keeps host detail host-side: epoch interrupts map to a timeout outcome, denied memory growth maps to a resource-limit error, and guest panics map to a redacted generic failure.
- `wasmtime` and `rquickjs` are pinned exactly in the workspace `Cargo.toml`. Maintainers must track wasmtime GHSA advisories; wasmtime had two critical sandbox-escape advisories patched in April 2026, and the pin must be bumped promptly when sandbox-relevant advisories land.

## Rationale

### Why a code-driven surface

Discovery-then-call over plain MCP costs one model round trip per tool interaction and forces intermediate tool results through the model's context window. `explore` lets the model filter and project search/describe results in-sandbox and return a small projection; `execute` lets it chain dependent tool calls without shipping intermediate payloads back through the context. This is the token-saving pattern Cloudflare's Code Mode established, and reusing their `code`-string shape and JSON envelope keeps the surface familiar to models already trained on it.

### Why a separate endpoint

Keeping `/code-mode-mcp` disjoint from `/mcp` keeps the aggregate surface small and predictable for clients that do not want code execution, lets deployments leave Code Mode disabled without affecting discovery, and gives the riskier capability its own reserved route, config gate, and audit identity.

### Backend selection

Wasmtime + a bespoke QuickJS guest was chosen as the first backend. The comparison:

| Option | Verdict | Reason |
| --- | --- | --- |
| Wasmtime + bespoke QuickJS guest (rquickjs/quickjs-ng ≥ 0.15.0 → `wasm32-wasip1`) | **Chosen** | Mature wasm isolation boundary, async host imports (`func_wrap_async`) bridge directly into the async `HostDispatcher`, full control over the guest's imports (zero WASI capabilities), current quickjs-ng. |
| Extism | Rejected | Host functions are blocking-only, which cannot bridge into the async grant/credential/upstream path without dedicating threads; its QuickJS plugin tracked a stale quickjs-ng pin. |
| Deno subprocess | Rejected | A subprocess running V8 with Deno's permission flags is not a sufficient boundary for hostile code on its own; it needs stronger outer isolation (gVisor/Firecracker-class), which is heavier operational machinery than this slice warrants. |
| Build-time componentizers (StarlingMonkey/ComponentizeJS, componentize-qjs, Javy CLI) | Rejected | Wrong shape: they compile known JS to wasm at build time, while Code Mode evaluates arbitrary model-authored JS at runtime. |
| In-process JS engines (rquickjs, Boa, deno_core/V8 embedded directly) | Rejected | An in-process engine shares the gateway's address space; an engine bug is a gateway compromise. Insufficient as the sole security boundary. |
| Young agent-sandbox crates | Rejected | Not mature enough to be trusted as a security boundary for hostile code. |

The executor abstraction means a second backend (for example a hardened subprocess runtime) can be added later behind `sandbox_backend` without route or policy changes.

### Logging both explore and execute

Aggregate `search_tools`/`describe_tool` calls are not logged as invocations, but Code Mode `explore` is. This deviation is deliberate: explore runs model-authored code, and operators need an audit row for every code execution regardless of whether it called upstream tools.

### Budget non-integration

MCP tool execution has never carried provider cost in Oceans' accounting: the usage ledger records LLM provider spend, and tool calls are observability events. Code Mode does not change that. Adding budget prechecks or ledger rows for zero-cost executions would invent a price for something Oceans does not bill, while the agent's actual spend remains fully attributed to the LLM requests driving it.

### No legacy HTTP+SSE fallback

`/code-mode-mcp` is a new gateway-owned route with no existing client base. Adding the legacy transport would preserve an older pattern this project has already declined for `/mcp` and would expand the attack/maintenance surface of a security-sensitive endpoint.

## Trade-Offs

- The QuickJS guest has no event loop, so `setTimeout`, real concurrency, and streaming inside the sandbox do not exist. `Promise.all` over `oceans.*` calls works but executes sequentially. Acceptable: host calls are the only async-shaped operations, and the synchronous-completion model is documented in the tool descriptions.
- The checked-in `.wasm` artifact adds a build-drift surface, managed by the `code-mode-guest-check` CI gate.
- Wall-clock timeouts around host calls mean a slow upstream tool consumes the whole execution budget; per-host-call deadlines are left to the upstream client timeouts that already exist.
- Hard-pinned `wasmtime`/`rquickjs` versions trade automatic updates for deliberate, reviewed bumps; this requires active GHSA tracking.
- Epoch-based preemption is approximate (tick granularity) rather than deterministic.

## Follow-Ups

- Per-tenant/per-team Code Mode enablement; the current `mcp.code_mode` gate is global-only.
- Generated typed per-tool wrappers over `oceans.*` so models get concrete function signatures instead of generic `callTool` addresses.
- Optional fuel-based metering for deterministic execution budgets where epoch wall-clock preemption is too coarse.
