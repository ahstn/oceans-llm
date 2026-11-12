
# Cleanup Command

Autonomous cleanup-loop iteration: discover ONE target → complete execution → verify → report. Runs stateless: derive from current tree; assume prior runs left it consistent.

<critical>
- Behavior-preserving ONLY: `gateway`/`oceans` CLI, HTTP API (inference and admin), `gateway.yaml` schema, database schema, serde wire shapes, rendered output NEVER change.
- Every iteration MUST yield a named concrete quality win: duplicate implementation gone, responsibility extracted, dead cluster removed, guard clutter deleted. Deletion favored: net-negative LOC expected and candidate tie-breaker; justified net-neutral/positive split or hierarchy fix acceptable only with real win. Report LOC delta either way.
- NEVER commit; NEVER touch generated or vendored code.
- Complete cutover this run: migrate every copy and callsite; delete originals. NEVER half-migrate.
- No target above bar → output exactly `CLEAN: no target above threshold` and stop.
</critical>

## Scope

Rust crates in `crates/` only. Crate priority, highest first:

1. `gateway-core`: shared domain every crate depends on; kind-grouped `domain.rs`/`traits.rs`, over-wide `pub`, so fixes pay off downstream.
2. `gateway-service`: largest business-logic crate; `service.rs`/`admin_models.rs` god-file candidates; most clone and pass-by-value hits.
3. `gateway-store`: parallel libSQL/Postgres implementations and `any_store_*` dispatch; dedup potential.
4. `gateway-providers`: per-provider adapters reimplementing shared request/stream handling; `openai_compat.rs`.
5. `gateway`: HTTP binary; `http/identity.rs`, `http/handlers.rs`, `http/inference_guardrails.rs` god-file candidates.
6. `gateway-guardrails`
7. `gateway-client-config`, `gateway-mcp`, `agent-session-analysis`, `gateway-skills`
8. `oceans-cli`, `gateway-client-ip`, `admin-ui` (Rust proxy only)

No crate is published to crates.io: workspace `pub` items are internal, and a change MUST migrate callsites in every dependent crate.

NEVER touch: `crates/admin-ui/web/**`, `crates/gateway/openapi/**` (generated), `crates/gateway-service/data/**` (vendored catalogs), `crates/gateway-store/migrations/**` (applied, checksummed), test fixtures/snapshots, `Cargo.lock`, non-Rust files except `Cargo.toml` dependencies the cleanup leaves unused.

## 1. Discover

No scanner exists. Gather evidence for the crate under review:

- File sizes (`wc -l` over `src/**/*.rs`, tests excluded): god-file candidates.
- `cargo clippy -p <crate> --all-targets -- -W clippy::redundant_clone -W clippy::needless_pass_by_value -W clippy::fn_params_excessive_bools -W clippy::unused_self -W clippy::ref_option -W clippy::match_same_arms -W clippy::unnecessary_wraps`: idiom hotspots. One-off scan only; NEVER add these lints to config.
- `lsp references` on `pub` items: dead or over-wide exports.
- Targeted grep: `#[allow(dead_code)]`, `#[allow(unused`, `pub use`, `.clone()`, `.unwrap()` outside tests.

Output is EVIDENCE, not verdict: read every entry before action.

Candidate classes:

**Dead weight** — highest value/risk
- Dead exports: `pub` items with zero non-test references. Used only inside their crate → narrow to `pub(crate)` or private; the compiler's `dead_code` lint then exposes truly dead items.
- `#[allow(dead_code)]`/`#[allow(unused…)]` hiding dead items.
- Unpassed options/parameters; unreachable branches.
- Compatibility shims, `#[deprecated]` items, `type` aliases, `pub use` re-export indirection from past refactors.
- Runtime checks duplicating type-system guarantees: re-validation of already-parsed values, `Option` fields always `Some` after construction.

**Duplication**
- Literal-heavy tables (migration registry, provider descriptors) and libSQL/Postgres pairs that differ by SQL dialect intentionally repeat; extract only if a helper genuinely simplifies every site.
- Helper reimplemented in 2+ files; copies differing only by literal/flag.
- Inline reimplementation of a helper a lower crate already owns: SSE/stream parsing, header handling, redaction, truncation, caching.
- Parallel `match` chains dispatching on one enum in multiple locations; belongs as one method on the enum.

**God objects**
- File dwarfs siblings AND mixes responsibilities: state + IO + rendering + parsing; or `impl` blocks span domains.
- Size alone no smell: retain large coherent files.

**Rust idiom debt** — fix inside the chosen cluster; a crate-wide sweep of one smell MAY itself be the target
- `.clone()` whose only job is to satisfy the borrow checker; use a borrow, `mem::take`, or a move.
- `&String`/`&Vec<T>`/`&Option<T>` parameters → `&str`/`&[T]`/`Option<&T>`; `&T` plus internal clone → take `T`.
- `bool` parameters on shared signatures → two-variant enum or two functions.
- `unwrap()` on non-test paths → `expect("why")`; NEVER convert a panic to `Err` here (behavior change).
- `get_` getter prefixes; `Manager`/`Helper`/`Util` names; module name repeated in type name.

**Hierarchy rot**
- Domainless junk drawers: `utils`, `helpers`, `misc`, `common` with unrelated accretions.
- `super::super::` chains or `crate::` paths reaching across domains: wrong module home.
- Modules grouped by kind (`types`, `traits`, `domain`, `errors`, `constants`) rather than domain.
- Non-leaf modules holding logic beyond `mod`/`pub use`; single-child module directories; names no longer describing contents.

## 2. Select

Score `(quality win × confidence) / blast radius`. Pick exactly ONE cluster, roughly ≤12 touched files. Tie-break: deletion > dedup > split > move; equals → larger LOC reduction.

Worth-doing bar: name win in one sentence, e.g. entire duplicate implementation or ≥100 duplicated/dead lines removed; oversized, multi-responsibility package file split; junk drawer, dead-export cluster, or guard-clutter hotspot eliminated; module cluster moved so tree reads designed, not accreted.

## 3. Execute

**Dead weight / type checks**
- Delete dead exports and tests only mirroring them. Export deletion requires `lsp references` across the workspace: no callsites—missed callsites are bugs. Items reachable from a public surface (CLI args, HTTP handlers, `utoipa` schemas, serde config/wire types) → retain.
- Parse once at IO boundary (serde, `TryFrom`, `FromStr`); internal code receives the typed value. Delete downstream re-validation, `unwrap_or_default()`/`unwrap_or(..)` on values that cannot be absent, and `as` casts papering over types.
- Genuinely sometimes-absent value → fix TYPE upstream; NEVER add downstream guards.
- Swallow-and-limp `.ok()`, `let _ =`, or `unwrap_or_default()` on `Result` → delete or propagate with `?`. Precise matches only, e.g. `io::ErrorKind::NotFound`.

**Dedup**
- 2+ copies → one function in nearest common domain module; cross-crate → domain module in the lowest common dependency crate. NEVER create a junk drawer.
- Literal/flag variants → one function taking an enum or options struct; NEVER `bool` parameters.
- Keep hardened copy—timeouts, caps, sanitization—not fresh copies that lack hardening.

**God objects**
- Split on existing seams into domain-named, single-responsibility modules.
- Extraction = MOVEMENT: code verbatim except `use` paths/visibility; rewriting while moving hides regressions.
- Update every `use` path; NEVER retain `pub use` re-export shim. Split introducing trait, generic, `dyn` object, channel, or builder where direct call existed = failed split.

**Hierarchy**
- Move the file and its `mod` declaration; rewrite every `use` path (compiler errors list them).
- Group by domain, not kind; name a leaf module after its primary item; collapse single-child module directories; delete empty re-export modules.
- Resulting tree MUST read as always designed.

**Perf** — opportunistic; only code already touched
- Hoist loop invariants; precompile regexes in `LazyLock`; `collect()` or `Vec::with_capacity` over push loops; remove intermediate `Vec`s/`String`s/clones.
- NEVER trade cold-path clarity for micro-perf; NEVER add caching layers.

## 4. Prohibitions

- NEVER add dependencies, config/options, Cargo features, wrapper layers, one-implementation traits, or "future-proofing".
- NEVER rename or alter public surface: `gateway`/`oceans` CLI, HTTP routes and JSON shapes, `utoipa` schema names, `gateway.yaml` keys, serde field names, database schema, metric and log field names.
- NEVER reformat/restyle outside touched cluster.
- NEVER drive-by comment/doc sweep; comment only new non-obvious code.
- NEVER add tests for moved-but-unchanged code; retain passing tests, relocating them with subject.

## 5. Verify

1. `mise run rust-fmt-check` and `mise run rust-lint`: clean.
2. `cargo test -p <crate>` for each touched crate, filtered to affected areas. Postgres store code touched → `mise run //store:test:postgres` (needs `mise run //store:postgres:start` and `TEST_POSTGRES_URL` from `mise run //store:postgres:env`).
3. `gateway` touched → `mise run admin-contract-check`: admin OpenAPI unchanged.
4. Run `.agents/skills/verify-oceans-admin/scripts/control-oceans-admin plan`; run every proof it maps.

## 6. Report

- Target: choice and smell class.
- Actions: deleted/merged/split/moved; named quality win; LOC delta.
- Verification: exact commands and results.
- Risk: reviewer checks.

<critical>
One target/run; complete migration; originals deleted; `mise run rust-lint` clean. Named quality win; identical behavior; no new abstractions or shims. Deletion-leaning: justify net-positive delta. Nothing above bar → `CLEAN: no target above threshold`.
</critical>
