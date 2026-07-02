# API Key All-Models Grant Mode

## Goal

Implement GitHub issue #204: allow user-owned API keys to grant all current and future gateway models while preserving explicit model grants for service-account and restricted-use workflows.

## Design

- Add a durable API-key model grant mode:
  - `all`: the API key starts from every current gateway model.
  - `explicit`: the API key starts from rows in `api_key_model_grants`.
- Do not infer `all` from an empty grant table. Existing keys must migrate to `explicit`.
- Keep team, user, service-account allowlists, routing viability, and budget behavior as independent intersections after the API-key grant mode is evaluated.
- Keep service-account keys explicit by default in the admin UI and service policy.

## Implementation Areas

- Domain and auth records:
  - `crates/gateway-core/src/domain.rs`
  - `crates/gateway-core/src/auth.rs`
- Store and migrations:
  - `crates/gateway-store/migrations/V35__api_key_model_grant_mode.sql`
  - `crates/gateway-store/migrations/postgres/V35__api_key_model_grant_mode.sql`
  - `crates/gateway-store/src/migration_registry.rs`
  - libsql/postgres API-key stores and decoders
- Runtime access:
  - `crates/gateway-service/src/model_access.rs`
  - `crates/gateway-service/src/admin_api_keys.rs`
- Admin API and generated contract:
  - `crates/gateway/src/http/api_keys.rs`
  - `crates/gateway/openapi/admin-api.json`
  - `crates/admin-ui/web/src/generated/admin-api.ts`
- Admin UI:
  - `crates/admin-ui/web/src/routes/api-keys/-use-api-keys-page.ts`
  - `crates/admin-ui/web/src/routes/api-keys/-components.tsx`
  - route/server/E2E tests
- Docs:
  - `docs/reference/data-relationships.md`
  - `docs/access/identity-and-access.md`
  - API-key ADR supersession note or new ADR

## Verification

Run the focused tests first, then full lint:

```bash
eval "$(/Users/ahstn/.local/bin/mise activate zsh)"
mise run admin-contract-generate
mise run admin-contract-check
cargo test -p gateway-store api_key
cargo test -p gateway-service model_access admin_api_keys
bun run --cwd crates/admin-ui/web test -- src/test/routes/api-keys-route.test.tsx src/server/admin-data.server.test.ts
mise run e2e-test
mise run lint
```
