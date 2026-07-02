# Issue 206: Top-Level Service Account Config

GitHub issue: https://github.com/ahstn/oceans-llm/issues/206

## Target Shape

```yaml
teams:
  - id: platform
    name: Platform

service_accounts:
  - id: ci-indexer
    name: CI Indexer
    team: platform
    budget:
      cadence: daily
      amount_usd: "25.0000"
      hard_limit: true
      timezone: UTC
    keys:
      - id: primary
        name: CI Indexer Primary
        auto_create: true
        value: env.CI_INDEXER_GATEWAY_API_KEY
        allowed_models:
          - gpt-4o-mini
```

Use plural `keys` so the config supports rotation without another shape change.
`teams[*].id`, `service_accounts[*].id`, and `service_accounts[*].keys[*].id`
are stable config identities. Display names remain renameable.

## Implementation Plan

1. Add an ADR for the declarative contract and generated-secret retrieval model.
2. Keep config changes cohesive. If the identity config surface grows further, split it from `crates/gateway/src/config.rs` into `crates/gateway/src/config/identity.rs`.
3. Add `GatewayConfig.service_accounts` after `teams`.
4. Replace YAML-facing `teams[*].key` with `teams[*].id`; keep DB/internal `team_key` vocabulary unless a broader schema rename is deliberately planned.
5. Remove `auth.seed_api_keys[*].service_account` instead of adding a compatibility translator. Old configs must fail loudly, not silently stop seeding.
6. Replace the conflated `SeedApiKey` model with separate service-account and managed-key seed DTOs.
7. Process config seeding in this order: providers, identity providers, models, teams, service accounts, managed service-account keys, users.
8. Reconcile service-account name and budget independently from API-key creation. Reject declarative team moves unless an ADR explicitly allows them.
9. Add managed credential identity and secret-material storage in both libsql and Postgres migrations.
10. Store generated raw key material separately from `api_keys.secret_hash`. `secret_hash` remains an Argon2 verifier only.
11. Extract the existing AES-256-GCM encrypted secret pattern from MCP credentials into a shared gateway-secret helper.
12. Add an explicit reveal endpoint for stored managed key material. List responses must remain redacted.
13. Authorize reveal for active platform admins or active members of the owning team, per issue #206.
14. Extend redaction for `raw_key`, `generated_key`, and `key_material`.
15. Update docs, examples, OpenAPI, generated admin UI types, and tests.

## Verification Gates

- Config parser tests for valid service accounts, duplicate IDs, missing teams, default names, generated keys, explicit env values, unknown model grants, and rejection of old nested service-account seed config.
- Store seed tests for libsql and Postgres covering create, idempotent rerun, rename, team-move rejection, budget reconciliation, allowed-model drift, and no duplicate managed keys.
- Secret-storage tests proving raw generated values are encrypted separately and not stored in `api_keys.secret_hash`.
- HTTP tests for unauthenticated, disabled, removed, non-member, cross-team, platform-admin, and owning-team reveal behavior.
- Redaction tests for raw key field names.
- Contract generation and drift checks.
- `mise run lint`.
