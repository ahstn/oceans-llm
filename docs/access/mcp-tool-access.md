# MCP Tool Access

`See also`: [MCP Servers](../configuration/mcp-servers.md), [MCP Client Setup](../setup/mcp-client-setup.md), [Identity and Access](identity-and-access.md), [Budgets](budgets.md), [MCP Invocations](../operations/observability/mcp-invocations.md)

MCP tool access controls which discovered tools an Oceans API key can see and call through `/mcp/{server_key}`.

There is no implicit access to every discovered MCP tool. A tool is callable only when an active grant resolves to that tool for the authenticated API key, owner user, owner service account, or team.

## Toolsets

A toolset is a named collection of discovered MCP tools. Use toolsets when several API keys, users, service accounts, or teams should receive the same tool bundle.

Toolsets contain stable `mcp_tool_id` values from discovery. If an upstream tool changes schema, the tool keeps the same id and gets a new schema version. If discovery later marks a tool inactive, grants and toolset membership remain auditable, but the inactive tool is not callable.

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

## Runtime Behavior

For `tools/list`, Oceans filters the upstream response to only granted, active tools on the requested server. The gateway supports JSON responses and finite `text/event-stream` responses for this rewrite.

For `tools/call`, Oceans checks access before contacting the upstream MCP server. Unauthorized calls return a deterministic MCP JSON-RPC error and are logged as policy-denied invocations.

Disabled servers, disabled toolsets, inactive tools, revoked grants, and inactive team memberships do not resolve as callable access.

## Relation To Budgets

MCP grants decide tool visibility and call permission. They do not create spend budgets.

MCP token-overhead estimates are context-window telemetry. They estimate how many prompt-context tokens granted tool definitions and tool results may consume, but they are not billing truth and do not count toward spend-budget accounting.
