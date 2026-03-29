# Admin API Contract Workflow

`Owns`: the generated admin API contract pipeline, checked-in artifacts, same-origin client boundary, drift rules, and the maintainer update flow when admin APIs change.
`Depends on`: [admin-control-plane.md](admin-control-plane.md), [e2e-contract-tests.md](e2e-contract-tests.md)
`See also`: [../README.md](../README.md), [../mise.toml](../mise.toml), [../crates/gateway/openapi/admin-api.json](../crates/gateway/openapi/admin-api.json), [../crates/admin-ui/web/src/generated/admin-api.ts](../crates/admin-ui/web/src/generated/admin-api.ts), [adr/2026-03-28-generated-admin-api-contract-and-typed-same-origin-client.md](adr/2026-03-28-generated-admin-api-contract-and-typed-same-origin-client.md), [adr/2026-03-29-live-admin-api-key-management-and-contract-coverage.md](adr/2026-03-29-live-admin-api-key-management-and-contract-coverage.md)

This page is maintainer-facing. It explains how the live admin contract is generated and why the checked-in artifacts are part of the review surface.

## Source of Truth

- contract DTOs and OpenAPI document: [../crates/gateway/src/http/admin_contract.rs](../crates/gateway/src/http/admin_contract.rs)
- route annotations: [../crates/gateway/src/http/identity.rs](../crates/gateway/src/http/identity.rs), [../crates/gateway/src/http/spend.rs](../crates/gateway/src/http/spend.rs), [../crates/gateway/src/http/observability.rs](../crates/gateway/src/http/observability.rs), [../crates/gateway/src/http/api_keys.rs](../crates/gateway/src/http/api_keys.rs)
- OpenAPI export binary: [../crates/gateway/src/bin/export_admin_openapi.rs](../crates/gateway/src/bin/export_admin_openapi.rs)
- generated artifact: [../crates/gateway/openapi/admin-api.json](../crates/gateway/openapi/admin-api.json)
- generated TypeScript types: [../crates/admin-ui/web/src/generated/admin-api.ts](../crates/admin-ui/web/src/generated/admin-api.ts)
- same-origin client: [../crates/admin-ui/web/src/server/gateway-client.server.ts](../crates/admin-ui/web/src/server/gateway-client.server.ts)

## Contract Boundary

The live admin contract belongs to the gateway HTTP layer.

- Rust handler annotations and transport DTOs define the contract
- the generated OpenAPI file is the reviewable snapshot
- the admin UI consumes generated types directly for live surfaces

The contract does not live in `gateway-core`. `gateway-core` owns domain types and repository traits, not the HTTP wire contract.

## Same-Origin Client Boundary

The admin UI is same-origin by design.

- the gateway serves `/admin`
- server-side admin loaders call back into the gateway
- cookies and forwarded origin state pass through the same-origin client layer

That is why a backend contract change can break the admin UI even when a page component did not change.

## Generated Artifacts

The checked-in artifacts are:

- OpenAPI document:
  - `crates/gateway/openapi/admin-api.json`
- generated TypeScript types:
  - `crates/admin-ui/web/src/generated/admin-api.ts`

These are not throwaway build files. They are part of the live contract review surface.

## Update Flow

When a live admin API changes:

1. update the gateway handler DTOs or route annotations
2. regenerate the artifacts
3. update the admin UI code if the wire contract changed
4. run contract drift checks
5. update operator docs if the user-visible behavior changed

Commands:

```bash
mise run admin-contract-generate
mise run admin-contract-check
```

## Drift Rules

Drift is treated as a failure, not a suggestion.

- checked-in OpenAPI must match the gateway HTTP source
- checked-in TypeScript must match the OpenAPI artifact
- CI and local lint both enforce this

The normal drift guard is `mise run admin-contract-check`.

## Live Versus Preview-Backed Surfaces

Only live gateway-backed surfaces should use this contract pipeline.

Current live surfaces:

- auth and session flows
- API keys
- identity
- spend
- request logs

Current preview-backed surface:

- Models

That split matters when deciding whether a UI page belongs in this workflow or in preview-only code.

## API-Key Architecture Note

Recent API-key work tightened the control-plane boundary.

- runtime auth stays in `ApiKeyRepository`
- admin lifecycle moved into `AdminApiKeyRepository`
- lifecycle policy moved into [../crates/gateway-service/src/admin_api_keys.rs](../crates/gateway-service/src/admin_api_keys.rs)

That split keeps a security-sensitive control-plane feature from living as optional runtime-auth behavior.

## What To Update When Behavior Changes

Use this rough rule:

- route shape, query params, or response envelopes changed:
  - update this page
- admin UI capability changed:
  - update [admin-control-plane.md](admin-control-plane.md)
- test coverage rules changed:
  - update [e2e-contract-tests.md](e2e-contract-tests.md)
- architectural reasoning changed:
  - update the related ADR and link back here

## What This Page Does Not Own

- operator-facing UI capability map: [admin-control-plane.md](admin-control-plane.md)
- browser and HTTP contract test scope: [e2e-contract-tests.md](e2e-contract-tests.md)
- local quick-start commands: [../README.md](../README.md)
