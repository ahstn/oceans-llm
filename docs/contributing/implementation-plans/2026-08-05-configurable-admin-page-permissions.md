# Configurable Admin Page Permissions

`See also`: [Identity and Access](../../access/identity-and-access.md), [Admin Control Plane](../../access/admin-control-plane.md), [Configuration Reference](../../configuration/configuration-reference.md), [ADR: Team Service Accounts for Non-Human Gateway Access](../../adr/2026-05-10-team-service-accounts.md)

- Date: 2026-08-05
- Status: Implemented
- Preceding change: [PR #261](https://github.com/ahstn/oceans-llm/pull/261)
- Primary target: control which signed-in admin UI pages each user group can see and open

## Summary

Add a top-level `permissions` object to gateway config. It will define page allowlists for three groups:

- `platform_admins`: users with `global_role: platform_admin`
- `team_admins`: regular users with an `owner` or `admin` team membership
- `users`: regular users with a `member` team membership or no team membership

The gateway will resolve a signed-in user's group and effective page set. Higher roles inherit all pages from lower roles. It will return the result in `/api/v1/auth/session`. The UI will use the returned page set for navigation, landing-page selection, and direct-route checks.

The default `users` set will include Leaderboard, Agent Harnesses, and Service Accounts. The related read APIs must support regular users so each visible page works. Existing write authorization and ownership checks will remain the security boundary.

## Scope

### Goals

- Let admins hide signed-in admin UI pages by user group.
- Add Leaderboard, Agent Harnesses, and Service Accounts to the default `users` page set.
- Treat team owners and team admins as one `team_admins` page group.
- Let higher roles inherit every page granted to lower roles.
- Accept repeated page grants across groups and remove duplicates from the effective sets.
- Make the gateway the source of truth for the user's resolved group and page set.
- Reject invalid page names and unsafe group-to-page grants at startup.
- Keep navigation, direct URLs, redirects, and server rendering consistent.
- Support a group with no console pages without creating a login redirect loop.

### Non-goals

- Do not replace handler-level or middleware API authorization.
- Do not change request-log, personal spend, API-key, or identity-directory data scope.
- Do not add action-level permissions such as `create`, `update`, or `delete` in this change.
- Do not add custom groups, per-user grants, team-specific page rules, or deny rules in v1.
- Do not persist page permissions in the database.
- Do not hot-reload page permissions. A gateway restart will apply config changes.

## Current Behavior

PR #261 added a hard-coded two-level UI policy:

- Platform admins can open every signed-in page.
- All other users can open API Keys, Models, Teams, Users, Usage Costs, Request Logs, and MCP Invocations.
- `adminOnly` navigation flags hide the remaining pages.
- `USER_ACCESSIBLE_PATHS` in `-auth-routing.ts` repeats the regular-user route list.
- `requireAdminSession` and `requireAuthenticatedSession` add a second role split at page loaders.
- The session contract exposes `global_role`, but it does not expose team membership or effective page permissions.

This plan extends that policy. All users will also see Leaderboard, Agent Harnesses, and Service Accounts by default. Leaderboard and Agent Harnesses will expose their current global read views to every active user. Service Accounts will show accounts for the user's own team. A teamless user will see an empty state. Service-account writes and credential metadata will keep their current stricter rules.

## Config Contract

Use stable page identifiers instead of URL paths. Page identifiers do not include the `/admin` base path and do not change when a child route is added.

```yaml
permissions:
  users:
    pages:
      - api_keys
      - models
      - usage_costs
      - leaderboard
      - agent_harnesses
      - request_logs
      - mcp_invocations
      - teams
      - users
      - service_accounts
    default_page: usage_costs

  team_admins:
    pages: []
    default_page: usage_costs

  platform_admins:
    pages:
      - mcp
      - review_agent
      - spend_controls
    default_page: api_keys
```

This explicit example produces the default effective sets without repeating shared pages. Team admins inherit every page under `users.pages`. Platform admins inherit pages from both lower groups, then add three platform-only pages.

Recommended Rust shape:

```rust
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct PermissionsConfig {
    pub platform_admins: Option<PagePermissionSetConfig>,
    pub team_admins: Option<PagePermissionSetConfig>,
    pub users: Option<PagePermissionSetConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PagePermissionSetConfig {
    pub pages: Option<Vec<AdminPage>>,
    pub default_page: Option<AdminPage>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AdminPage {
    ApiKeys,
    Models,
    Mcp,
    ReviewAgent,
    UsageCosts,
    SpendControls,
    Leaderboard,
    AgentHarnesses,
    RequestLogs,
    McpInvocations,
    Teams,
    Users,
    ServiceAccounts,
}
```

Place this named policy concept in `crates/gateway/src/config/permissions.rs` and re-export its public config types from `config.rs`. Do not add more page-policy logic to the existing large `config.rs` file.

### Default and override rules

- Each `pages` list contains direct grants for that group. It does not need pages inherited from a lower group.
- The effective `users` set is `users.pages`.
- The effective `team_admins` set is the union of `users.pages` and `team_admins.pages`.
- The effective `platform_admins` set is the union of all three `pages` lists.
- A page can appear in more than one group. Treat page lists as sets and remove duplicates without an error.
- If `permissions` is absent, use the direct default grants in the table below.
- If one group is absent, use that group's direct default grants.
- If a group exists but `pages` is absent, use that group's direct default grants.
- If `pages` is present, it replaces only that group's direct default grants. Inherited grants still apply.
- An explicit empty list is valid. A group can still receive inherited pages.
- Validate `default_page` against the final effective set, not only the group's direct list.
- If `default_page` is absent, keep the current preferred default when it is effective. Otherwise, select the first effective page from a stable gateway fallback priority.
- If the final effective set is empty, the user will land on a signed-in no-access page.
- Do not support `*` in v1. New pages must not become visible through an old explicit list without an admin review.

### Default grants and effective sets

| Group | Direct default grants | Effective default pages | Default page |
| --- | --- | --- | --- |
| `users` | `api_keys`, `models`, `teams`, `users`, `usage_costs`, `leaderboard`, `agent_harnesses`, `request_logs`, `mcp_invocations`, `service_accounts` | The same 10 pages | `usage_costs` |
| `team_admins` | None | The 10 pages inherited from `users` | `usage_costs` |
| `platform_admins` | `mcp`, `review_agent`, `spend_controls` | All 13 signed-in pages | `api_keys` |

These defaults add three pages for regular users. They preserve the existing platform-admin set and avoid repeated grants in the default config.

### Validation rules

Reject config at startup when:

- a group, field, or page identifier is unknown
- `default_page` is not in that group's effective page list
- a group is granted a page that its backend capability ceiling cannot support

Repeated page identifiers are valid. Normalize and remove duplicates within one list and across inherited lists.

Use these initial capability ceilings:

| Group | Pages that config can allow |
| --- | --- |
| `platform_admins` | All signed-in pages |
| `team_admins` | The 10 shared pages |
| `users` | The 10 shared pages |

The ceiling applies to direct grants before inheritance. It prevents a configuration such as `users: [spend_controls]` from exposing a page whose APIs still require a platform admin. A later change can widen a ceiling only after the page's API and data scope support that group.

## Runtime Policy and Session Contract

Derive a compact, validated `ResolvedAdminPermissions` value from `GatewayConfig` during startup. Apply the role unions once during this step. Store the resolved effective sets in `AppState` as an `Arc`. Do not store the full gateway config in HTTP state.

The gateway fallback priority controls only automatic default selection. It does not map URLs or control sidebar order. The UI will use the resolved `default_page` from the session response.

Resolve the user group on each session response:

1. `global_role: platform_admin` resolves to `platform_admins` and takes precedence over team membership.
2. An `owner` or `admin` membership resolves to `team_admins`.
3. A `member` membership or no membership resolves to `users`.

The current store permits at most one team membership per user, so this rule does not need multi-team precedence logic. Membership changes will affect the next session lookup without session re-issuance. Do not query team membership for a platform admin because the global role has precedence.

Extend `AuthSessionView`:

```rust
pub struct AuthSessionPermissionsView {
    pub group: AdminPermissionGroup,
    pub pages: Vec<AdminPage>,
    pub default_page: Option<AdminPage>,
}

pub struct AuthSessionView {
    pub user: AuthSessionUserView,
    pub must_change_password: bool,
    pub permissions: AuthSessionPermissionsView,
}
```

Convert `build_auth_session_view` to an async builder that can call `get_team_membership_for_user`. Use it for session lookup, password login, password change, and any other response that returns `AuthSessionView`. Each response must resolve membership once.

The `pages` field in the session response contains the final effective set after inheritance and duplicate removal. Regenerate the OpenAPI JSON and TypeScript types after the contract changes. The UI must consume the generated `AdminPage` and group unions instead of duplicating string unions by hand.

## UI Design

### One page registry

Make `admin-nav.ts` the UI registry for configurable pages. Each item will have a typed `page` identifier, path, label, icon, and section.

Use this registry for:

- sidebar filtering
- breadcrumb lookup
- page-to-path conversion
- path-to-page conversion
- default-page redirects
- direct-route checks
- cross-page link and action checks

Map child paths to their owning page. For example, `/mcp`, `/mcp/servers`, and the legacy `/mcp/access` path all map to `mcp`.

Remove `adminOnly` and `USER_ACCESSIBLE_PATHS`. They are parallel permission lists and will drift from config-backed policy.

### Route enforcement

Remove the role-specific page guards. The root route will load the session once, resolve the current path through the page registry, and check `session.permissions.pages`. Child routes will consume the root route context and must not fetch the session again.

This keeps SSR, client navigation, and direct URLs consistent. A hidden page will redirect to the resolved default page. It must not redirect to login because the user is authenticated. Route files will not repeat page identifiers or path-to-page mappings.

Use one `canAccessPage(session, page)` helper for links and actions that open another configurable page. Hide or disable an action when its target page is not effective. This includes Teams to Users, Service Accounts to Teams or API Keys, and Request Logs to MCP Invocations.

Change the `/` index redirect so it uses `session.permissions.default_page` instead of the current constant.

Add an authenticated `/no-access` page outside the configurable page registry. It will show a short message when the effective page list is empty. It will not appear in navigation. Sign-out and password change will remain available.

Public and account-lifecycle routes remain outside page permissions:

- login
- invite completion
- account ready
- required password change

### Read API changes

Visible pages must return useful data. Change only the read APIs required by the new shared pages:

- Leaderboard: allow every active signed-in user to read the current global leaderboard view. The response still contains user names, user ids, exact spend, request counts, model use, and tool-cardinality totals. This is an accepted cross-team disclosure because the page is a platform-wide ranking. Page config does not remove direct API access to this view.
- Agent Harnesses: allow every active signed-in user to read the current global harness-usage view.
- Service Accounts: allow every active signed-in user to list service accounts for their own team. Platform admins still see all teams. Teamless users receive an empty list.
- Service-account credential metadata: keep the current rule. Platform admins and team owners or admins can see team service-account API-key metadata. Ordinary team members must see an explicit restricted state, not the false text `No credential attached`.

Use `require_active_session` for the two global observability reads. Add a short-lived response cache keyed by range for each global view. Use single-flight loading so concurrent cache misses do not repeat the same aggregate queries. Record cache hit, miss, and load duration metrics.

Extend the service-account list authorization to active team members. Add store methods that load one active team and its active service accounts by `team_id`. Use them for regular users instead of loading all teams and accounts before filtering. Keep the current unscoped store calls for platform admins.

### Security boundary

Do not use page grants as API authorization. Existing rules remain in place except for the three read changes above:

- platform-wide write APIs still require `platform_admin`
- personal spend, request logs, MCP invocations, API keys, and identity directories keep their current data scope
- service-account create, update, and disable operations still allow only platform admins or an owner/admin of the matching team
- identity mutations retain both centralized and handler-level platform-admin checks

Page config is a console policy. It is not a general-purpose RBAC engine.

## Implementation Phases

Keep this as one feature change because the session contract, shared reads, and UI routing must move together.

### 1. Add typed config and defaults

Files:

- `crates/gateway/src/config.rs`
- `crates/gateway/src/config/permissions.rs`
- config unit tests in `crates/gateway/src/config.rs` or the new module

Add `permissions` to `GatewayConfig`. Implement default expansion, partial replacement, stable ordering, capability ceilings, and validation.

### 2. Resolve group and return effective permissions

Files:

- `crates/gateway/src/main.rs`
- `crates/gateway/src/http/state.rs`
- `crates/gateway/src/http/identity.rs`
- `crates/gateway/src/http/admin_contract.rs`
- `crates/gateway/openapi/admin-api.json`
- `crates/admin-ui/web/src/generated/admin-api.ts`

Build the compact runtime policy once, add it to `AppState`, resolve team membership for session views, and extend the generated contract.

No database migration is needed.

### 3. Expand the shared read APIs

Files:

- `crates/gateway/src/http/observability.rs`
- `crates/gateway/src/http/identity.rs`
- `crates/gateway-store/src/store.rs`
- `crates/gateway-store/src/libsql_store/identity.rs`
- `crates/gateway-store/src/libsql_store/mod.rs`
- `crates/gateway-store/src/postgres_store/identity.rs`
- `crates/gateway-store/src/postgres_store/mod.rs`
- related gateway handler and contract tests

Allow active users to read Leaderboard and Agent Harnesses. Cache the shared aggregate responses for a short time. Extend the service-account list handler to active team members, and load only their team data. Do not widen any write route.

### 4. Make UI navigation and routing permission-driven

Files:

- `crates/admin-ui/web/src/components/layout/admin-nav.ts`
- `crates/admin-ui/web/src/components/app-sidebar.tsx`
- `crates/admin-ui/web/src/components/layout/app-shell.tsx`
- `crates/admin-ui/web/src/routes/-auth-routing.ts`
- `crates/admin-ui/web/src/routes/-admin-guard.ts`
- `crates/admin-ui/web/src/routes/__root.tsx`
- `crates/admin-ui/web/src/routes/index.tsx`
- new `crates/admin-ui/web/src/routes/no-access.tsx`
- pages with links or actions that target another configurable page

Filter navigation, route access, and cross-page actions from `session.permissions.pages`. Keep `admin-nav.ts` as the only UI path-to-page mapping. Load the session once in the root route and pass it to child routes through route context.

Update the Service Accounts page so ordinary team members see team service accounts without credential metadata. Show a clear restricted label when credential details are not available.

### 5. Update examples and policy documentation

Files:

- `gateway.yaml`
- `gateway.prod.yaml`
- `deploy/config/gateway.yaml`
- `deploy/config/gateway.local.yaml`
- `docs/configuration/configuration-reference.md`
- `docs/access/identity-and-access.md`
- `docs/access/admin-control-plane.md`
- a new ADR under `docs/adr/`

Use the Configuration Reference as the canonical syntax page. Use Identity and Access as the canonical page for group inheritance and data scope. Keep the Admin Control Plane page concise and link to those owners.

Add a commented or explicit permissions example that produces the default unions without repeated shared pages. The ADR must record the three-group derivation, union rules, duplicate handling, capability ceilings, session-owned effective policy, shared read changes, the accepted global leaderboard disclosure, and the separation between page visibility and API authorization.

### 6. Add cross-layer tests

Rust config and policy tests:

- omitted config produces the new 10-page user set and 13-page platform-admin set
- `team_admins` inherits `users.pages`
- `platform_admins` inherits both lower page sets
- an override replaces only the direct grants for that group
- repeated grants within or across groups are accepted and removed from the effective set
- empty direct page lists still receive lower-role grants where applicable
- an empty final page set resolves to no access
- unknown fields, unknown pages, and invalid defaults fail
- unsupported group-to-page grants fail
- deterministic page order does not depend on YAML map order

Session contract tests:

- platform admin membership does not lower the platform group
- team owner and team admin resolve to `team_admins`
- team member and teamless user resolve to `users`
- membership changes appear on the next session response
- each login or password response returns the same permission shape as session lookup
- a platform-admin session does not query team membership

UI unit tests:

- sidebar sections contain only allowed pages
- breadcrumbs use the filtered registry
- direct hidden paths redirect to the configured default
- an allowed child route maps to its parent page identifier
- an empty page set renders `/no-access`
- login redirect targets are accepted only when their page is allowed
- default users see Leaderboard, Agent Harnesses, and Service Accounts
- ordinary team members see a restricted credential state on Service Accounts
- cross-page links and actions are hidden when the target page is not effective
- root and child loaders share one session lookup per navigation

End-to-end tests:

- add a regular member and a team admin fixture
- verify the inherited default matrix before an override
- hide one regular-user page and verify both sidebar removal and direct-URL redirect
- repeat one user page under both admin groups and verify config is accepted without duplicate navigation items
- verify Leaderboard and Agent Harnesses load for an ordinary user
- verify the global leaderboard fields and cross-team disclosure for an ordinary user
- verify a hidden Leaderboard page remains available through its authenticated read API
- verify an ordinary team member can open Service Accounts and sees only that team's accounts
- verify the ordinary member cannot see service-account credential metadata or use service-account write APIs
- verify a teamless user sees the Service Accounts empty state
- verify API authorization still rejects unsupported operations regardless of UI visibility

Performance tests:

- repeated and concurrent global observability requests reuse the range cache
- cache expiry refreshes each global view
- team-member service-account reads query only the member's team
- query plans for the 7-day and 31-day global ranges use the expected time indexes

## Verification

Run all repo commands through `mise`:

```bash
eval "$(/Users/ahstn/.local/bin/mise activate zsh)"
mise run admin-contract-generate
mise run admin-contract-check
mise run ui-check
mise run rust-test
mise run e2e-test
mise run docs:check
mise run docs:verify
mise run lint
```

## Acceptance Criteria

- A gateway with no `permissions` object gives users the seven PR #261 pages plus Leaderboard, Agent Harnesses, and Service Accounts.
- Admins can replace the direct page grants for any group without repeating inherited pages.
- Team admins receive the union of `users.pages` and `team_admins.pages`.
- Platform admins receive the union of all three page lists.
- Repeated grants within or across groups do not fail and do not create duplicate navigation items.
- Team owners and team admins resolve to `team_admins`; platform admins always resolve to `platform_admins`.
- Hidden pages do not appear in navigation and cannot be opened by direct URL.
- A hidden-page redirect always reaches an allowed page or `/no-access`.
- Invalid or unsupported page grants fail during config load with a specific error.
- Session responses contain the resolved group, effective pages, and optional default page.
- All active users can read the global Leaderboard and Agent Harnesses views.
- The global leaderboard disclosure is documented and covered by a cross-team response test.
- Active team members can list their own team's service accounts, while teamless users receive an empty list.
- Team-member service-account reads do not load all teams or all service accounts.
- Service-account credential metadata and all write authorization keep their current stricter rules.
- Each navigation loads the auth session once, and cross-page actions respect the target page grant.
- Shared global observability responses use bounded range caches with single-flight loading.
- Config, Rust, UI, contract, E2E, docs, and lint checks pass.
