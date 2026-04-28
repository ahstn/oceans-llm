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
- admin Models data loading
- admin UI API-key create, live use, and revoke
- live spend report API behavior
- team hard-limit enforcement for team-owned keys
- strict `404` behavior for missing request-log detail
- live identity-user create-and-list API coverage

## Maturing Surface Rule

Live pages may appear in smoke or data-loading assertions before every workflow is covered end to end.

Today that matters for:

- Models

The Models page is gateway-backed, but route/provider detail and Responses capability visibility still have follow-up work. Treat it as live data-loading coverage until those workflows harden.

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

## Extension Mechanics

Source files:

- Playwright config:
  - [../crates/admin-ui/web/playwright.config.ts](../../crates/admin-ui/web/playwright.config.ts)
- E2E specs:
  - [../crates/admin-ui/web/e2e/](../../crates/admin-ui/web/e2e)
- stack launcher:
  - [../scripts/start-e2e-stack.sh](../../scripts/start-e2e-stack.sh)
- deterministic upstream:
  - [../scripts/mock-openai-upstream.mjs](../../scripts/mock-openai-upstream.mjs)

Useful environment knobs:

- `E2E_GATEWAY_PORT`
- `E2E_UI_PORT`
- `E2E_UPSTREAM_PORT`
- `E2E_BASE_URL`
- `E2E_GATEWAY_API_KEY`
- `E2E_ADMIN_EMAIL`
- `E2E_ADMIN_PASSWORD`
- `E2E_ADMIN_NEW_PASSWORD`

Playwright writes reports under `crates/admin-ui/web/playwright-report` and failure artifacts under `crates/admin-ui/web/test-results`.

When adding a live admin surface:

- add or update the gateway API contract first
- regenerate checked-in admin contract artifacts if the admin API shape changed
- extend the deterministic E2E config or mock upstream only as needed
- prefer direct HTTP assertions for envelope/status semantics
- use browser assertions for auth, SSR loader behavior, and visible workflow sequencing
- keep preview-only UI behavior out of business-flow assertions

## Still Missing

- password invite completion coverage
- richer request-log filtering flows as that surface hardens
