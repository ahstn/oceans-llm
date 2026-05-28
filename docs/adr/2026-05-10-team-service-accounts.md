# ADR: Team Service Accounts for Non-Human Gateway Access

- Date: 2026-05-10
- Status: Accepted
- Superseded in part by: [2026-05-27 Budget Principal Taxonomy](2026-05-27-budget-principal-taxonomy.md)
- Related Issues:
  - [#107](https://github.com/ahstn/oceans-llm/issues/107)
- Builds On:
  - [2026-03-05-identity-foundation.md](2026-03-05-identity-foundation.md)
  - [2026-03-29-live-admin-api-key-management-and-contract-coverage.md](2026-03-29-live-admin-api-key-management-and-contract-coverage.md)
  - [2026-03-31-declarative-config-seeded-identity-and-budget-reconciliation.md](2026-03-31-declarative-config-seeded-identity-and-budget-reconciliation.md)

## Context

The gateway already has users, teams, API keys, budgets, request logs, and admin lifecycle APIs. That was enough for early runtime access, but it left non-human team callers in the wrong part of the identity model.

The old shape overloaded team-owned API keys and config-seeded system keys:

- a team could directly own a runtime API key,
- deploy-style seed keys used a reserved `system-legacy` team,
- non-human automation did not have a principal of its own,
- budget and request-log attribution could identify a team but not the automation identity within that team,
- docs had to warn that provider `service_account` auth was unrelated to gateway service-account semantics.

That model is too weak for long-lived team automation. A credential is a secret. It should not also be the durable principal that owns lifecycle, grants, attribution, and budget policy.

## Decision

We are making service accounts first-class team-owned non-human gateway principals.

The decisions are:

### 1. Remove legacy team-owned runtime API keys

Direct team-owned runtime API keys are removed from the product contract.

There is no compatibility mode for:

- direct `team` ownership on runtime API keys,
- the reserved `system-legacy` team,
- system-owned seeded gateway keys,
- treating a team itself as the non-human caller.

Why:

- team-owned keys collapse a team ownership boundary and a non-human caller into one object,
- `system-legacy` creates a permanent exception that weakens authorization and reporting rules,
- removing the compatibility path keeps migrations and future docs from preserving a misleading security model.

### 2. Service accounts are durable non-human principals

A service account belongs to exactly one team and can own runtime credentials.

Service accounts:

- cannot sign in to `/admin`,
- can authenticate to `/v1/*` through issued credentials,
- carry request-log and spend attribution,
- can have service-account budgets,
- remain visible after deactivation for historical audit.

Why:

- automation needs a stable identity independent of individual users,
- credentials can rotate without changing the principal,
- request logs and budget alerts can point to the specific non-human caller.

### 3. Team owners and admins manage their own service accounts

The management boundary follows team administration.

- platform admins bypass team scope and can manage service accounts across all teams,
- team owners and team admins can manage service accounts for their own team,
- ordinary members cannot manage service accounts,
- non-members cannot manage another team's service accounts.

Why:

- team owners and admins are the people closest to the automation they own,
- platform admins still need an operational break-glass and oversight path,
- ordinary membership should not imply permission to create or rotate production credentials.

### 4. Deletion means deactivation

Service-account deletion is implemented as deactivation.

Deactivation:

- blocks future runtime authentication for the principal,
- keeps historical request logs and spend rows attributable,
- preserves the audit trail for credentials, grants, and budgets,
- avoids accidental identity reuse.

Why:

- runtime credentials are security-sensitive,
- historical cost and request attribution must remain stable,
- hard deletion would make incident response and billing review weaker.

### 5. Service-account budget alerts go to owning-team operators

Service-account budget alerts are delivered to active owners and admins of the owning team.

Disabled users, removed team members, ordinary members, and non-members are excluded.

Why:

- a service account has no inbox,
- the owning team's operators are responsible for remediation,
- resolving active recipients prevents stale owners from receiving operational alerts.

### 6. Provider service-account auth remains upstream-only

Provider auth modes such as Vertex `auth.mode: service_account` are upstream credential configuration.

They do not:

- create a gateway service account,
- authenticate a caller to the gateway,
- grant team membership or admin permissions,
- participate in gateway service-account budget policy.

Why:

- provider credentials let the gateway call an upstream provider,
- gateway service accounts let callers call the gateway,
- using the same phrase for both without a boundary would create operator mistakes.

## Consequences

Positive:

- non-human callers get stable principal identity,
- credentials can rotate without changing request attribution,
- team-scoped administration becomes explicit,
- budget alerts have actionable recipients,
- the `system-legacy` exception is removed instead of documented forever.

Trade-offs:

- pre-release databases with direct team-owned runtime keys do not receive a compatibility migration; those credentials are removed and must be recreated as service-account-owned credentials,
- admin APIs and UI need a new service-account management surface,
- docs and examples must stop using seeded gateway keys as the deploy-time data-plane default.

## Follow-Up

- add service-account lifecycle and credential management contract coverage
- add service-account spend and alert recipient tests
- update deploy examples after the implementation slice lands
