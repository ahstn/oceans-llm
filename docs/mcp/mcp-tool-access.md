# MCP Tool Access

`See also`: [MCP Servers](../configuration/mcp-servers.md), [MCP Client Setup](mcp-client-setup.md), [Identity and Access](../access/identity-and-access.md), [Budgets](../access/budgets.md), [MCP Invocations](mcp-invocations.md)

MCP tool access controls which discovered tools an Oceans API key can find through `/mcp` and see or call through `/mcp/{server_key}`.

No implicit access exists for every discovered MCP tool. A caller can use a tool only when an active grant resolves to that tool for the authenticated API key, owner user, owner service account, or team.

## Toolsets

A toolset is a named collection of discovered MCP tools. Use toolsets when API keys, users, service accounts, or teams need the same tool bundle.

Toolsets contain stable `mcp_tool_id` values from discovery. If an upstream tool changes schema, the tool keeps the same id and gets a new schema version. If discovery later marks a tool inactive, grants and toolset membership remain auditable, but the inactive tool is not callable.

Admins manage toolsets from the MCP workspace:

```text
/admin/mcp/toolsets
```

![MCP toolsets page](../public/images/mcp-toolsets-page.png)

Typical flow:

1. Register or import an MCP server.
2. Refresh discovery so the gateway stores current tools and schemas.
3. Open the server detail dialog and select active tools from the **Tools** tab,
   or create a toolset directly from the **Toolsets** tab.
4. Save the toolset membership.
5. Grant the toolset to the API key, user, team, or service account that should
   see those tools.

Toolsets can contain tools from more than one registered server. Keep them small
and purpose-oriented; they are the main way to keep agent-visible tool catalogs
understandable.

Toolsets are platform-managed and granted organisation-wide, so they hold only
tools of servers published to the organisation. Saving a toolset that holds a tool
of a `team` server returns `409`, even when the set is unchanged, so a server
narrowed to its team cannot keep tools in one. See
[Team-Managed Servers](#team-managed-servers).

## Grants

Grant subjects are:

- API key
- user
- team
- service account

Grant targets are:

- one tool
- one toolset

Effective access is the union of active grants for the API key and its owner. User-owned keys also receive active team grants through the user's current team membership. Service-account keys receive service-account grants and owning-team grants.

Teams are access metadata, not runtime owners. Spend, request ownership, and service-account budget checks still belong to the user or service account that authenticated.

Admins manage grants from:

```text
/admin/mcp/access
```

![MCP access grants page](../public/images/mcp-access-page.png)

The Access tab supports:

- creating a grant from a real subject picker and a real tool/toolset picker
- revoking existing grants
- previewing effective access for a subject, optionally scoped to one server

Use direct tool grants for exceptions. Prefer toolset grants when teams, service
accounts, users, or API keys need the same bundle.

## Team-Managed Servers

Team owners and admins manage access for the servers their team owns. They use the
same Access tab, limited to their team:

- The target is always one tool of one of their team's servers. Toolsets are
  platform-managed.
- The subject must be inside their team: the team itself, a member, one of the
  team's service accounts, or an API key owned by one of those. Granting to anyone
  else returns `403`.
- They can revoke any grant on their team's servers' tools, including one a
  platform admin made.
- They see only grants on their team's servers' tools.
- The access preview needs one of their team's servers and a subject inside their
  team.

For every admin, previewing an API key applies that key's scopes and server list
as the gateway does at runtime: a key without an MCP scope, or limited to other
servers, previews no tools.

A server's visibility bounds every actor, platform admins included. A `team`
server's tools can be granted only to subjects of its owning team; granting one to
anyone else returns `400`. Publish the server to the organisation first. See
[Ownership and Visibility](../configuration/mcp-servers.md#ownership-and-visibility).

Visibility is checked when a grant is written, inside the same write: if the
server changes owner or visibility while the grant is saved, it returns `409` and
nothing is written. A user grant stays after the user leaves the team, like any
other user grant, so revoke it when someone moves teams.

Every grant change is written to the [admin audit log](../access/admin-audit-log.md)
with the actor's `actor_access` (`platform_admin` or `team_admin`) and, when they
belong to a team, `actor_team_id` and `actor_team_role`.

## Runtime Behavior

For aggregate `/mcp`, `tools/list` exposes only gateway-owned tools: `search_tools`, `describe_tool`, and `call_tool`. `search_tools` searches the caller's granted active tools across registered servers. `describe_tool` returns the exact persisted schema and metadata for a granted canonical address such as `mcp://github/tools/issues.create`, including the upstream `title`, `output_schema`, and `annotations` when the server declares them. `search_tools` results include the `title` and `annotations`. `call_tool` re-checks the grant, resolves the upstream credential, and calls the upstream server.

For direct `/mcp/{server_key}`, Oceans filters the upstream `tools/list` response to only granted, active tools on the requested server. Granted tool objects pass through unchanged, including `title`, `annotations`, and `outputSchema`. The gateway supports JSON responses and finite `text/event-stream` responses for this rewrite.

For direct `tools/call`, Oceans checks access before contacting the upstream MCP server. Unauthorized calls return a deterministic MCP JSON-RPC error, and the gateway logs them as policy-denied invocations.

### Other methods on direct `/mcp/{server_key}`

Resources, prompts, and completions can return server data just as tools can, so the direct proxy classifies every JSON-RPC method before it contacts the upstream server:

| Method | Gateway behavior |
| --- | --- |
| `initialize` | Forwarded. The gateway removes the `tasks` capability from the result. |
| `ping`, `logging/setLevel`, `notifications/initialized`, `notifications/cancelled`, `notifications/progress`, `notifications/roots/list_changed` (no `id`), client replies to server requests, `DELETE` (end session) | Forwarded. These carry no server data. |
| `tools/list`, `tools/call` | Filtered and checked per tool, as described above. `tools/list` results lose each tool's `execution.taskSupport`, and a `tools/call` with a `task` param is rejected with HTTP 400. |
| `resources/list`, `resources/templates/list`, `resources/read`, `resources/subscribe`, `resources/unsubscribe`, `prompts/list`, `prompts/get`, `completion/complete` | Forwarded only when the caller has at least one effective grant on the server. Otherwise the gateway returns HTTP 403 with JSON-RPC code `-32001` and logs a policy-denied invocation with error code `mcp_server_not_granted`. |
| `GET` (server-to-client event stream) | Forwarded only when the caller has at least one effective grant on the server. The stream is not logged per call. |
| Any other method | Denied with HTTP 403 and JSON-RPC code `-32601`, and logged with error code `mcp_method_not_allowed`. |
| `POST` with an empty or malformed body, or `GET`/`DELETE` with a body | Rejected with HTTP 400 and JSON-RPC code `-32600`. |

The gateway does not proxy MCP tasks. A task's result comes back through `tasks/result`, which names no tool, so the gateway could not check grants or guardrails on it. `tasks/*` methods fall under "any other method", and the gateway removes the capability and tool hints above so clients call tools directly.

Grants are per tool, and resources and prompts have no grant model of their own. A grant on any tool on the server is therefore the minimum bar for reading its resources or prompts. Use the server's tool grants to decide who may use the server at all.

The gateway applies result guardrails to `resources/read` and `prompts/get` responses, including finite `text/event-stream` responses, in the same way as `tools/call` results. List and completion results are not result-guarded, which matches `tools/list`. Resource and prompt requests are not checked against request guardrails.

Each governed call is recorded on the MCP Invocations page. The `Method` column holds the JSON-RPC method. The `Target` column shows the tool name for `tools/call`, the resource URI for `resources/read` and the resource subscription methods, and the prompt name for `prompts/get`.

Before this change, the direct proxy forwarded every method other than `tools/list` and `tools/call` once it had resolved the upstream credential, without checking grants, guardrails, or logging. Any valid API key could read resources and prompts from any active server. See [Govern every MCP method on the direct proxy](../adr/2026-10-13-govern-every-mcp-method.md).

`call_tool` input uses:

```json
{
  "address": "mcp://github/tools/issues.create",
  "arguments": {},
  "schema_hash": "sha256:optional"
}
```

If the caller supplies `schema_hash` and it no longer matches the persisted tool schema, title, annotations, or output schema, the gateway returns `tool_schema_changed` without contacting the upstream server. Missing upstream credentials return `credential_required`; expired bindings return `credential_expired`.

Disabled servers, disabled toolsets, inactive tools, revoked grants, and inactive team memberships do not resolve as callable access.

## What Clients Can See

MCP clients never see the global registry. The gateway derives their visible
tools from effective access at request time:

- aggregate `/mcp` exposes the gateway tools `search_tools`, `describe_tool`,
  and `call_tool`; search and describe return only granted active catalog tools
- direct `/mcp/{server_key}` exposes only granted active upstream tools on that
  server
- the gateway rejects denied direct calls before it contacts the upstream MCP server
- direct resources, prompts, and completions need at least one effective grant
  on the server, and unknown methods are denied
- missing or expired upstream credentials fail after grant checks, so denied
  callers do not learn whether a credential exists

## Relation To Budgets

MCP grants decide tool visibility and call permission. They do not create spend budgets.

Service-account MCP credentials and service-account budgets are separate controls. A service account can have an upstream credential binding while another budget policy blocks it. It can also have budget capacity but no MCP credential for a particular upstream server. User-facing budget setup remains in [Budgets](../access/budgets.md).

MCP token-overhead estimates are context-window telemetry. They estimate how many prompt-context tokens granted tool definitions and tool results may consume, but they are not billing truth and do not count toward spend-budget accounting.
