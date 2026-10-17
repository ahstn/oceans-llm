# ADR: Delegated MCP Server Ownership

- Date: 2026-10-17
- Status: Accepted
- Issue: [#445](https://github.com/ahstn/oceans-llm/issues/445), part of [#458](https://github.com/ahstn/oceans-llm/issues/458)
- Builds on: [Admin Audit Log](./2026-10-15-admin-audit-log.md)

## Current state

- [MCP Servers: Ownership and Visibility](../configuration/mcp-servers.md#ownership-and-visibility)
- [MCP Tool Access: Team-Managed Servers](../mcp/mcp-tool-access.md#team-managed-servers)

## Context

Every MCP admin route required a platform admin. Each new server, tool grant and credential change went through a small central group. Teams that could not wait connected MCP servers to their clients directly, outside grants, guardrails and invocation logs. Servers also had no recorded owner, so nobody knew who to ask about one.

Teams already have Owner, Admin and Member roles, and Spend Controls already lets team owners and admins manage their own team's budgets. A user belongs to at most one team.

## Decision

### 1. Ownership

`external_mcp_servers` gains three columns (migration `V67__mcp_server_ownership.sql`, both backends):

| Column | Meaning |
|---|---|
| `owner_team_id` | The owning team. `NULL` means the server is platform-owned. |
| `owner_contact` | Free text, up to 256 characters: an email address, a chat channel or an on-call alias. |
| `visibility` | `team` or `organisation`. |

Existing servers become platform-owned and `organisation`, so nothing changes for them. The team reference restricts deletion: a team that still owns servers cannot be deleted, so a team-only server never loses its owner. Teams are deactivated, not deleted, today. The API returns `owner_team_id: null` for a platform-owned server, and the UI shows its owner as "Platform admins", so it cannot be mistaken for a team named "Platform".

A team owner or admin who registers a server always registers it for their own team. The contact defaults to their email address. A platform admin may register a server for any team, or for the platform, with the `Owning team` field in the admin UI or `owner_team_id` in the API.

### 2. Visibility

Visibility never grants access by itself. Grants still decide who can call a tool. Visibility decides who may be granted the server's tools, and who sees the server in the admin catalog.

- **`team`**, the default for a team-owned server. Its tools can be granted only to subjects inside the owning team: the team itself, its members, its service accounts, and API keys that one of those owns. This holds for every actor, platform admins included. Its tools cannot be added to a tool set, because tool sets are granted organisation-wide. Other teams' admins do not see the server.
- **`organisation`**. The server appears in every team admin's catalog, read-only for teams that do not own it. Platform admins may grant its tools to anyone and add them to tool sets.

A platform-owned server is always `organisation`.

Narrowing a server (to `team`, or to a different owning team) is rejected with `409 Conflict` while it would strand access: an active grant on one of its tools held by a subject outside the new owning team, or one of its tools in an active tool set (a disabled tool set, or a disabled server, exposes nothing). Only a grant someone could use counts, as at runtime: not one held by a revoked key, a key without an MCP scope or whose server list leaves the server out, or a service account (or its key) that is disabled or has no active budget. The error says which. The check runs inside the narrowing write, with the server row locked, whether the narrowing is a transfer or part of a settings save; a settings save that narrows is one write, so a refused narrowing saves none of the other settings and a saved one never needs undoing. The admin revokes those first. Widening is never blocked.

### 3. Publishing to the organisation

Setting `organisation` visibility is publishing. Who may publish is a platform setting:

```yaml
mcp:
  registry:
    organisation_publishing: platform_admins   # or team_admins
```

- `platform_admins` (default): only a platform admin can publish a server. A team admin can still unpublish their own server.
- `team_admins`: a team owner or admin can also publish servers their team owns.

### 4. What team owners and admins can do

The checks live in one place, `gateway_service::McpOwnership` (`crates/gateway-service/src/mcp_ownership.rs`). Handlers resolve the caller to an `McpAdminScope` (platform, or team with role) with `require_mcp_admin_scope` and ask it. `authorize_grant` checks one grant write; `authorize_server_grant` checks a subject against one server, so tag-based grants (#443) can call it for each server a tag reaches, and tool approval (#444) can gate on `managed_server`. Grant resolution itself is unchanged.

Team owners and admins reach a scoped MCP area when they have the `mcp` page, which every group gets by default. Every other signed-in user also gets the `mcp` page by default, through the `users` group. They see, read-only, only the servers they can connect to: those where a grant to them, their team, or one of their own API keys reaches an active tool. Each server shows its owner, its URL's origin and the tools they can call. Every write and the grant, credential and tool set routes stay admin-only.

| Action | Team owner or admin |
|---|---|
| List servers | Their team's servers, and every `organisation` server read-only |
| Register a server | Yes, owned by their team, at a public address |
| Edit, disable, refresh discovery | Their team's servers only |
| Change visibility and contact | Their team's servers. Publishing follows the platform setting |
| Change the owning team | No |
| List tools | Servers they can see |
| Grant tools | Tools of their team's servers, to subjects inside their team |
| Revoke grants | Any grant on their team's servers' tools, because revoking only removes access |
| Tool sets | No. Tool sets are platform-managed |
| Credential bindings | List, on their team's servers. Set: team and service-account scopes in their team. Revoke: any binding on their team's servers |
| Effective access preview | Subjects in their team, limited to one of their team's servers. An API key's scopes and server list apply, as they do at runtime |
| OAuth discovery, connection info, recommended servers | Yes. These are reads |

A server or tool that a team admin cannot see returns `404`, so its existence does not leak. An action they cannot take on a server they can see returns `403`. For a server they can see but not manage, the view shows only the URL's origin (`scheme://host[:port]`), because any path segment could be an opaque credential, and leaves out the auth config and the discovery error summary.

### 5. Gateway-managed credentials

A team admin may set gateway-managed credentials for their own team's servers, but only for the team scope or one of the team's service accounts (whose binding also names the team), and only by storing the secret itself, encrypted. A `secret_ref` names a gateway-wide environment variable, which could hold another team's or the platform's credential, so it is platform-only. For the same reason a team admin cannot add or retarget an `env/...` reference in a server's auth config: a reference a platform admin set stays only while it, the server URL and every URL in the auth config are unchanged, since the gateway sends that secret to the token endpoint the config names. Replacing a binding by ID is checked against the binding being replaced too, so an ID cannot reach another team's binding. They own the upstream relationship, and a team-scoped credential is the shared credential that team's callers use. Personal, user-scoped credentials stay with each user, through OAuth connect, or with a platform admin. A team admin can revoke any binding on their team's servers, including a user's, because revoking only removes access. A team admin cannot change a server's URL while it holds a user-scoped, `secret_ref` or other team's binding, because the gateway would send that credential to the new destination; they revoke it first (`409` otherwise). The store counts those bindings inside the settings write, under the server lock, and every binding write is pinned to the server URL and auth settings it was authorized against, so a binding and a retarget, or a new token endpoint, racing each other cannot both land. A write naming a binding ID is pinned to that binding as it was checked, or to its absence, so a binding changed or created with the ID in between is never overwritten. A binding never moves to another server. Changing a server's URL, auth mode or auth config, by anyone, drops its dynamically registered OAuth client and revokes its users' OAuth tokens in the same write, so a refresh token never goes to an endpoint that did not issue it; users connect again. The same write deletes the server's outstanding OAuth states, so a flow started before the change cannot finish against the new endpoints, and a callback confirms its state still exists after reloading the server. An OAuth callback's token write and a dynamic registration pin the URL and auth settings they ran against in their guard, so neither can land after such a change, and a callback rechecks the user's team after storing the token. Every settings save is likewise pinned to the URL and auth settings it was checked against. A team-only server is hidden from the personal connection routes for users outside its current owning team, checked again at the OAuth callback.

### 6. Platform admins

Platform admins keep every action on every server. They take over ownership with:

```
PUT /api/v1/admin/mcp/servers/{server_id}/owner
{ "owner_team_id": null | "<team uuid>", "owner_contact": "...", "visibility": "team" | "organisation" }
```

`owner_team_id` is required, so a request that only changes the contact cannot move a server by accident. `null` moves the server to the platform and forces `organisation`, which can never strand access. Moving it to a team follows the narrowing rule in section 2. The route is platform-admin only, so a team admin cannot give a server away or claim another team's.

### 7. Audit

Every mutation goes through the admin audit boundary as before. MCP entries now also carry the actor's scope in their metadata:

- `actor_access`: `platform_admin` or `team_admin`.
- `actor_team_id` and `actor_team_role` (`owner`, `admin` or `member`), when the actor belongs to a team.

Server summaries include `owner_team_id`, `owner_contact` and `visibility`, so a publish or unpublish shows up as a `visibility` change in `mcp_server.updated`. A takeover is `mcp_server.ownership_transferred`. When a settings change revokes users' OAuth tokens, each revoke is recorded as `mcp_credential_binding.revoked` with `operation: server_settings_changed`, so every connection's `mcp_credential_binding.set` has a matching revoke. They are written in multi-row inserts of up to 200 entries, so a server with many connected users still costs a few round trips. A callback that undoes its own connection after the user lost access records `operation: oauth_access_lost`.

## Trade-offs

- **Writes that depend on ownership check it inside the write.** A settings save and a disable name the owner (and a save, the visibility) they were authorized against, and the store rejects them with `409` under the same write if either has changed, so a former team cannot overwrite or disable a server mid-transfer, or republish one another request narrowed. Grant and credential binding writes carry the server's owner and visibility the same way (`McpServerOwnershipGuard`), and a credential write its URL too. The store reads the server row inside the write's transaction, locked (libSQL transactions hold the write lock; Postgres takes `FOR SHARE` on the row), and returns `409` without writing anything if the server has moved. A tool set write reads every named tool's server the same way and refuses tools of a team-only server, even when the set is unchanged. A save that omits the contact keeps the one read under the lock. From the other side, a transfer or a narrowing to `team` counts the grants held outside the team (a revoked API key, or one that cannot call the server, holds none) and the active tool sets inside its own write, with the server row locked: a grant write that started first holds the row until it commits and is counted, and one that starts after is refused by its own guard. A narrowing is therefore never committed and then undone. A discovery refresh stores its result only while the server keeps the owner, visibility, URL and auth settings it started with, checked the same way. A team admin's grant or credential revoke holds only while the server keeps the owner it was authorized against.
- **Team-only visibility is enforced when access is resolved, not by rewriting grants.** A grant row on a `team` server stays after its holder leaves the team, but it no longer works: on every request, the per-server routes and the aggregate endpoint drop a team-only server's tools unless the caller's current team (the key's own, or its owner's) owns it. The member view and the access preview apply the same rule, `McpServerOwnership::is_reachable_from`, which the store's hidden-server queries implement in SQL and a store test holds them to, and the member's server list and tool list check the member's team again after their reads. The grant-resolution queries are unchanged: a request naming a server checks only that server, and the aggregate endpoint filters the servers it already loaded. The team admin, or a platform admin, can still revoke the inert grant. A server-to-client event stream is checked the same way every 30 seconds while it is open, whatever the server's visibility when it opened: an API key is authenticated again (so a revoked key, or a service account that is disabled or loses its budget, ends it), and the caller must still hold a grant on a server they can see. A check that cannot be answered also ends the stream, so a store outage never extends access; the client reconnects and is checked afresh. An OAuth caller's token is watched separately, as before.
- **No per-team quotas on registration.** A team admin can register many servers. Discovery is admin-triggered and bounded by the existing timeout, and servers are visible to platform admins with their owner.
- **Team admins reach public addresses only.** When a team admin saves a server URL, or a URL in the auth config such as a token endpoint, runs `oauth-discovery` or refreshes discovery, the host must resolve, and must not be, or resolve to, a loopback, private, link-local, shared or other non-public address. A team admin's OAuth discovery runs through the public scope of the SSRF-safe outbound fetcher ([SSRF-safe outbound fetch](./2026-10-17-ssrf-safe-outbound-fetch.md)), which checks and pins every address at connect time, for the server and every hop the upstream names. Token exchange and registration for a team-owned server are scoped by each endpoint's own URL rather than the server's, so an endpoint the team edited cannot borrow the private reach of a server a platform admin placed on an internal network. Platform admins register internal servers, including for a team, and the owning team can still edit them while the URL is unchanged. A team admin cannot refresh such a server, unless its host is listed under `mcp.oauth.private_network_hosts`, which still holds it to private networks at connect time. Discovery refresh and the MCP proxy, on the per-server routes and the aggregate endpoint, send a team-owned server's traffic through the same guarded resolver (`McpUpstreamClients`), so a name that later resolves to an internal address is refused when it connects. The scope comes from the URL as written, never from a DNS answer: a hostname reaches public addresses only, unless it is listed under `mcp.oauth.private_network_hosts`, and an IP address or `localhost` keeps its own kind, which a team admin could only have saved if it was public. A literal never reaches the resolver, so a team-owned server whose URL is an address no scope allows, such as a link-local or cloud metadata address, is refused before any request. A platform admin who registers an internal server for a team by hostname lists that host there. A team admin cannot save a server URL, or an auth-config endpoint, on a listed host, since the list widens what a name may reach and only a platform admin vouches for it. This traffic also skips any HTTP proxy set in the environment and follows no redirects. A platform-owned server's traffic is unchanged.

## Follow-up

- Let teams own tool sets made only of their own tools.
- Show the actor's team role as a column in the audit log UI.
