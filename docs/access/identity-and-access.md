# Identity and Access

`See also`: [MCP Servers](../configuration/mcp-servers.md), [MCP Client Setup](../mcp/mcp-client-setup.md), [Configuration Reference](../configuration/configuration-reference.md), [Data Relationships](../contributing/reference/data-relationships.md), [Runtime Bootstrap and Access](../setup/runtime-bootstrap-and-access.md), [Service Accounts](service-accounts.md), [OIDC and SSO](oidc-and-sso-status.md), [Admin Control Plane](admin-control-plane.md), [Budgets](budgets.md), [Tagging](../operations/tagging.md), [MCP Invocations](../mcp/mcp-invocations.md), [ADR: Configurable Admin Page Permissions](../adr/2026-08-05-configurable-admin-page-permissions.md), [ADR: Team Service Accounts for Non-Human Gateway Access](../adr/2026-05-10-team-service-accounts.md), [ADR: Admin Identity Lifecycle and Team Member Workflow Hardening](../adr/2026-03-26-admin-identity-lifecycle-and-team-member-workflows.md)

This page describes the live identity model across the gateway and admin control plane.

## Source of Truth

- identity APIs:
  - [../crates/gateway/src/http/identity.rs](../../crates/gateway/src/http/identity.rs)
- lifecycle policy:
  - [../crates/gateway/src/http/identity_lifecycle.rs](../../crates/gateway/src/http/identity_lifecycle.rs)
- access evaluation:
  - [../crates/gateway-service/src/model_access.rs](../../crates/gateway-service/src/model_access.rs)

## Ownership Model

The product uses first-class users, teams, service accounts, and API-key credentials.

- Users and service accounts are gateway principals.
- API keys are credentials attached to a user or service account.
- Teams are durable grouping and service-account ownership boundaries.
- Service accounts are first-class team-owned non-human gateway principals.
- Direct team-owned runtime API keys are not part of the product contract.
- One user belongs to at most one team in this slice.
- Users can exist without a team.
- No reserved `system-legacy` team or system-legacy runtime-key compatibility exists.

Gateway service-account callers are modeled as service-account principals with API-key credentials.

- Use a service-account API key for shared automation or service workloads.
- Use a user-owned API key only for traffic that should spend against one user.
- Use config-seeded keys for bootstrap or deployment-managed callers by declaring their service account and budget in config.
- Keep provider service-account credentials out of this identity model; they belong to provider config.

## User Lifecycle

User status is typed, not free-form text.

- `invited`
- `active`
- `disabled`

Important rules:

- auth-mode changes are only allowed while the user is still `invited`
- deactivation revokes runtime access
- reactivation only restores access when the current auth proof still exists
- reset-onboarding returns the user to `invited`
- the last active platform admin cannot be deactivated or demoted
- the bootstrap admin stays out of normal user-management views

## Browser Session Lifecycle

Browser sessions are durable server-side records referenced by the `ogw_session` browser cookie. Both platform admins and regular users can sign in to the UI.

- normal sign-out revokes only the current cookie-backed session
- logout is idempotent and clears the browser cookie even when the session is already gone
- user lifecycle actions such as deactivation can revoke every active session for that user
- expired, revoked, missing, or disabled-user sessions resolve as unauthenticated and return the user to sign-in

## Admin Page Permission Groups

The gateway selects one page permission group for each browser session:

- A user with `global_role: platform_admin` is in `platform_admins`, even if the user also has a team membership.
- A regular user with an `owner` or `admin` team membership is in `team_admins`.
- A regular team member or teamless user is in `users`.

The gateway resolves the effective page and action sets from config. Team admins inherit all user grants. Platform admins inherit all team-admin and user grants. A repeated grant is safe and appears once. The session response includes the selected group, effective pages, effective actions, and default page. Membership changes affect the next session response.

Page permissions control admin UI navigation, direct routes, and landing pages. They do not authorize API calls. API-key action permissions control both the API operation and its UI control. Each allowed action still checks the active session, ownership, team scope, and resource state. See the [`permissions` config reference](../configuration/configuration-reference.md#permissions) for syntax and validation.

The default UI policy is:

- Platform admins can use all 13 signed-in pages.
- Team admins and regular users can open API Keys, Models, Teams, Users, Usage Costs, Leaderboard, Agent Harnesses, Request Logs, MCP Invocations, and Service Accounts.
- The API Keys page shows credentials owned by the signed-in user. By default, users can create, update, and revoke their own keys. Active team owners and team admins can also manage and reveal service-account credentials for their team. Personal key secrets are shown only once, at creation.
- The Models page shows the full routed-model catalog and can generate client configuration. Model allowlist membership and pricing refresh stay platform-admin-only.
- The Teams and Users pages show the full identity directory read-only. Onboarding links, provider setup data, assignable-user payloads, and all identity mutations stay platform-admin-only.
- Regular-user spend queries are forced to the signed-in user and exclude service-account spend.
- Regular-user request-log and MCP-invocation queries are forced to the signed-in user. Detail endpoints reject records that do not belong to that user.
- Leaderboard and Agent Harnesses are global read views. Every active user can see cross-team names, identifiers, request counts, model use, tool counts, and exact spend in the leaderboard.
- Service Accounts lists active accounts for the signed-in user's team. A teamless user receives an empty list. Ordinary members cannot see service-account credential metadata or use service-account write APIs.
- Identity changes, budget controls, MCP management, Review Agent, and other platform writes keep their current platform-admin checks.

## Bootstrap Admin

Bootstrap admin is the first control-plane access path, not a normal user-management path.

- local config keeps it enabled without forced password rotation
- production-shaped local config keeps it enabled with forced password rotation
- the active config and startup toggles decide whether it is created on boot

For the startup and first-access path, use [runtime-bootstrap-and-access.md](../setup/runtime-bootstrap-and-access.md).

## Onboarding Model

User provisioning is admin-controlled. The control plane generates three onboarding link flows:

- password onboarding: a unique, token-bearing invite URL that only the invited password user can complete
- OIDC onboarding: an optional generated sign-in URL that selects the OIDC provider, passes the user email as `login_hint`, and lands on the account-ready page
- OAuth onboarding: an optional generated sign-in URL that selects the OAuth provider, passes the user email as `login_hint`, and lands on the account-ready page

Admins create users through config or the control plane, or invite them into teams. Pre-provisioned OIDC and OAuth users can instead open the shared `/admin/login` page, select their configured provider, and activate their account after a successful identity match, without a per-user link.

A sign-in URL does not grant access. The gateway still requires a valid provider identity and applies the configured invite or JIT policy. The public login page discovers enabled providers, but it does not create an account when JIT is disabled. See [OIDC and SSO](oidc-and-sso-status.md#start-sso-sign-in) for the sign-in URLs and parameter rules.

## Team Lifecycle

Current team-management rules:

- teams can be created before users exist
- teams can be created with zero admins
- the admin UI can add existing teamless users or invite new users directly into a team
- team owners and admins can manage service accounts for their own team
- platform admins can manage service accounts across teams
- non-owner memberships can be transferred between teams
- `owner` memberships are visible but blocked from casual lifecycle edits

## Team Transfer Rule

Team transfer is easy to overread. The rule is narrow on purpose.

Transfer changes:

- the user’s current membership
- future membership-derived access

Transfer does not change:

- historical request logs
- historical spend rows
- existing budgets
- API-key ownership
- service-account ownership

That boundary is a policy rule, not a UI shortcut.

## Identity Tags

Admins can attach bounded key/value tags to users, teams, and service accounts. User and team tags can be managed from the admin identity UI; config-seeded user, team, and service-account tags are reconciled on startup. These tags are displayed in the admin identity views and are intended for attribution, export, and reconciliation with external systems.

Identity tags do not change runtime access, budget checks, request routing, request-log filtering, or historical ownership. The tag rules and usage guidance live in [Tagging](../operations/tagging.md).

## OIDC Boundary

OIDC is part of the browser sign-in contract.

- enabled providers are seeded from `auth.oidc.providers`
- pre-provisioned OIDC users are supported by provider key
- invited OIDC users activate on first successful provider login
- provider-specific JIT can create users with explicit defaults
- password users are not auto-linked by email

Use [oidc-and-sso-status.md](oidc-and-sso-status.md) for the practical SSO contract and Authentik fixture details.

## Model Access Overlays

Effective model access is layered:

1. API-key grant mode for the authenticated user or service-account credential.
2. Team allowlist when the team is `restricted`.
3. Service-account allowlist when the service account is `restricted`.
4. User allowlist when the user is `restricted`.
5. Model-level allowlist when the requested gateway model declares `allowlist` in config.

API keys can use `model_grant_mode='explicit'` with rows in `api_key_model_grants`, or `model_grant_mode='all'` to track every current and future gateway model. Owner restrictions always intersect that baseline; `all` never bypasses team, service-account, user, or model-level allowlists.

Principal-centric restrictions answer "which models may this principal use?" Model-level allowlists answer "which users or teams may use this model?" They are separate overlays and both must pass.

For human users, a model-level allowlist passes when either the normalized user email or the user's effective team key is listed. Unknown refs in the model config are allowed and become useful if a matching user or team exists later.

For service accounts, the team allowlist applies through the owning team and the service-account allowlist applies directly when restricted. User allowlists do not apply because service accounts are not users. Admin-managed service-account API keys require explicit model grants so automation credentials stay deliberately scoped.

In v1, service-account-owned API keys are denied for models that have a model-level allowlist. Use a human user-owned key for allowlisted model traffic, or leave the model without a model-level allowlist when service-account automation must call it.

## MCP Gateway API-Key Contract

MCP gateway data-plane routes use the same Oceans API-key identity model as `/v1/*`, with a narrower header contract:

```text
GET /mcp/{server_key}
POST /mcp/{server_key}
DELETE /mcp/{server_key}
```

Accepted inbound credential headers:

- preferred: `Authorization: Bearer <oceans-api-key>`
- secondary explicit header: `x-oceans-api-key: <oceans-api-key>`

If both are present, the raw key extracted from `Authorization` must exactly match `x-oceans-api-key`. A malformed `Authorization` header is rejected even when `x-oceans-api-key` is present.

Not accepted:

- `x-api-key`
- `API-Key`
- query-string credentials
- upstream provider credentials
- upstream MCP credentials

For this slice, valid user-owned and service-account-owned Oceans API keys may proxy to active MCP servers that use `none`, `gateway_static_header`, or `gateway_bearer_token`. Servers that require user-scoped upstream credentials return `403 mcp_upstream_auth_required`.

Inbound Oceans credentials are never upstream credentials. The gateway strips inbound `Authorization` and `x-oceans-api-key` before proxying to a registered MCP server, then applies only configured gateway-managed upstream auth when the server record requires it.

## Request Logging Preference

Request logging policy is partly owned by identity.

- user-owned requests honor `users.request_logging_enabled`
- service-account requests always persist request-log summary rows
- the admin identity view exposes the current user preference read-only

MCP invocation logging follows the same ownership vocabulary for audit context. Invocation rows should preserve the API key, user, and team ids available at execution time, but they do not rewrite historical ownership when a user changes teams or an API key is revoked later.

## Declarative Identity Seed

Config-backed identity is now part of the startup seed path.

- `teams` are reconciled by `team_key`
- `users` are reconciled by normalized email
- `service_accounts` are reconciled by `service_account_key`
- listed users can reconcile team membership and active budgets
- `teams[].tags`, `users[].tags`, and `service_accounts[].tags` reconcile identity tags from config when present
- new config-seeded users start as `invited`
- config seeding does not emit onboarding URLs
- config-seeded OIDC and OAuth users can use the shared `/admin/login` URL; admins may generate a prefilled SSO sign-in link from the control plane when useful
- config-seeded password users still require a unique invite URL generated through the control plane

Config seeding no longer creates legacy system-owned runtime API keys. Non-human team access is managed through service accounts.

The `tags` fields reconcile identity tags when present. Field syntax, omit/clear semantics, and startup validation are owned by the [Configuration Reference](../configuration/configuration-reference.md#identity-tags-in-declarative-identity). Tag rules and naming guidance are owned by [Tagging](../operations/tagging.md).

Team config supports:

- `id`: stable team key used for reconciliation
- `name`: display name
- `tags`: optional identity tag list

User config supports:

- `name`, `email`, and `auth_mode`
- `global_role` and `request_logging_enabled`
- optional `oidc_provider_key` or `oauth_provider_key` for SSO users
- optional `membership` with `team` and non-`owner` `role`
- optional `budget`
- `tags`: optional identity tag list

Service-account config supports:

- `id`: stable service-account key used for reconciliation
- `name`: display name, defaulting to `id`
- `team`: owning team key
- `budget`
- `keys`: optional managed API-key declarations
- `tags`: optional identity tag list

## Service Accounts

Service accounts are the non-human gateway identity model.

- each service account belongs to exactly one team
- service accounts cannot sign in to `/admin`
- service-account credentials can call `/v1/*`
- deletion is deactivation
- service-account budget alerts go to active owning-team owners and admins

Team-scoped management rules live in [service-accounts.md](service-accounts.md).

## Current Boundaries

- group or claim-to-role mapping is not part of the current OIDC contract
- Okta validation is a later benchmark provider, not the local fixture
- broader session-management UI remains separate from the sign-in flow

## Where Identity Appears Operationally

- admin workflows:
  - [admin-control-plane.md](admin-control-plane.md)
- startup and first access:
  - [runtime-bootstrap-and-access.md](../setup/runtime-bootstrap-and-access.md)
- spend ownership effects:
  - [budgets.md](budgets.md)
- request resolution effects:
  - [model-routing-and-api-behavior.md](../configuration/model-routing-and-api-behavior.md)
- non-human gateway access:
  - [service-accounts.md](service-accounts.md)
