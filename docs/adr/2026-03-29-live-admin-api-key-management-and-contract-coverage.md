# ADR: Live Admin API-Key Management and Contract Coverage

- Date: 2026-03-29
- Status: Accepted
- Related Issues:
  - [#26](https://github.com/ahstn/oceans-llm/issues/26)
  - [#31](https://github.com/ahstn/oceans-llm/issues/31)
- Builds On:
  - [2026-03-05-identity-foundation.md](./2026-03-05-identity-foundation.md)
  - [2026-03-15-v1-runtime-simplification.md](./2026-03-15-v1-runtime-simplification.md)
  - [2026-03-17-post-success-accounting-and-strict-request-log-lookups.md](./2026-03-17-post-success-accounting-and-strict-request-log-lookups.md)

## Context

Before this change, the control plane had an architectural split that was useful for early UI work but no longer acceptable for the product shape we are trying to keep stable:

- the API Keys page in [crates/admin-ui/web/src/routes/api-keys.tsx](../../crates/admin-ui/web/src/routes/api-keys.tsx) was read-only and backed by fixture data in [crates/admin-ui/web/src/server/admin-data.server.ts](../../crates/admin-ui/web/src/server/admin-data.server.ts),
- the gateway had no admin API-key routes even though the rest of the admin surface was same-origin and live,
- the store contract only supported runtime lookup and `last_used` mutation for API keys,
- the end-to-end harness from [#31](https://github.com/ahstn/oceans-llm/issues/31) existed already, but it did not yet cover the missing admin API-key lifecycle.

That left an awkward and fragile pattern in the codebase:

- runtime authentication treated API keys as real security objects,
- the admin control plane treated them as preview-only placeholders,
- docs had to explain that contradiction,
- future work would have been tempted to preserve the preview path with extra fallbacks instead of removing it.

We do not want two parallel truths for a security-sensitive control-plane surface. API keys must either be a real managed object or not appear as a live admin workflow at all.

That first implementation deliberately optimized for shipping the live workflow. It left one follow-up architectural weakness behind:

- the admin API-key lifecycle was still modeled as optional behavior on `ApiKeyRepository`,
- the HTTP module in [crates/gateway/src/http/api_keys.rs](../../crates/gateway/src/http/api_keys.rs) still owned too much validation and lifecycle policy,
- the API Keys page had become a large route file again as real create/revoke behavior arrived.

That shape worked, but it made the architecture too easy to erode. Optional repository methods become implicit fallbacks. Policy in the HTTP layer invites other handlers to repeat the same pattern. Large route files become default dumping grounds for future UI changes.

## Decision

We turned API-key management into a real same-origin control-plane contract and removed the preview path.

The decisions are:

### 1. API keys are managed through the gateway, not through UI fixtures

The authoritative admin API-key lifecycle now lives in:

- [crates/gateway/src/http/api_keys.rs](../../crates/gateway/src/http/api_keys.rs)
- [crates/gateway-service/src/admin_api_keys.rs](../../crates/gateway-service/src/admin_api_keys.rs)
- [crates/gateway/src/http/mod.rs](../../crates/gateway/src/http/mod.rs)

The admin UI now consumes that contract through:

- [crates/admin-ui/web/src/server/admin-data.server.ts](../../crates/admin-ui/web/src/server/admin-data.server.ts)
- [crates/admin-ui/web/src/server/admin-data.functions.ts](../../crates/admin-ui/web/src/server/admin-data.functions.ts)
- [crates/admin-ui/web/src/routes/api-keys.tsx](../../crates/admin-ui/web/src/routes/api-keys.tsx)

Why:

- API keys define data-plane access and must share the same source of truth as runtime auth,
- same-origin control-plane behavior is already the established pattern for identity, spend, and observability,
- removing fixture data avoids a second, stale contract that future changes would otherwise need to maintain.

### 1a. Admin API-key lifecycle is a dedicated repository concern, not an optional extension of runtime auth

The repository boundary is now explicit in [crates/gateway-core/src/traits.rs](../../crates/gateway-core/src/traits.rs):

- `ApiKeyRepository` is limited to runtime auth lookup and `last_used` mutation,
- `AdminApiKeyRepository` owns list, fetch-by-id, create, grant replacement, and revoke,
- `AdminIdentityRepository` provides the owner catalog needed by admin API-key workflows.

The composed store surface in [crates/gateway-store/src/store.rs](../../crates/gateway-store/src/store.rs) requires those traits directly, and both backends implement them in:

- [crates/gateway-store/src/libsql_store/api_keys.rs](../../crates/gateway-store/src/libsql_store/api_keys.rs)
- [crates/gateway-store/src/postgres_store/api_keys.rs](../../crates/gateway-store/src/postgres_store/api_keys.rs)
- [crates/gateway-store/src/libsql_store/mod.rs](../../crates/gateway-store/src/libsql_store/mod.rs)
- [crates/gateway-store/src/postgres_store/mod.rs](../../crates/gateway-store/src/postgres_store/mod.rs)

Why:

- runtime authentication and admin lifecycle are different responsibilities and should not share a soft optional contract,
- required trait methods remove the last API-key admin compatibility shim at compile time,
- this makes unsupported store implementations impossible instead of merely deferred to runtime.

### 2. The API-key lifecycle is intentionally narrow: create and revoke only

We explicitly did not add rename, secret re-reveal, update-in-place grants, or restore-from-revoked flows.

The backend contract is:

- `GET /api/v1/admin/api-keys`
- `POST /api/v1/admin/api-keys`
- `POST /api/v1/admin/api-keys/{api_key_id}/revoke`

Why:

- API keys are security credentials, not collaborative content objects,
- a smaller lifecycle is easier to reason about operationally and easier to audit,
- revocation remains final, which keeps runtime semantics simple and reduces accidental privilege restoration.

### 3. Secrets are generated server-side, stored hashed, and returned exactly once

The gateway generates the public identifier and secret, hashes the secret with the existing runtime hashing path, stores only the hash, and returns the full raw key once in the create response.

Relevant code:

- [crates/gateway/src/http/api_keys.rs](../../crates/gateway/src/http/api_keys.rs)
- [crates/gateway-service/src/admin_api_keys.rs](../../crates/gateway-service/src/admin_api_keys.rs)
- [crates/gateway-service/src/authenticator.rs](../../crates/gateway-service/src/authenticator.rs)

Why:

- the server is the trust boundary for credential creation,
- reuse of the existing hash/verify path avoids introducing a second credential format,
- “show once” removes the temptation to add secret-reveal storage or recovery shims later.

### 4. Ownership and grants must be explicit and validated at creation time

Every created key must have:

- one explicit owner kind,
- exactly one valid owner ID for that kind,
- at least one explicit granted gateway model.

Relevant code:

- [crates/gateway-service/src/admin_api_keys.rs](../../crates/gateway-service/src/admin_api_keys.rs)
- [crates/gateway-store/src/store.rs](../../crates/gateway-store/src/store.rs)
- [crates/gateway-store/src/libsql_store/api_keys.rs](../../crates/gateway-store/src/libsql_store/api_keys.rs)
- [crates/gateway-store/src/postgres_store/api_keys.rs](../../crates/gateway-store/src/postgres_store/api_keys.rs)

Why:

- implicit ownership is a long-term audit problem,
- implicit or inherited grants would make the operator contract harder to explain,
- create-time validation keeps bad state out of the database instead of tolerating it in the UI.

### 4a. API-key lifecycle policy belongs in a service layer, not in the HTTP handler

The control-plane policy is now centered in [crates/gateway-service/src/admin_api_keys.rs](../../crates/gateway-service/src/admin_api_keys.rs) through `AdminApiKeyService`.

The service owns:

- owner-kind parsing and owner existence checks,
- active-user and active-team validation,
- model grant validation,
- key material generation and secret hashing,
- payload assembly for list/create/revoke results,
- reload semantics after create and revoke.

The HTTP module in [crates/gateway/src/http/api_keys.rs](../../crates/gateway/src/http/api_keys.rs) is intentionally thin and now only owns:

- platform-admin session auth,
- request and path parsing,
- mapping service errors to HTTP errors,
- serialization into the existing wire DTOs.

Why:

- lifecycle policy should be testable without an HTTP harness,
- thin transport keeps other admin endpoints from copying business logic into handlers,
- the service boundary makes the architecture align with other gateway-service patterns.

### 5. API-key status is a typed domain concept

API-key status now lives in [crates/gateway-core/src/domain.rs](../../crates/gateway-core/src/domain.rs) as `ApiKeyStatus`, with store decoding and runtime auth checks updated accordingly.

Why:

- the gateway, store, and tests should not rely on stringly-typed status handling,
- typed status makes revoke semantics harder to accidentally weaken,
- this matches the direction already taken for other domain enums such as user lifecycle and budgets.

### 6. Contract coverage extends the existing E2E harness instead of creating a separate UI suite

The cross-layer coverage for [#31](https://github.com/ahstn/oceans-llm/issues/31) now includes the live API-key flow in:

- [crates/admin-ui/web/e2e/gateway-contract.e2e.ts](../../crates/admin-ui/web/e2e/gateway-contract.e2e.ts)

Why:

- the value of this feature is the end-to-end contract, not isolated browser behavior,
- the harness already boots the real gateway, admin UI, and deterministic upstream,
- one create/use/revoke scenario provides more durable coverage than a broader but shallower UI-only test set.

## Implementation

### Gateway and store contract

The gateway now splits transport, policy, and persistence explicitly:

- [crates/gateway/src/http/api_keys.rs](../../crates/gateway/src/http/api_keys.rs)
- [crates/gateway-service/src/admin_api_keys.rs](../../crates/gateway-service/src/admin_api_keys.rs)
- [crates/gateway-core/src/traits.rs](../../crates/gateway-core/src/traits.rs)
- [crates/gateway-store/src/store.rs](../../crates/gateway-store/src/store.rs)

The HTTP module:

- requires platform-admin session auth,
- parses request payloads and path parameters,
- maps service errors into the existing admin API envelopes.

The service module:

- lists API keys together with assignable owners and live model choices,
- validates create requests,
- creates keys through the store,
- revokes keys and reloads the resulting state.

The store contract is split between runtime auth (`ApiKeyRepository`) and admin lifecycle (`AdminApiKeyRepository`), then implemented for both backends in:

- [crates/gateway-store/src/libsql_store/api_keys.rs](../../crates/gateway-store/src/libsql_store/api_keys.rs)
- [crates/gateway-store/src/postgres_store/api_keys.rs](../../crates/gateway-store/src/postgres_store/api_keys.rs)
- [crates/gateway-store/src/libsql_store/mod.rs](../../crates/gateway-store/src/libsql_store/mod.rs)
- [crates/gateway-store/src/postgres_store/mod.rs](../../crates/gateway-store/src/postgres_store/mod.rs)

Those changes let the admin layer do real list/create/revoke work without bypassing the same storage backends used at runtime and without relying on optional trait defaults.

### Runtime alignment

API-key status parsing and runtime auth checks were hardened in:

- [crates/gateway-core/src/domain.rs](../../crates/gateway-core/src/domain.rs)
- [crates/gateway-core/src/traits.rs](../../crates/gateway-core/src/traits.rs)
- [crates/gateway-store/src/libsql_store/support.rs](../../crates/gateway-store/src/libsql_store/support.rs)
- [crates/gateway-store/src/postgres_store/support.rs](../../crates/gateway-store/src/postgres_store/support.rs)
- [crates/gateway-service/src/authenticator.rs](../../crates/gateway-service/src/authenticator.rs)

This matters because control-plane revoke is only real if the data plane rejects revoked credentials immediately.

### Admin UI

The admin UI no longer fabricates API-key rows locally. The API Keys feature is also now decomposed into feature-local pieces instead of a single large route:

- [crates/admin-ui/web/src/routes/api-keys.tsx](../../crates/admin-ui/web/src/routes/api-keys.tsx)
- [crates/admin-ui/web/src/routes/api-keys/-use-api-keys-page.ts](../../crates/admin-ui/web/src/routes/api-keys/-use-api-keys-page.ts)
- [crates/admin-ui/web/src/routes/api-keys/-components.tsx](../../crates/admin-ui/web/src/routes/api-keys/-components.tsx)

That split keeps:

- the route focused on the loader boundary,
- orchestration state in one feature-local hook,
- create/revoke/one-time-secret UI in dedicated local components.

Operators can still:

- list live keys,
- show owner and grant context,
- create keys through server functions,
- expose the raw key once,
- revoke keys through the live admin contract.

The contract types were updated in [crates/admin-ui/web/src/types/api.ts](../../crates/admin-ui/web/src/types/api.ts), and the shell copy was corrected in [crates/admin-ui/web/src/components/layout/app-shell.tsx](../../crates/admin-ui/web/src/components/layout/app-shell.tsx) so operators are not told that API keys are still preview-only.

### Tests

Coverage was added or updated in:

- [crates/gateway/src/main.rs](../../crates/gateway/src/main.rs)
- [crates/admin-ui/web/src/server/admin-data.server.test.ts](../../crates/admin-ui/web/src/server/admin-data.server.test.ts)
- [crates/admin-ui/web/e2e/gateway-contract.e2e.ts](../../crates/admin-ui/web/e2e/gateway-contract.e2e.ts)
- [crates/admin-ui/web/e2e/admin-auth.e2e.ts](../../crates/admin-ui/web/e2e/admin-auth.e2e.ts)

The important contract now covered is:

1. create a key through the admin UI,
2. use that raw key against live `/v1/models`,
3. revoke it through the admin UI,
4. observe `401` with `api_key_revoked`.

### Documentation cleanup

Canonical docs were updated in:

- [../README.md](../README.md)
- [../admin-control-plane.md](../admin-control-plane.md)
- [../e2e-contract-tests.md](../e2e-contract-tests.md)
- [../observability-and-request-logs.md](../observability-and-request-logs.md)

The observability update is not directly about API keys, but this work intentionally fixed the stale request-log-detail wording at the same time because the runtime had already moved to strict `404` semantics.

## Consequences

Positive:

- the control plane and runtime now share one API-key truth,
- new credentials are created with explicit ownership and explicit grants,
- revoke is an operationally meaningful action rather than a UI-only status label,
- the UI no longer trains contributors to preserve the old preview pattern,
- the contract suite now covers the newly-live surface through the existing harness.

Trade-offs:

- grant edits and rename flows remain deferred until we have a clear lifecycle reason to add them,
- create now depends on live owner and model state rather than tolerating placeholder data,
- list payload assembly currently reloads grant state per key, which is acceptable at current admin scale but may need batching later,
- the service introduces one more internal boundary to maintain, but that cost is lower than continuing to let policy spread through transport code.

## Follow-Up Work

- [#27](https://github.com/ahstn/oceans-llm/issues/27) remains the next control-plane maturity gap for model inventory.
- If we later add grant editing, it should preserve the same principles as this ADR: explicit owner semantics, explicit grant changes, and no secret recovery path.
