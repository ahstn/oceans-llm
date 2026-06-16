# MCP Tool Access

`See also`: [MCP Servers](../configuration/mcp-servers.md), [MCP Client Setup](../setup/mcp-client-setup.md), [Identity and Access](identity-and-access.md), [Budgets](budgets.md), [MCP Invocations](../operations/observability/mcp-invocations.md)

MCP tool access controls which discovered tools an Oceans API key can find through `/mcp` and see or call through `/mcp/{server_key}`.

There is no implicit access to every discovered MCP tool. A tool is callable only when an active grant resolves to that tool for the authenticated API key, owner user, owner service account, or team.

## Toolsets

A toolset is a named collection of discovered MCP tools. Use toolsets when several API keys, users, service accounts, or teams should receive the same tool bundle.

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

Use direct tool grants for exceptions. Prefer toolset grants when the same
bundle will be reused across teams, service accounts, users, or API keys.

## Runtime Behavior

For aggregate `/mcp`, `tools/list` exposes only gateway-owned tools: `search_tools`, `describe_tool`, and `call_tool`. `search_tools` searches the caller's granted active tools across registered servers. `describe_tool` returns the exact persisted schema and metadata for a granted canonical address such as `mcp://github/tools/issues.create`. `call_tool` re-checks the grant, resolves the upstream credential, and calls the upstream server.

For direct `/mcp/{server_key}`, Oceans filters the upstream `tools/list` response to only granted, active tools on the requested server. The gateway supports JSON responses and finite `text/event-stream` responses for this rewrite.

For direct `tools/call`, Oceans checks access before contacting the upstream MCP server. Unauthorized calls return a deterministic MCP JSON-RPC error and are logged as policy-denied invocations.

`call_tool` input uses:

```json
{
  "address": "mcp://github/tools/issues.create",
  "arguments": {},
  "schema_hash": "sha256:optional"
}
```

If `schema_hash` is supplied and no longer matches the persisted tool schema, the gateway returns `tool_schema_changed` without contacting the upstream server. Missing upstream credentials return `credential_required`; expired bindings return `credential_expired`.

Disabled servers, disabled toolsets, inactive tools, revoked grants, and inactive team memberships do not resolve as callable access.

## What Clients Can See

MCP clients never see the global registry. Their visible tools are derived from
effective access at request time:

- aggregate `/mcp` exposes the gateway tools `search_tools`, `describe_tool`,
  and `call_tool`; search and describe return only granted active catalog tools
- direct `/mcp/{server_key}` exposes only granted active upstream tools on that
  server
- denied direct calls are rejected before the upstream MCP server is contacted
- missing or expired upstream credentials fail after grant checks, so denied
  callers do not learn whether a credential exists

## Relation To Budgets

MCP grants decide tool visibility and call permission. They do not create spend budgets.

Service-account MCP credentials and service-account budgets are separate controls. A service account can have an upstream credential binding and still be blocked by budget policy elsewhere, or have budget capacity but no MCP credential for a particular upstream server. User-facing budget setup remains in [Budgets](budgets.md).

MCP token-overhead estimates are context-window telemetry. They estimate how many prompt-context tokens granted tool definitions and tool results may consume, but they are not billing truth and do not count toward spend-budget accounting.
