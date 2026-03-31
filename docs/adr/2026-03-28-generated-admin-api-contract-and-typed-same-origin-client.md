# ADR: Generated Admin API Contract and Typed Same-Origin Client

- Date: 2026-03-28
- Status: Accepted
- Related Issues:
  - [#60](https://github.com/ahstn/oceans-llm/issues/60)
- Builds On:
  - [2026-03-05-identity-foundation.md](2026-03-05-identity-foundation.md)
  - [2026-03-15-otlp-observability-and-request-log-payloads.md](2026-03-15-otlp-observability-and-request-log-payloads.md)
  - [2026-03-15-spend-control-plane-reporting-and-team-hard-limits.md](2026-03-15-spend-control-plane-reporting-and-team-hard-limits.md)
  - [2026-03-17-post-success-accounting-and-strict-request-log-lookups.md](2026-03-17-post-success-accounting-and-strict-request-log-lookups.md)
  - [2026-03-26-admin-identity-lifecycle-and-team-member-workflows.md](2026-03-26-admin-identity-lifecycle-and-team-member-workflows.md)

## Current state

- [../admin-api-contract-workflow.md](../reference/admin-api-contract-workflow.md)
- [../admin-control-plane.md](../access/admin-control-plane.md)
- [../e2e-contract-tests.md](../reference/e2e-contract-tests.md)

## Context

The admin control plane had reached the point where the gateway owned the real behavior, but the contract between the gateway and the admin UI was still partly hand-maintained. That created a familiar failure mode:

- handler signatures and frontend request shapes could drift apart,
- route documentation and client code could silently disagree about query params or response envelopes,
- the observability UI had already accumulated a local camelCase view model that did not match the backend wire contract,
- preview-backed screens and live gateway-backed screens were mixed together in a way that made the boundary hard to reason about.

Issue [#60](https://github.com/ahstn/oceans-llm/issues/60) asked for a stronger contract story for the live admin surfaces. The goal was not just type export. It was a durable contract for the REST boundary itself: paths, query parameters, request bodies, response bodies, and a thin client that still behaves like the existing same-origin admin UI.

The important architectural constraint was that this contract belongs at the gateway HTTP boundary, not in `gateway-core`. `gateway-core` holds domain types and repositories. The admin contract is HTTP transport shape: what the UI sends, what the gateway returns, and how the public admin API is documented and generated.

## Decision

We adopted a code-first OpenAPI pipeline for the live admin control plane.

The key decisions are:

### 1. Make the gateway HTTP layer the source of truth

The canonical contract now lives in [../../crates/gateway/src/http/admin_contract.rs](../../crates/gateway/src/http/admin_contract.rs), with route annotations on the live handlers in the gateway HTTP layer.

Why:

- the gateway is the authority for the admin API surface,
- the HTTP boundary is where request and response semantics actually live,
- keeping transport DTOs in the gateway avoids polluting `gateway-core` with HTTP-only concerns,
- a single contract module makes drift easier to see and harder to ignore.

### 2. Generate the admin OpenAPI document from the gateway

The gateway exports a checked-in OpenAPI artifact at [../../crates/gateway/openapi/admin-api.json](../../crates/gateway/openapi/admin-api.json) through [../../crates/gateway/src/bin/export_admin_openapi.rs](../../crates/gateway/src/bin/export_admin_openapi.rs) and the new library target in [../../crates/gateway/src/lib.rs](../../crates/gateway/src/lib.rs).

Why:

- the gateway already owns the live route behavior,
- OpenAPI describes the full REST contract, not just Rust type shapes,
- the generated document becomes a reviewable artifact that can be diffed and checked in.

### 3. Generate the frontend contract from OpenAPI and consume it directly

The admin UI now consumes generated types from [../../crates/admin-ui/web/src/generated/admin-api.ts](../../crates/admin-ui/web/src/generated/admin-api.ts), with live-specific aliases in [../../crates/admin-ui/web/src/types/live-api.ts](../../crates/admin-ui/web/src/types/live-api.ts).

Why:

- the UI should not hand-author request/response types for live gateway endpoints,
- generated types keep the route layer and frontend aligned on path names, query keys, envelopes, and status-specific responses,
- the live UI should use the backend wire shape directly instead of maintaining a second local model.

### 4. Keep a thin same-origin client rather than inventing a new SDK

The admin UI uses [../../crates/admin-ui/web/src/server/gateway-client.server.ts](../../crates/admin-ui/web/src/server/gateway-client.server.ts) as the same-origin fetch adapter and `openapi-fetch` as the typed client surface.

Why:

- the existing admin UI relies on forwarded cookies, forwarded origin headers, and `set-cookie` passthrough,
- a thin adapter preserves those behaviors without introducing a custom RPC layer,
- the generated client remains close to `fetch`, which keeps the implementation easy to audit.

### 5. Split live and preview-backed data explicitly

Preview-only content moved to [../../crates/admin-ui/web/src/server/admin-preview-data.server.ts](../../crates/admin-ui/web/src/server/admin-preview-data.server.ts) and [../../crates/admin-ui/web/src/types/preview-api.ts](../../crates/admin-ui/web/src/types/preview-api.ts), while the live admin data layer now lives in [../../crates/admin-ui/web/src/server/admin-data.server.ts](../../crates/admin-ui/web/src/server/admin-data.server.ts) and [../../crates/admin-ui/web/src/server/admin-data.functions.ts](../../crates/admin-ui/web/src/server/admin-data.functions.ts).

Why:

- API Keys and Models remain preview-backed in this slice,
- the live contract should not be blurred by mock or preview paths,
- explicit separation makes future contract generation scope easier to reason about.

### 6. Remove compatibility shims instead of preserving them

The observability route now consumes the wire contract directly in [../../crates/admin-ui/web/src/routes/observability/request-logs.tsx](../../crates/admin-ui/web/src/routes/observability/request-logs.tsx), and the old camelCase remap layer was removed rather than retained behind fallback wrappers.

Why:

- dual models are a maintenance trap,
- compatibility shims delay the point at which the real contract becomes the only contract,
- the cleanest maintenance burden is one wire shape and one frontend representation for live surfaces.

## Implementation

This ADR is intentionally about both the process and the structure.

### Gateway contract generation

- [../../crates/gateway/src/http/admin_contract.rs](../../crates/gateway/src/http/admin_contract.rs)
  - defines reusable transport DTOs and envelopes for auth, identity, spend, and observability,
  - owns the `utoipa` OpenAPI document definition,
  - keeps live transport types close to the handlers that use them.
- [../../crates/gateway/src/http/identity.rs](../../crates/gateway/src/http/identity.rs)
- [../../crates/gateway/src/http/spend.rs](../../crates/gateway/src/http/spend.rs)
- [../../crates/gateway/src/http/observability.rs](../../crates/gateway/src/http/observability.rs)
  - annotate live routes with `#[utoipa::path]` and return the shared contract types.
- [../../crates/gateway/src/bin/export_admin_openapi.rs](../../crates/gateway/src/bin/export_admin_openapi.rs)
  - emits the checked-in OpenAPI artifact.
- [../../crates/gateway/openapi/admin-api.json](../../crates/gateway/openapi/admin-api.json)
  - becomes the stable input for frontend type generation.

This keeps the contract close to the live implementation and avoids a second contract source in shared domain code.

### Frontend generation and client wiring

- [../../crates/admin-ui/web/src/generated/admin-api.ts](../../crates/admin-ui/web/src/generated/admin-api.ts)
  - checked-in generated TypeScript path/operation types.
- [../../crates/admin-ui/web/src/types/live-api.ts](../../crates/admin-ui/web/src/types/live-api.ts)
  - live-facing aliases over the generated schema.
- [../../crates/admin-ui/web/src/server/gateway-client.server.ts](../../crates/admin-ui/web/src/server/gateway-client.server.ts)
  - same-origin header forwarding and cookie passthrough for the generated client.
- [../../crates/admin-ui/web/src/server/admin-data.server.ts](../../crates/admin-ui/web/src/server/admin-data.server.ts)
  - live admin endpoints backed by the generated client.
- [../../crates/admin-ui/web/src/routes/observability/request-logs.tsx](../../crates/admin-ui/web/src/routes/observability/request-logs.tsx)
  - now consumes the generated wire shapes directly.

### Drift control

Drift prevention is intentionally simple:

- generate the OpenAPI artifact and TypeScript output,
- check the generated files into the repository,
- fail CI and local lint if regeneration changes the working tree.

The tooling for that lives in [../../mise.toml](../../mise.toml) and the CI gate in [../../.github/workflows/rust-ci.yml](../../.github/workflows/rust-ci.yml).

This approach keeps the source of truth inspectable and makes contract drift a normal code review problem instead of a runtime surprise.

### Documentation updates

Canonical docs now describe the generated live contract pipeline and the boundary between live and preview-backed surfaces:

- [../../README.md](../../README.md)
- [../access/admin-control-plane.md](../access/admin-control-plane.md)
- [../operations/observability-and-request-logs.md](../operations/observability-and-request-logs.md)
- [../reference/e2e-contract-tests.md](../reference/e2e-contract-tests.md)

## Tradeoffs

This decision improves consistency, but it is not free.

- OpenAPI generation adds a build step and an artifact that must stay in sync.
- The contract now depends on route annotations being kept current when handlers change.
- Generated code is less flexible than hand-written wrappers when we want to experiment quickly.
- We accepted that cost because the live admin API is now important enough that type drift is more expensive than generation.

We also considered Rust-to-TypeScript exporters like `ts-rs` and `specta`, but they were not sufficient for this slice because they export types rather than a complete REST contract. The admin UI needed path, query, request body, response body, and client behavior all tied together. OpenAPI plus `openapi-typescript` plus `openapi-fetch` solves the whole boundary.

## Consequences

Positive:

- the gateway HTTP contract is now explicit and reviewable,
- the admin UI consumes generated live types instead of local copies,
- request/response envelopes and query keys are shared across the live admin surfaces,
- observability request-log data now uses the backend wire contract directly,
- contract drift is caught by tooling instead of by users.

Negative:

- contributors must regenerate checked-in artifacts when the API changes,
- route annotations and transport DTOs now carry more maintenance responsibility,
- the codebase has one more intentional generation workflow to understand.

## Follow-Up

The live contract scope in this slice intentionally excludes preview-backed `API Keys` and `Models`. If those screens later move onto the gateway contract, they should be added deliberately rather than folded in through a compatibility layer.

Future work should continue to keep the gateway as the only source of truth for live admin API semantics and resist reintroducing local view-model remaps in the UI.

This ADR was prepared through collaborative human + AI implementation and documentation work.
