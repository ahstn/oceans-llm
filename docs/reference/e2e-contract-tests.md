# End-to-End Contract Tests

`See also`: [Admin Control Plane](../access/admin-control-plane.md), [Admin API Contract Workflow](admin-api-contract-workflow.md)

The E2E harness boots three real processes:

- a deterministic OpenAI-compatible stub upstream
- the built admin UI SSR server
- the real gateway, with `/admin/*` served through the gateway proxy

Run it locally with:

```bash
mise run e2e-test
```

## Why This Harness Exists

The product is same-origin and cross-layer by design.

That means a pure browser suite would miss backend contract breaks, and a pure HTTP suite would miss same-origin auth and SSR behavior. The harness exists because the real failure surface sits across both.

## Fixed Test Credentials

The harness uses deterministic seed values:

- bootstrap admin email:
  - `admin@local`
- bootstrap admin password:
  - `admin`
- replacement password:
  - `s3cur3-passw0rd`
- seeded gateway API key:
  - `gwk_e2e.secret-value`

Those values match the bootstrap and seed assumptions in the test stack.

## Scope Rule

Treat the harness as a contract suite for live gateway-backed flows.

Current intended coverage:

- admin auth and session behavior
- password rotation
- live `/v1/*` request handling
- additional admin flows only when the page is backed by a real gateway contract

## Covered Today

The current suite already covers:

- browser auth and forced password-rotation flow
- current-session logout and revoked-session redirect behavior
- public `/v1/models`
- public `/v1/chat/completions`
- public `/v1/responses`
- admin UI API-key create, live use, and revoke
- live spend report API behavior
- team hard-limit enforcement for team-owned keys
- strict `404` behavior for missing request-log detail
- live identity-user create-and-list API coverage

## Preview-Backed Surface Rule

Preview-backed pages may appear in smoke or landing assertions, but they are not treated as business-flow coverage until the underlying data is live.

Today that matters for:

- Models

## Browser Flow Versus HTTP Assertion

Use a browser flow when the contract depends on:

- same-origin auth behavior
- SSR loader behavior
- user-visible workflow sequencing

Use a direct HTTP assertion when the contract is clearer at the boundary:

- exact status codes
- invalid transitions
- response envelope shape
- admin API contract drift

## Generated Contract Boundary

Generated admin contract maintenance belongs in the same durability bucket.

- refresh artifacts with `mise run admin-contract-generate`
- verify drift with `mise run admin-contract-check`
- keep E2E assertions aligned with the checked-in contract for live surfaces

## Still Missing

- password invite completion coverage
- richer request-log filtering flows as that surface hardens
