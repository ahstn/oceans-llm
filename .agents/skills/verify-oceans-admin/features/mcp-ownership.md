# MCP ownership

Team owners and admins reach a scoped MCP area. They register servers for their own team, see each server's owner and visibility, filter the registry by owner, and manage only their team's servers. Platform admins see every server, publish servers to the organisation, and move servers between teams or to the platform.

## Sub-features

- `mcp-owner-registry` shows the `Owner` column (`Platform admins` or the team name, with the contact as its title) and a visibility badge (`Team only` or `Organisation`) in `mcp-server-list`, and filters it with the `Filter by owner` select (`All owners`, `Platform admins`, then each team).
- `mcp-team-register` lets a team admin register a server through `Add server`. The form's `Visibility` select starts at `Team only`, `Organisation` is disabled with `Only a platform admin can publish this server to the organisation.` under the default `mcp.registry.organisation_publishing: platform_admins`, and `Owner contact` defaults to the admin's email.
- `mcp-team-read-only` shows servers the team does not manage with a `View` button instead of `Manage`, a disabled refresh, and an overview line `<owner> manages this server.` with no `Configuration` or `Credentials` sections and no edit or disable buttons.
- `mcp-team-access` limits the team admin's Access tab to `Tool` targets from their own servers and their own team's subjects, and hides the `Tool Sets` nav entry. `/admin/mcp/toolsets` redirects to `/admin/mcp?tab=servers`.
- `mcp-ownership-transfer` is the platform admin's `Ownership` panel in a server's `Overview`: the `Owning team` select and `Transfer ownership`.
- `mcp-ownership-audit` records `actor_access`, `actor_team_id`, and `actor_team_role` on each MCP audit entry, and `mcp_server.ownership_transferred` for a transfer.

## How to get to it (user POV)

- Sign in as a team admin, such as `cara@research.local` (password `localdemo123`). `MCP Servers` appears in the sidebar because `gateway.yaml` grants `team_admins` the `mcp` page. Open `/admin/mcp`.
- Sign in as `admin@local` / `admin` for the platform view of the same servers, the `Ownership` panel, and the Audit Log at `/admin/observability/audit-log`.
- A team member, such as `diego@research.local`, gets `MCP Servers` too, read-only: the list holds only servers a grant to them, their team or their own key reaches, with no `Add server`, `Browse catalog` or `Access`. With no grants the list is empty. Write routes and `GET /api/v1/admin/mcp/grants` return `403`.

## Driving it with control-oceans-admin

Preconditions:

- Use the owned local stack and require `control-oceans-admin doctor` to pass. The seeded demo data has no MCP servers, so the run registers its own. Use a public, credential-free endpoint such as `https://mcp.context7.com/mcp` with auth mode `None`, and refresh discovery only if upstream calls are acceptable for the run.
- Use a run-scoped server key, such as `verify-<run tail>-research`, and disable every server the run creates before stack cleanup.

Steps:

- **Team admin registers.** As `cara@research.local`, open `/admin/mcp` and use `Add server`. Check the `Visibility` select shows `Team only` and the publish hint, fill `Server key`, `Display name`, and `Server URL`, then submit. The new row's `Owner` cell reads `Research` with `Team only`. `GET /api/v1/admin/mcp/servers` returns the row with `owner_team_id` set, `visibility: "team"`, `can_manage: true`, and `can_publish_to_organisation: false`.
- **Publishing is blocked.** `PATCH /api/v1/admin/mcp/servers/{id}` with `"visibility": "organisation"` from the team admin's session returns `403`.
- **Platform view.** As `admin@local`, the same row appears with `Research` and `Team only`. Register a second server as the platform admin; its `Owner` reads `Platform admins` with `Organisation`. Use `Filter by owner` to select `Research`, then `Platform admins`, and check `Showing <n> of <total> servers` against the API each time.
- **Read-only for other owners.** Back as the team admin, the platform server shows `View <display name>`, its refresh is disabled, and its detail shows `Platform admins manages this server.` with no `Edit` or `Disable` buttons. `PATCH` and `POST .../disable` on it return `403`.
- **Scoped access.** Open `Access`. The `Grant target type` select offers only `Tool`, and `MCP sections` has no `Tool Sets`. Visiting `/admin/mcp/toolsets` lands on the servers registry.
- **Transfer.** As `admin@local`, open the team server's `Overview`, choose `Platform admins` in `Owning team`, and use `Transfer ownership`. The row's `Owner` becomes `Platform admins` with `Organisation`. In the Audit Log, filter `mcp_server.ownership_transferred` and open the entry: **Details** has `actor_access: platform_admin`, and the before and after show the owner change. The `mcp_server.created` entry for the team admin's server has `actor_access: team_admin` and `actor_team_role: admin`.
- **Clean up.** Disable both servers as the platform admin, then record the proof with `control-oceans-admin record mcp-ownership` and one `--check` per step above.

## Gotchas

- The demo seed has a team named `Platform`. Platform-owned servers are labelled `Platform admins`, so pick options by that exact label.

- `cara@research.local` must keep the `admin` (or `owner`) role in Research. A `member` gets a read-only list and `403` on every write; that is the expected boundary, not a regression.
- Explicit `team_admins.pages` lists in a gateway config replace the default. A config that omits `mcp` keeps team admins out of the MCP area even though the code default grants it. `gateway.prod.yaml` and the deploy configs set `pages: []`.
- The tool picker on `Access` lists tools only after discovery. Without a refresh, the team admin's server has no tools, so grant proof is limited to the empty picker; the role and boundary checks are proved by `http::mcp_registry::ownership_tests`.
- A transfer to another team, or narrowing a published server to `Team only`, returns `409` while the server's tools are in a tool set or granted outside the new owning team. Revoke first.
