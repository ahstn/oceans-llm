# End-to-End Contract Tests

`Owns`: the E2E harness shape, scope rules, and extension rules for cross-layer contract coverage.
`Depends on`: [admin-control-plane.md](admin-control-plane.md), [model-routing-and-api-behavior.md](model-routing-and-api-behavior.md)
`See also`: [../crates/admin-ui/web/e2e/](../crates/admin-ui/web/e2e/), [../mise.toml](../mise.toml)

The E2E harness boots three real processes:

- a deterministic OpenAI-compatible stub upstream
- the built admin UI SSR server
- the real gateway, with `/admin/*` served through the gateway proxy

Run it locally with:

```bash
mise run e2e-test
```

## Fixed Test Credentials

The harness uses deterministic seed values:

- bootstrap admin email: `admin@local`
- bootstrap admin password: `admin`
- bootstrap admin replacement password: `s3cur3-passw0rd`
- seed gateway API key: `gwk_e2e.secret-value`

## Scope Rule

Treat the harness as a contract suite for live gateway-backed flows.

Current intended coverage:

- admin auth and session behavior
- password rotation
- live `/v1/*` request handling
- additional admin flows only when the page is backed by a real gateway contract

## Covered Today

The current suite already covers more than a browser-only smoke pass:

- browser auth and forced password-rotation flow
- public `/v1/models`
- public `/v1/chat/completions`
- live spend report API behavior
- team hard-limit enforcement for team-owned keys
- strict `404` behavior for missing request-log detail
- live identity-user create-and-list API coverage

Planned contract coverage is now expected for:

- user lifecycle management
- team member removal and transfer

## Preview-Backed Surface Rule

Preview-backed pages may appear in smoke or landing assertions, but they are not treated as business-flow coverage until the underlying data is live.

Today that matters for:

- API Keys
- Models

Those pages still use local preview data in the admin UI. See [admin-control-plane.md](admin-control-plane.md).

## Extension Rule

When adding new browser scenarios:

- prefer one critical cross-layer flow per newly live surface
- keep the suite contract-focused rather than broad UI regression coverage
- avoid treating mock or preview-only pages as durable product workflows
- assert invalid transitions directly at the HTTP boundary when that is the clearest contract

## Coverage Shape

The harness is intentionally mixed:

- browser-admin flows for same-origin control-plane behavior
- raw API contract assertions for gateway and admin endpoints

Generated admin contract maintenance belongs in the same durability bucket:

- refresh artifacts with `mise run admin-contract-generate`
- verify drift with `mise run admin-contract-check`
- keep E2E assertions aligned with the checked-in gateway contract for live surfaces

That split is intentional because some critical contracts are better asserted directly at the HTTP boundary than through page interactions.

## Still Missing

- password invite completion coverage
- richer request-log filtering flows as that surface hardens
