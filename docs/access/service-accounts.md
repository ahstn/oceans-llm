# Service Accounts

`See also`: [Identity and Access](identity-and-access.md), [Admin Control Plane](admin-control-plane.md), [Budgets](budgets.md), [Configuration Reference](../configuration/configuration-reference.md), [Runtime Bootstrap and Access](../setup/runtime-bootstrap-and-access.md), [ADR: Team Service Accounts for Non-Human Gateway Access](../adr/2026-05-10-team-service-accounts.md), [Service Account Config and Managed API Key Secrets](../adr/2026-07-02-service-account-config-and-managed-api-key-secrets.md)

Service accounts are the gateway identity for automation, applications, CI jobs and agents.

Use them when you need a shared identity, with least privilege, that can be scoped to a team and a workload.

## When to Use a Service Account

Use a service account when:

- a shared workload calls `/v1/*`
- the caller should keep working when a team member leaves
- spend should be visible as team-owned automation spend
- model grants should be scoped to the workload instead of a person

Use a user-owned API key only when the traffic should be attributable to that user. Do not use provider service-account credentials, such as Google Cloud service-account JSON, as gateway caller credentials.

## Model

Service accounts are first-class gateway principals for machine callers.

- every service account belongs to exactly one team
- service accounts are not users and cannot sign in to `/admin`
- service-account API keys can call `/v1/*`
- service-account spend is attributable to the service account and its owning team
- service-account lifecycle is independent from team user membership
- active service-account API keys require an active service-account budget

API keys are credentials. They are not the principal for team automation. A non-human team caller authenticates with a credential attached to a service account.

## Add a Service Account in the Admin UI

Allowed admins can create service accounts and credentials from `/admin`.

1. Open **Identity** and create the service account under the owning team.
2. Open **Spend Controls** and create an active **Service Account Budget** for that service account.
3. Open **API Keys** and create a key whose owner is the service account.
4. Grant only the gateway models that workload needs.
5. Copy the raw key when it is shown and store it in the caller's secret manager.
6. Configure the caller to send the key as a bearer token:

```http
Authorization: Bearer gwk_<public-id>.<secret>
```

For rotation, create a replacement key, update the caller secret, verify traffic with the replacement key, then revoke the old key.

## Configure Service Accounts Declaratively

Deployments can seed service accounts from top-level `service_accounts` config. This is the recommended path for bootstrap or deployment-managed automation credentials.

The owning team must exist in `teams`. Each declared service account must have an active budget before its active keys can authenticate.

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
        allowed_models:
          - gpt-4o-mini
          - gemini-fast
```

This example creates the `ci-indexer` service account, reconciles its budget, and creates one managed gateway API key if that managed key does not already exist. Generated managed keys are create-only: rerunning config does not rotate an existing generated key.

Managed service-account key material is encrypted before storage so allowed admins can reveal it later. Set `OCEANS_API_KEY_SECRET_ENCRYPTION_KEY` to a base64-encoded 32-byte key before using config-managed service-account keys or creating service-account-owned keys in the admin UI. Do not rotate this encryption key by changing only the environment variable; existing stored key material would become undecryptable.

## Use a Static Predefined Key

If a caller already has a deployment-managed key value, reference it with `value`. Use `env.*` so the raw key comes from the deployment secret manager instead of the YAML file.

```yaml
teams:
  - id: platform
    name: Platform

service_accounts:
  - id: release-bot
    name: Release Bot
    team: platform
    budget:
      cadence: monthly
      amount_usd: "100.0000"
      hard_limit: true
      timezone: UTC
    keys:
      - id: primary
        name: Release Bot Primary
        value: env.RELEASE_BOT_GATEWAY_API_KEY
        allowed_models:
          - openai-fast
```

The referenced environment variable must contain a gateway API key in the gateway format:

```dotenv
RELEASE_BOT_GATEWAY_API_KEY=gwk_releasebotprod.primary-secret-value
```

Static, predefined keys must be prefixed with `gwk_` and include both a non-empty public ID and a non-empty secret separated by a dot:

```text
gwk_<public-id>.<secret>
```

Examples that are rejected:

```text
releasebotprod.primary-secret-value
gwk_releasebotprod
gwk_.primary-secret-value
gwk_releasebotprod.
```

Configured key values are authoritative. Changing `keys[*].value` rotates that managed key to the new predefined value on the next seed run.

## Access Control

Service-account management is scoped by the acting admin.

Platform admins can:

- list and manage service accounts across all teams
- create service accounts for any team
- deactivate service accounts for any team
- manage service-account credentials and grants across all teams

Team owners and team admins can:

- list service accounts for their own team
- create service accounts for their own team
- deactivate service accounts for their own team
- manage credentials and grants for their own team's service accounts
- reveal stored key material for their own team's service-account credentials

Ordinary team members cannot manage service accounts or reveal stored key material. Users outside the owning team cannot manage that team's service accounts unless they are platform admins.

## Lifecycle

Deletion is deactivation.

Deactivation means:

- the service account remains in historical records
- active runtime credentials stop authenticating
- historical request logs and spend rows keep their service-account attribution
- the service account cannot be used for new runtime calls unless it is explicitly reactivated by an allowed admin workflow

Credential revocation remains separate from service-account deactivation. Revoking one credential blocks that secret only. Deactivating the service account blocks the principal.

## Budget Gate

Service accounts are spend-bearing principals. A service-account API key cannot authenticate unless the service account has an active budget. Admins must revoke or deactivate active service-account keys before deactivating that service account's budget.

## Budget Alerts

Service-account budgets notify the people who can act for the owning team.

Recipients are:

- active team owners
- active team admins

Recipients are resolved when alert delivery rows are created. Disabled users, removed team members, ordinary members, and non-members do not receive service-account budget alerts.

## No Legacy Team-Owned Runtime Keys

Direct team-owned runtime API keys are removed from the product contract.

Removed compatibility paths:

- no reserved `system-legacy` team
- no system-owned seeded runtime key compatibility
- no direct team owner kind on runtime API keys
- no fallback that treats a team as a non-human principal

Teams own service accounts. Service accounts own their runtime credentials.

The old YAML-facing `auth.seed_api_keys` path is removed. Declare non-human gateway callers in top-level `service_accounts` instead.

## Provider Credential Boundary

Gateway service accounts are not provider service-account credentials.

For example, Vertex config can use:

```yaml
providers:
  - id: vertex
    type: gcp_vertex
    auth:
      mode: service_account
      credentials_path: /var/run/secrets/gcp/service-account.json
```

That `service_account` mode is upstream Google Cloud authentication. It lets the gateway call Vertex. It does not create a gateway service account, grant a caller access to `/v1/*`, or participate in gateway team membership rules.
