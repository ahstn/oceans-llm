# Service Account Config and Managed API Key Secrets

## Status

Accepted.

## Context

Service accounts were historically created as a side effect of `auth.seed_api_keys`.
That made workload identity, bootstrap credentials, budgets, and API key material share
one config surface. It also made service accounts difficult to reason about because the
stable identity lived under an API key entry rather than as a first-class object.

Generated gateway API keys also need a retrieval model. The verifier hash in
`api_keys.secret_hash` must remain a one-way authentication verifier and cannot be used
to show the caller credential later.

## Decision

Declarative workload identity is configured with top-level `service_accounts` entries.
Teams use YAML `id` fields as stable config identities, and service accounts reference
those team IDs.

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
        auto_create: true
        value: env.CI_INDEXER_GATEWAY_API_KEY
        allowed_models:
          - gpt-4o-mini
```

`auth.seed_api_keys` is removed from the YAML contract. Old configs fail during parsing
instead of being translated.

Config seeding processes identity in this order:

1. teams
2. service accounts
3. managed service-account API keys
4. users and memberships

Service-account names and budgets are reconciled independently from API keys. Team moves
for existing declarative service accounts are rejected because moving a workload identity
between teams changes authorization and budget ownership.

Managed service-account keys have stable config IDs under their owning service account.
Configured key values are authoritative and may rotate by changing `keys[*].value`.
Generated keys are create-only: if the managed key already exists, rerunning config
seeding must not rotate its public ID, verifier hash, or encrypted secret material.

Retrievable service-account key material is stored in `api_key_secret_materials` as an
encrypted blob. `api_keys.secret_hash` remains an Argon2 verifier. Encryption uses the
gateway runtime key from `OCEANS_API_KEY_SECRET_ENCRYPTION_KEY`, which must be a
base64-encoded 32-byte key. Raw key values are not returned by list APIs and are covered
by response redaction keys.

## Consequences

- Service-account ownership is visible and configurable without relying on API-key seed
  side effects.
- Bootstrap/admin credentials and workload credentials are separate operational concerns.
- Deployments that configure managed service-account keys must provide
  `OCEANS_API_KEY_SECRET_ENCRYPTION_KEY` to seed or create retrievable service-account
  keys.
- Generated managed keys can be revealed later to platform admins or active members of
  the owning team, but they are not implicitly rotated on restart.
- Rotating the encryption key requires a deliberate re-encryption procedure; changing
  only the environment variable makes existing stored materials undecryptable.

## Follow-Ups

- Add an admin UI affordance for revealing stored service-account key material with
  explicit user intent.
- Add operational documentation for encryption-key rotation once key identifiers support
  multiple active decrypt keys.
