# MCP Client Setup

`See also`: [MCP Servers](../configuration/mcp-servers.md), [Identity and Access](../access/identity-and-access.md), [Service Accounts](../access/service-accounts.md)

MCP clients normally connect to the aggregate gateway endpoint:

```text
https://<gateway-origin>/mcp
```

The aggregate endpoint exposes a small discovery surface:

- `search_tools`: search the tools granted to the authenticated API key across all registered MCP servers
- `describe_tool`: fetch the persisted schema and metadata for one granted tool address
- `call_tool`: call one granted tool by canonical `mcp://{server_key}/tools/{tool_name}` address

This is intentional. Oceans does not expose every granted upstream tool directly
from aggregate `tools/list`; that would make large registries noisy and expensive
for agents. Clients search first, describe the specific tool they intend to use,
and then call the canonical address.

Example aggregate call target:

```json
{
  "address": "mcp://context7/tools/query-docs",
  "arguments": {
    "libraryId": "/tanstack/router",
    "query": "How do route loaders work?"
  }
}
```

Direct per-server proxying remains available when a client should talk to one upstream server:

```text
https://<gateway-origin>/mcp/{server_key}
```

Use an Oceans API key for inbound auth. The gateway does not accept provider tokens, upstream MCP tokens, or query-string credentials at this endpoint.

## Before Connecting A Client

Ask an admin to confirm:

- the upstream MCP server is registered and discovery is successful
- the required tools are active in the server's **Tools** dialog
- a toolset or direct tool grant exists for the API key owner
- any required upstream credential binding exists for the user, team, or service
  account that owns the API key

If any of those steps are missing, the client may connect successfully but see no
matching tools, or receive `credential_required` when it tries to execute one.

## Inbound Auth

Preferred header:

```http
Authorization: Bearer <oceans-api-key>
```

Secondary explicit header:

```http
x-oceans-api-key: <oceans-api-key>
```

If both headers are present, they must contain the same raw Oceans key after Bearer extraction. A malformed `Authorization` header is rejected even when `x-oceans-api-key` is valid.

Valid user-owned and service-account-owned Oceans API keys can use `/mcp`. Direct `/mcp/{server_key}` proxy calls can target active servers with gateway-managed credentials or principal-bound upstream credential bindings.

## Claude Code

Add the HTTP MCP server with the aggregate gateway URL and an Oceans API key header:

```json
{
  "mcpServers": {
    "oceans": {
      "type": "http",
      "url": "https://gateway.example.com/mcp",
      "headers": {
        "Authorization": "Bearer ${OCEANS_API_KEY}"
      }
    }
  }
}
```

## Codex

Configure an HTTP MCP server that points at the same gateway endpoint:

```toml
[mcp_servers.oceans]
url = "https://gateway.example.com/mcp"
headers = { Authorization = "Bearer ${OCEANS_API_KEY}" }
```

## Cursor

Use a Streamable HTTP server entry:

```json
{
  "mcpServers": {
    "oceans": {
      "url": "https://gateway.example.com/mcp",
      "headers": {
        "Authorization": "Bearer ${OCEANS_API_KEY}"
      }
    }
  }
}
```

## Raw SDK Shape

Any Streamable HTTP MCP client should send discovery requests to `/mcp` and include normal MCP protocol headers.

```http
POST /mcp HTTP/1.1
Host: gateway.example.com
Authorization: Bearer gwk_...
Content-Type: application/json
Accept: application/json, text/event-stream
MCP-Protocol-Version: 2025-03-26
```

The aggregate endpoint issues an `MCP-Session-Id` during `initialize`. Clients must send that header on later aggregate requests. The session is bound to the authenticated Oceans API key, so a session id copied to another principal is treated as not found.

For direct proxying, send requests to `/mcp/{server_key}`. The gateway preserves MCP response status, content type, and upstream MCP session headers. It strips inbound `Authorization` and `x-oceans-api-key` before proxying upstream.

## Aggregate Versus Direct Proxy

Use aggregate `/mcp` when:

- an agent should search across multiple registered MCP servers
- admins want a small, stable tool surface for clients
- the client can call `search_tools`, `describe_tool`, and `call_tool`

Use direct `/mcp/{server_key}` when:

- the client must speak to one upstream server's normal MCP tool surface
- you are validating a single server's `tools/list` or `tools/call` behavior
- a client does not work well with the aggregate search/describe/call pattern

Both routes enforce the same Oceans API-key ownership model, grant checks, server
disablement, active-tool filtering, and upstream credential separation.

## Upstream Credentials

Oceans API keys are never forwarded upstream. For servers that need caller-specific credentials, platform admins create MCP credential bindings for a user, team, or service account. User-owned keys use a user binding first and then an allowed team binding. Service-account keys use a service-account binding first and then the owning-team binding. A service account never borrows a user's credential.

Missing upstream credentials return a structured `credential_required` tool error from aggregate `call_tool`, or a normal gateway error from direct `/mcp/{server_key}` proxying. Expired bindings return `credential_expired`.
