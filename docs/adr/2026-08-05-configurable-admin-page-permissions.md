# ADR: Configurable Admin Page Permissions

- Date: 2026-08-05
- Status: Accepted

## Current state

- [Configuration Reference](../configuration/configuration-reference.md#permissions)
- [Identity and Access](../access/identity-and-access.md#admin-page-permission-groups)
- [Admin Control Plane](../access/admin-control-plane.md#admin-auth-and-session-behavior)

## Context

Regular users can sign in to the admin UI, but the UI used hard-coded role checks and repeated route lists. Admins could not change page visibility through gateway config. The session contract also did not describe the effective page policy.

Page visibility must stay separate from API authorization. A visible page must have useful read behavior for its audience, while write routes and scoped data must keep their existing security checks.

## Decision

### 1. Config defines direct grants for three groups

The top-level `permissions` object has `users`, `team_admins`, and `platform_admins` groups. Each group can set a `pages` list and `default_page`.

Regular team members and teamless users resolve to `users`. Team owners and team admins resolve to `team_admins`. A platform admin always resolves to `platform_admins`, regardless of team membership.

### 2. Higher groups inherit lower grants

The effective `team_admins` set is the union of the direct user and team-admin grants. The effective `platform_admins` set is the union of all three direct grant lists.

Repeated grants within or across lists are valid. The gateway removes duplicates and returns pages in a stable order. An explicit empty list removes only that group's direct grants. A final empty set sends the signed-in user to a no-access page.

### 3. Capability ceilings reject unsupported exposure

Users and team admins can receive only the 10 shared pages. Platform admins can receive all 13 signed-in pages. Startup rejects unknown fields, unknown page names, invalid default pages, and direct grants above a group's capability ceiling.

The ceiling prevents config from showing a page whose backend does not support that audience. It can expand only when the related API and data scope also expand.

### 4. The session owns the effective UI policy

The gateway resolves config once at startup. Each auth session response selects the user's group from the current role and team membership, then returns the effective pages and default page.

The UI has one typed page registry. It uses the session policy for sidebar filtering, direct-route checks, default redirects, and cross-page links. Public auth routes and required password change remain outside page permissions.

### 5. Three read pages join the shared set

Leaderboard and Agent Harnesses allow every active signed-in user to read their current global views. The responses use short, range-keyed caches with single-flight loading.

The global leaderboard disclosure is accepted. A regular user can see cross-team names, user identifiers, exact spend, request counts, model use, and tool-cardinality totals. Hiding the page does not remove direct authenticated API access.

Service Accounts allows active users to list accounts for their own active team. A teamless user receives an empty list. Platform admins keep the global list. Ordinary team members do not receive service-account credential metadata, and all service-account write checks stay unchanged.

### 6. Page visibility is not API authorization

Page grants are a console policy, not a general RBAC system. Handlers continue to enforce active sessions, platform roles, team scope, ownership, and mutation rules. Personal spend, request logs, MCP invocations, API keys, and identity directories keep their existing data scope.

## Consequences

Benefits:

- admins can change console visibility without rebuilding the UI
- higher groups do not need repeated shared grants
- SSR, client navigation, and direct URLs use one effective policy
- session responses make the selected policy explicit and testable
- default regular users can inspect global rankings, harness use, and their team's service accounts

Trade-offs:

- a config change needs a gateway restart
- the global leaderboard exposes cross-team operational data to all active users
- page hiding does not block direct API calls
- adding a new page requires a page identifier, UI registry entry, capability review, and config update

## Follow-up work

- Add action-level permissions only if page-level control is too broad for future workflows.
- Widen a capability ceiling only after its API authorization and data scope support that audience.

## Attribution

This ADR was prepared through collaborative human + AI implementation and design work.
