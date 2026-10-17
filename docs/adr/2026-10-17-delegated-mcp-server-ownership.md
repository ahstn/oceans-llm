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

`external_mcp_servers` gains three columns (migration `V65__mcp_server_ownership.sql`, both backends):

| Column | Meaning |
|---|---|
| `owner_team_id` | The owning team. `NULL` means the server is platform-owned. |
| `owner_contact` | Free text, up to 256 characters: an email address, a chat channel or an on-call alias. |
| `visibility` | `team` or `organisation`. |

Existing servers become platform-owned and `organisation`, so nothing changes for them. The team reference restricts deletion: a team that still owns servers cannot be deleted, so a team-only server never loses its owner. Teams are deactivated, not deleted, today. The API returns `owner_team_id: null` for a platform-owned server, and the UI shows its owner as "Platform admins", so it cannot be mistaken for a team named "Platform".

A team owner or admin who registers a server always registers it for their own team. The contact defaults to their email address. A platform admin may register a server for any team, or for the platform.

### 2. Visibility

Visibility never grants access by itself. Grants still decide who can call a tool. Visibility decides who may be granted the server's tools, and who sees the server in the admin catalog.

- **`team`**, the default for a team-owned server. Its tools can be granted only to subjects inside the owning team: the team itself, its members, its service accounts, and API keys that one of those owns. This holds for every actor, platform admins included. Its tools cannot be added to a tool set, because tool sets are granted organisation-wide. Other teams' admins do not see the server.
- **`organisation`**. The server appears in every team admin's catalog, read-only for teams that do not own it. Platform admins may grant its tools to anyone and add them to tool sets.

A platform-owned server is always `organisation`.

Narrowing a server (to `team`, or to a different owning team) is rejected with `409 Conflict` while it would strand access: an active grant on one of its tools held by a subject outside the new owning team, or one of its tools in a tool set. The error says which. The admin revokes those first. Widening is never blocked.

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

Team owners and admins reach a scoped MCP area when the `team_admins` permission group includes the `mcp` page, which it does by default. Members do not.

| Action | Team owner or admin |
|---|---|
| List servers | Their team's servers, and every `organisation` server read-only |
| Register a server | Yes, owned by their team |
| Edit, disable, refresh discovery | Their team's servers only |
| Change visibility and contact | Their team's servers. Publishing follows the platform setting |
| Change the owning team | No |
| List tools | Servers they can see |
| Grant tools | Tools of their team's servers, to subjects inside their team |
| Revoke grants | Any grant on their team's servers' tools, because revoking only removes access |
| Tool sets | No. Tool sets are platform-managed |
| Credential bindings | List, on their team's servers. Set: team and service-account scopes in their team. Revoke: any binding on their team's servers |
| Effective access preview | Subjects in their team, limited to one of their team's servers |
| OAuth discovery, connection info, recommended servers | Yes. These are reads |

A server or tool that a team admin cannot see returns `404`, so its existence does not leak. An action they cannot take on a server they can see returns `403`. For a server they can see but not manage, the view drops the URL's credentials, query and token-like path segments, its auth config and its discovery error summary.

### 5. Gateway-managed credentials

A team admin may set gateway-managed credentials for their own team's servers, but only for the team scope or one of the team's service accounts (whose binding also names the team). Replacing a binding by ID is checked against the binding being replaced too, so an ID cannot reach another team's binding. They own the upstream relationship, and a team-scoped credential is the shared credential that team's callers use. Personal, user-scoped credentials stay with each user, through OAuth connect, or with a platform admin. A team admin can revoke any binding on their team's servers, including a user's, because revoking only removes access.

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

Server summaries include `owner_team_id`, `owner_contact` and `visibility`, so a publish or unpublish shows up as a `visibility` change in `mcp_server.updated`. A takeover is `mcp_server.ownership_transferred`.

## Trade-offs

- **Checks run before the write, not inside it.** The narrowing check reads grants and tool sets, then writes. A platform admin granting an outside subject at the same moment as a team admin unpublishes could leave one stranded grant. The window is short, needs two admins, and the grant stays visible in the access list and the audit log. Moving these checks into the store's transaction would mean changing grant and tool set methods that parallel work is also changing.
- **Grants do not follow team membership changes.** A user grant on a `team` server stays after the user leaves the team, as user grants do today. Grant resolution is unchanged by this decision. The team admin, or a platform admin, revokes it.
- **No per-team quotas on registration.** A team admin can register many servers. Discovery is admin-triggered and bounded by the existing timeout, and servers are visible to platform admins with their owner.
- **OAuth discovery reaches arbitrary URLs.** Team admins can now run `oauth-discovery` and discovery refresh. Both already reach the URL a server admin enters, under the same URL validation. Deployments that need tighter outbound rules use network egress policy.

## Follow-up

- Enforce team visibility at grant resolution too, once tag-based grants (#443) land, so membership changes drop access automatically.
- Let teams own tool sets made only of their own tools.
- Show the actor's team role as a column in the audit log UI.
