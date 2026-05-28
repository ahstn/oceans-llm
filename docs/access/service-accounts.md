# Service Accounts

`See also`: [Identity and Access](identity-and-access.md), [Admin Control Plane](admin-control-plane.md), [Budgets and Spending](../operations/budgets-and-spending.md), [Data Relationships](../reference/data-relationships.md), [ADR: Team Service Accounts for Non-Human Gateway Access](../adr/2026-05-10-team-service-accounts.md)

This page describes the intended service-account model for non-human gateway callers.

## Model

Service accounts are first-class gateway principals for automation, applications, and other non-human callers.

- every service account belongs to exactly one team
- service accounts are not users and cannot sign in to `/admin`
- service-account credentials can call `/v1/*`
- service-account spend is attributable to the service account and its owning team
- service-account lifecycle is independent from team user membership
- active service-account credentials require an active service-account budget

API keys remain credentials. They are not the principal for team automation. A non-human team caller authenticates with a credential attached to a service account.

## No Legacy Team-Owned Runtime Keys

Direct team-owned runtime API keys are removed from the product contract.

Removed compatibility paths:

- no reserved `system-legacy` team
- no system-owned seeded runtime key compatibility
- no direct team owner kind on runtime API keys
- no fallback that treats a team as a non-human principal

Teams own service accounts. Service accounts own their runtime credentials.

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

Ordinary team members cannot manage service accounts. Users outside the owning team cannot manage that team's service accounts unless they are platform admins.

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
