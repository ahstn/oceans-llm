# Implementation Plan: Code Mode MCP Entrypoint (`/code-mode-mcp`)

Issue: [#172](https://github.com/ahstn/oceans-llm/issues/172)
Status: agreed plan, ready for implementation
Related ADRs: aggregate-mcp-gateway, mcp-tool-grants-and-token-overhead,
mcp-upstream-credential-bindings-and-execution, external-mcp-registry-and-discovery

## Agreed Decisions (resolved during planning)

| # | Decision | Resolution |
|---|----------|------------|
| 1 | Budget integration | **Out of scope.** MCP execution has zero budget integration today and Code Mode has no provider cost. No precheck, no ledger rows. Nested calls get invocation logging with parent linkage only. Issue acceptance criteria mentioning budget docs are satisfied by cross-link/accuracy work only. |
| 2 | Backend scope | **Ship both milestones in this issue**: executor abstraction + deterministic test executor, then the first real sandbox backend. May land as two sequenced PRs off this plan. |
| 3 | Sandbox backend | **Wasmtime + bespoke QuickJS guest** (`rquickjs` with quickjs-ng >= 0.15.0, compiled to `wasm32-wasip1`, zero WASI capabilities linked). Extism rejected (blocking-only host functions, stale quickjs-ng pin). Build-time componentizers (StarlingMonkey/ComponentizeJS, componentize-qjs, Javy CLI) rejected (wrong shape: Code Mode evaluates arbitrary JS at runtime). Young agent-sandbox crates rejected (not mature enough to be a security boundary). |
| 4 | Tool semantics | Both `explore` and `execute` take a single required `code` string ("JavaScript async arrow function"), matching the Cloudflare shape. Capability profiles: `explore` = `oceans.searchTools` + `oceans.describeTool`; `execute` = those + `oceans.callTool`. One sandbox path, two capability sets. |
| 5 | Config | New global-only `mcp.code_mode` YAML block, disabled by default. Disabled/unconfigured => `/code-mode-mcp` returns 404 (same posture as unknown server keys). Per-tenant/team enablement is a documented follow-up. |
| 6 | Invocation logging | Parent invocation rows for **both** `explore` and `execute` (they run model-authored code; deviates deliberately from search/describe-don't-log). Synthetic identity: `server_display_key = "code-mode"`, `tool_display_key = "explore" \| "execute"`. New nullable self-referencing `parent_invocation_id` column via migration **V32**, exposed as an admin observability list filter. |
| 7 | Crate layout | Abstraction in `crates/gateway-service/src/mcp_code_mode.rs`; concrete backend in new crate `crates/gateway-code-mode-wasmtime`; protocol helpers in `gateway-mcp`; HTTP route module `crates/gateway/src/http/mcp_gateway/code_mode.rs` mirroring `aggregate.rs`. |
| 8 | Guest artifact | Guest Rust crate compiled to `wasm32-wasip1`; compiled `.wasm` checked in; `mise run code-mode-guest-build` rebuild task + CI drift check (same pattern as `admin-contract-generate`/`-check`); gateway embeds via `include_bytes!`. |

## Architecture Summary

```
MCP client
  │  POST /code-mode-mcp   (Streamable HTTP, Oceans API key auth, aggregate-session semantics)
  ▼
crates/gateway/src/http/mcp_gateway/code_mode.rs
  - 404 when mcp.code_mode.enabled = false
  - auth (bearer / x-oceans-api-key), owner kind User|ServiceAccount
  - durable session (reuses mcp_aggregate_sessions machinery)
  - tools/list => explore, execute only
  - tools/call => CodeModeService
  ▼
crates/gateway-service/src/mcp_code_mode.rs
  - CodeModeService: builds HostDispatcher scoped to caller's auth subjects
  - CodeExecutor trait, ExecutionLimits, CapabilityProfile (Explore|Execute)
  - DeterministicTestExecutor (in-process, for route/service tests)
  - parent invocation logging + nested-call counting + redaction
  ▼ (host dispatch: every call re-checks grants, resolves credentials host-side)
  ├─ oceans.searchTools  -> McpCatalog::search_tools          (existing)
  ├─ oceans.describeTool -> McpCatalog::describe_tool         (existing)
  └─ oceans.callTool     -> call_catalog_tool path            (existing: grant re-check,
                            schema-hash preflight, credential resolution, upstream call,
                            invocation log + parent_invocation_id)
  ▲
crates/gateway-code-mode-wasmtime  (first real backend)
  - wasmtime Engine (Cranelift only), precompiled guest Module
  - fresh Store per execution, ResourceLimiter, epoch deadline,
    tokio::time::timeout around call_async
  - Func::wrap_async host import: oceans_call(ns, fn, args_json) -> {"result"|"error"} envelope
  - guest: crates/code-mode-guest (rquickjs/quickjs-ng >= 0.15.0 -> wasm32-wasip1,
    no WASI caps; exports evaluate(); captures console.* into logs)
```

Sandbox never sees: Oceans API keys, aggregate session IDs, upstream credentials,
encrypted blobs, `secret_ref` values, env vars, filesystem, processes, network.
The sandbox is never the policy engine; every host call is re-authorized.

### Executor contract (backend-neutral)

```rust
#[async_trait]
pub trait CodeExecutor: Send + Sync {
    /// Err = infrastructure failure only. Guest failures land in
    /// ExecutionOutcome.error (executors never throw for guest errors).
    async fn execute(
        &self,
        code: &str,
        dispatcher: Arc<dyn HostDispatcher>,
        limits: &CodeModeLimits,
    ) -> Result<ExecutionOutcome, ExecutorError>;
}

#[async_trait]
pub trait HostDispatcher: Send + Sync {
    /// namespace "oceans"; name searchTools|describeTool|callTool.
    /// Enforces capability profile, nested-call count, arg-size limits.
    async fn call(&self, namespace: &str, name: &str, args: serde_json::Value)
        -> Result<serde_json::Value, HostCallError>;
}

pub struct ExecutionOutcome {
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
    pub logs: Vec<String>,
    pub truncated: bool,
}
```

Guest <-> host boundary uses the Cloudflare-compatible JSON envelope
(`{"result": ...}` / `{"error": "..."}`); host-call errors surface as ordinary
catchable guest exceptions. Stable structured tool errors (`credential_required`,
`credential_expired`, `tool_schema_changed`, policy denials) pass through the
envelope unchanged so Code Mode callers see the same contracts as aggregate
`call_tool`.

### Config contract

```yaml
mcp:
  code_mode:
    enabled: false                      # default; 404 route when false/absent
    sandbox_backend: wasmtime_quickjs   # only production value
    limits:
      execution_timeout_ms: 30000       # wall clock, includes host calls
      memory_limit_bytes: 67108864      # 64 MiB, ResourceLimiter
      max_output_bytes: 32768           # truncate with explicit marker
      max_log_lines: 100
      max_log_bytes: 16384
      max_host_calls: 50                # nested host-dispatch count
```

Validation: reject unknown `sandbox_backend`; reject zero/negative limits;
the deterministic test executor is not selectable from YAML (test-only wiring).

## Milestone 1 - Executor abstraction, route, config, logging (test executor)

1. **Config** (`crates/gateway/src/config.rs` + config reference docs)
   - `mcp.code_mode` block per contract above; defaults + validation + unit tests.
2. **Migration V32** (`crates/gateway-store/migrations/V32__mcp_code_mode_invocation_linkage.sql` + postgres twin)
   - `ALTER TABLE mcp_tool_invocations ADD COLUMN parent_invocation_id` (nullable,
     self-referencing); index for the admin filter. Update store read/write paths
     and `McpInvocationLogInput`/`McpToolInvocationRecord`.
3. **Service layer** (`crates/gateway-service/src/mcp_code_mode.rs`)
   - `CodeExecutor`, `HostDispatcher`, `CodeModeLimits`, `CapabilityProfile`,
     `ExecutionOutcome`, `DeterministicTestExecutor`.
   - `OceansHostDispatcher`: wraps `McpCatalog`, `McpAccess`, `McpGatewayService`,
     `McpCredentialService`, `McpInvocationLogging`; enforces capability profile,
     `max_host_calls`, arg-size cap; logs nested invocations with
     `parent_invocation_id`; reuses `redaction.rs` for logs/errors/diagnostics.
4. **Protocol** (`crates/gateway-mcp/src/server.rs`)
   - `explore`/`execute` tool definitions (single required `code` param; descriptions
     embed the `oceans.*` API typings + worked examples, Cloudflare-style);
     result helpers (text content; `isError: true` + `"Error: ..."`;
     `--- TRUNCATED ---` marker convention).
5. **Route** (`crates/gateway/src/http/mcp_gateway/code_mode.rs` + `http/mod.rs`)
   - Mount `/code-mode-mcp` for POST/DELETE (GET => 405, query strings rejected,
     matching aggregate). 404 when disabled. Reuse aggregate session create/
     validate/touch machinery against `mcp_aggregate_sessions` (sessions remain
     bound to the authenticated API key; cross-principal reuse => not found).
   - Route reservation: extend `normalize_mcp_server_key` validation or registry
     create/update to reject reserved keys (`code-mode-mcp`, and document `mcp`)
     so an upstream registration can never collide with gateway-owned surfaces.
   - Parent invocation logging for both tools (status: success | timeout |
     gateway_error | policy_denied | invalid_request).
6. **Admin observability**
   - `parent_invocation_id` in invocation list/detail responses + list filter.
   - Regenerate admin contract (`mise run admin-contract-generate` / `-check`).
7. **Tests (milestone 1 matrix, deterministic executor)**
   - disabled-by-default => 404; enabled => initialize/tools-list works
   - GET 405, query-string rejection, bad auth, session binding/cross-principal 404
   - `tools/list` exposes exactly `explore` + `execute`
   - explore: grant-scoped results only; no global-inventory leakage via counts,
     names, errors, or schemas; denied describe for ungranted address
   - execute: nested call happy path; denied execution; duplicate upstream tool
     names; schema-hash mismatch => `tool_schema_changed`; missing/expired
     credential => `credential_required`/`credential_expired`
   - `max_host_calls` exceeded; oversized output truncation; log caps
   - parent + nested invocation rows linked via `parent_invocation_id`;
     payload redaction in nested logs
   - registry rejects reserved server keys

## Milestone 2 - Wasmtime QuickJS backend

1. **Guest crate** (`crates/code-mode-guest`, target `wasm32-wasip1`)
   - `rquickjs` (quickjs-ng >= 0.15.0). Exports `evaluate`; imports
     `oceans_call(ns_ptr, fn_ptr, args_ptr) -> result_ptr` (JSON envelope).
   - Wraps caller code as an async arrow function; every `await oceans.*()`
     completes synchronously from the guest's view (no event loop - document
     this in tool descriptions). Captures `console.log/warn/error` into the
     outcome `logs`. Guest serializes `{result|error}` explicitly - never rely
     on promise-rejection plumbing for error reporting.
   - Checked-in artifact `crates/gateway-code-mode-wasmtime/guest/code_mode_guest.wasm`;
     `mise run code-mode-guest-build` + CI drift check (`code-mode-guest-check`).
2. **Backend crate** (`crates/gateway-code-mode-wasmtime`)
   - Engine: Cranelift only (never Winch for hostile code), epoch interruption
     enabled, `Module` precompiled once at startup from `include_bytes!`.
   - Per execution: fresh `Store`, `StoreLimits` (memory cap, instances=1,
     tight table limits), `max_wasm_stack` set, epoch deadline ticker, host
     import registered via `Func::wrap_async` bridging directly into the async
     `HostDispatcher`, whole `call_async` wrapped in
     `tokio::time::timeout(execution_timeout_ms)` (covers host-call hangs that
     epochs cannot interrupt).
   - No WASI context linked at all - the only import is `oceans_call`.
   - Map traps: `Trap::OutOfFuel`/epoch => timeout outcome; memory-growth denial
     => resource-limit outcome; guest panic => gateway_error with redacted detail.
3. **Wiring**: `sandbox_backend: wasmtime_quickjs` selects this executor at
   startup; startup fails loudly if the backend is selected but the guest
   module fails validation.
4. **Tests (milestone 2 matrix, real sandbox)**
   - `while(true){}` preempted by epoch deadline => timeout
   - memory-bomb allocation => resource-limit error, host unaffected
   - no ambient capabilities: `fetch`/network unavailable; no env; no fs; no
     process; `import`/`require` unavailable
   - host-call hang simulated => wall-clock timeout still fires
   - console capture, output truncation marker, log caps in real guest
   - envelope error => catchable guest exception (try/catch over a denied callTool)
   - end-to-end: explore code filters describe results in-sandbox and returns
     a small projection (the token-saving pattern)
   - concurrency: parallel executions isolated (fresh stores, no state bleed)
5. **Dependency hygiene**: workspace-pin `wasmtime` and `rquickjs`; note GHSA
   subscription requirement in the ADR (wasmtime had two critical sandbox-escape
   advisories patched April 2026).

## Milestone 3 - Documentation and ADR

**ADR** (`docs/adr/2026-06-XX-code-mode-mcp-entrypoint.md`)
- Decision: separate gateway-owned Code Mode MCP surface at reserved
  `/code-mode-mcp`; `explore`/`execute` code-driven shape; executor abstraction
  with capability-profiled host dispatch; Wasmtime+QuickJS-guest as first
  backend (full comparison table incl. rejected options: Extism, Deno,
  componentizers, in-process JS engines, young sandbox crates); disabled-by-
  default config contract; parent/child invocation logging with V32 linkage;
  budget non-integration rationale (MCP execution carries no provider cost;
  spend stays attributed to the LLM requests that drive the agent); why legacy
  HTTP+SSE fallback remains out of scope; follow-ups (per-tenant enablement,
  typed generated wrappers, fuel-based determinism option).

**User-facing docs** (no operator internals)
- `docs/setup/mcp-client-setup.md`: `/code-mode-mcp` endpoint, auth/session
  contract (same as `/mcp`), the `code` parameter shape, `oceans.*` API with
  examples, address format, how explore differs from `/mcp` `search_tools`.
- `docs/access/mcp-tool-access.md`: Code Mode sees only granted tools; every
  nested call re-checks grants; credential-required/expired behavior identical
  to aggregate `call_tool`.
- `docs/configuration/mcp-servers.md`: route map gains `/code-mode-mcp`;
  reserved-key rule for registrations.
- `docs/access/budgets.md`: **accuracy pass + cross-link only.** Fix the noted
  drift (hard-budget wording vs post-provider enforcement reality in
  `budget_guard.rs:53-103`); confirm taxonomy (user / service-account /
  user-model), setup flow, and per-model budget docs match code; add one line:
  MCP/Code Mode tool calls are not budgeted spend.
- `README.md` documentation map entry if a new doc page is added.

**Developer/operator docs**
- `docs/configuration/configuration-reference.md`: full `mcp.code_mode` block.
- `docs/operations/observability/mcp-invocations.md`: parent rows for
  explore/execute, synthetic `code-mode` identity, `parent_invocation_id`
  filter, statuses, redaction guarantees.
- `docs/operations/observability/mcp-registry-and-discovery.md`: data-plane
  route list gains `/code-mode-mcp`; reserved-key registry behavior.
- `docs/reference/data-relationships.md`: `parent_invocation_id` self-reference;
  refresh stale See-Also links to the 2026-06-09 MCP ADRs (noted drift).
- `docs/operations/budgets-and-spending.md`: cross-link only - one paragraph
  stating MCP invocation logging (incl. Code Mode parent/child rows) is
  observability, not spend; usage ledger remains the only budget source.

## Entropy / cleanup commitments (no new fallbacks)

- No legacy HTTP+SSE support on `/code-mode-mcp` (Streamable HTTP only, same
  as aggregate; reaffirmed in ADR).
- No "compat" path exposing Code Mode tools on `/mcp` - the surfaces stay
  disjoint.
- Single executor selection path: YAML `sandbox_backend` only; no env-var
  override shim.
- Reserved-key validation added properly in registry validation, not as a
  route-layer special case.
- Fix the stale `data-relationships.md` See-Also links and the
  `docs/access/budgets.md` hard-budget wording drift found during recon.
- Aggregate-session reuse must be by extraction/sharing of the existing
  machinery (move shared helpers into a module both routes use), not
  copy-paste of `aggregate.rs`.

## Verification

- `mise run lint` (mixed Rust changes) and `mise run test`
- `mise run admin-contract-check` after observability API changes
- `mise run e2e-test` for the contract suite
- New `mise run code-mode-guest-check` in CI for guest artifact drift
- Manual smoke: enable in `gateway.yaml`, connect an MCP client, run an
  explore + execute round trip against the seeded demo dataset

## Explicit non-goals (this issue)

- Budget precheck/ledger rows for Code Mode executions
- Per-tenant/per-team enablement
- Generated typed per-tool wrappers over `oceans.*`
- CLI, OpenAPI/GraphQL sources, browser OAuth/token-refresh UX
- Deno or any second sandbox backend
