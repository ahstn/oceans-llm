# MCP Tool Access

`See also`: [MCP Servers](../configuration/mcp-servers.md), [MCP Client Setup](../setup/mcp-client-setup.md), [Identity and Access](identity-and-access.md), [Budgets](budgets.md), [MCP Invocations](../operations/observability/mcp-invocations.md)

MCP tool access controls which discovered tools an Oceans API key can find through `/mcp`, see or call through `/mcp/{server_key}`, and reach from Code Mode sandboxes through `/code-mode-mcp`.

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

## Code Mode Behavior

When Code Mode is enabled, `/code-mode-mcp` code sees only the caller's granted tools. `oceans.searchTools` and `oceans.describeTool` resolve against the same effective grants as the aggregate catalog; ungranted tools do not appear in results, counts, or error detail.

Every nested `oceans.callTool` invocation re-checks grants at call time, exactly like aggregate `call_tool`. Grant changes take effect on the next call, even within a single running execution.

Error behavior splits into two contracts:

- Grant denials, capability denials, and invalid arguments **throw** catchable exceptions inside the sandbox.
- Structured tool errors (`credential_required`, `credential_expired`, `tool_schema_changed`) and upstream failures **resolve**: `await oceans.callTool(...)` returns the same aggregate-style result object `/mcp` clients see — `{content, isError: true, structuredContent: {error_code, ...}}`. Code must check `result.isError` before using a tool result; these errors do not throw.

The `explore` tool cannot call tools at all. `oceans.callTool` is only available in `execute`; an explore execution that attempts it gets a thrown exception, and an execution whose uncaught failure is that denial is logged as a policy-denied invocation.

## Relation To Budgets

MCP grants decide tool visibility and call permission. They do not create spend budgets.

Service-account MCP credentials and service-account budgets are separate controls. A service account can have an upstream credential binding and still be blocked by budget policy elsewhere, or have budget capacity but no MCP credential for a particular upstream server. User-facing budget setup remains in [Budgets](budgets.md).

MCP token-overhead estimates are context-window telemetry. They estimate how many prompt-context tokens granted tool definitions and tool results may consume, but they are not billing truth and do not count toward spend-budget accounting.
