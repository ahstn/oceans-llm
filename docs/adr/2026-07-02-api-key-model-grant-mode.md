# ADR: API-Key Model Grant Mode

- Date: 2026-07-02
- Status: Accepted
- Related Issues:
  - [#204](https://github.com/ahstn/oceans-llm/issues/204)
- Builds On:
  - [2026-03-05-identity-foundation.md](2026-03-05-identity-foundation.md)
  - [2026-03-29-live-admin-api-key-management-and-contract-coverage.md](2026-03-29-live-admin-api-key-management-and-contract-coverage.md)
  - [2026-05-10-team-service-accounts.md](2026-05-10-team-service-accounts.md)

## Context

API keys previously represented model access only through explicit rows in `api_key_model_grants`. That made narrow credentials easy to audit, but it forced admins to revisit user-owned API keys whenever the gateway model catalog changed.

The product needs a way for a user-owned API key to follow the current model catalog without treating an empty grant list as an overloaded sentinel. Empty explicit grants already mean "no selected models" during validation, and using that shape for "all models" would make admin summaries, runtime auth, and future migrations ambiguous.

Service-account API keys have a different operational profile. They are shared automation credentials with service-account budgets and should remain deliberately scoped to the models the workload needs.

## Decision

Store API-key model grant behavior as `api_keys.model_grant_mode`.

The supported values are:

- `all`: runtime starts from the current gateway model catalog.
- `explicit`: runtime starts from rows in `api_key_model_grants`.

Owner overlays still intersect the API-key baseline:

- restricted teams narrow both user-owned and service-account-owned keys;
- restricted service accounts narrow service-account-owned keys;
- restricted users narrow user-owned keys.

Admin-managed service-account API keys must use `explicit`. User-owned API keys may use either mode. When a key is in `all` mode, admin responses return an empty `model_keys` list because there are no explicit grant rows to display.

## Implementation

The decision is implemented across the shared domain, both store backends, runtime access resolution, admin service validation, HTTP contract, and admin UI:

- `ApiKeyModelGrantMode` in `gateway-core`
- `model_grant_mode` on `api_keys`
- `replace_api_key_model_access` as the store operation that updates mode and explicit rows together
- runtime model resolution that expands `all` from `list_models()`
- admin API create/update validation that rejects mixed `all` plus `model_keys`
- admin UI grant-mode controls for user-owned keys, with service-account keys fixed to selected models

## Consequences

This removes the need for admins to maintain explicit grants for user credentials that should track all models.

It keeps service-account credentials least-privilege by default and avoids expanding automation access just because the model catalog grows.

It makes the grant mode auditable and migration-safe because `all` is explicit data, not inferred from missing rows.

## Follow-Up

- Add richer audit-log events for grant-mode changes when the audit surface is expanded.
- Revisit whether service-account all-mode is ever appropriate only if there is a separate workload policy model that can preserve least-privilege guarantees.
