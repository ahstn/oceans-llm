# MCP Client Setup

`See also`: [MCP Servers](../configuration/mcp-servers.md), [Identity and Access](../access/identity-and-access.md), [Service Accounts](../access/service-accounts.md)

MCP clients connect to Oceans through the gateway data-plane endpoint:

```text
https://<gateway-origin>/mcp/{server_key}
```

Use an Oceans API key for inbound auth. The gateway does not accept provider tokens, upstream MCP tokens, or query-string credentials at this endpoint.

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

Valid user-owned and service-account-owned Oceans API keys can call active servers that use `none`, `gateway_static_header`, or `gateway_bearer_token` upstream auth modes.

## Claude Code

Add the HTTP MCP server with the gateway URL and an Oceans API key header:

```json
{
  "mcpServers": {
    "github": {
      "type": "http",
      "url": "https://gateway.example.com/mcp/github",
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
[mcp_servers.github]
url = "https://gateway.example.com/mcp/github"
headers = { Authorization = "Bearer ${OCEANS_API_KEY}" }
```

## Cursor

Use a Streamable HTTP server entry:

```json
{
  "mcpServers": {
    "github": {
      "url": "https://gateway.example.com/mcp/github",
      "headers": {
        "Authorization": "Bearer ${OCEANS_API_KEY}"
      }
    }
  }
}
```

## Raw SDK Shape

Any Streamable HTTP MCP client should send requests to `/mcp/{server_key}` and include the protocol headers it normally sends to the upstream server.

```http
POST /mcp/github HTTP/1.1
Host: gateway.example.com
Authorization: Bearer gwk_...
Content-Type: application/json
Accept: application/json, text/event-stream
MCP-Protocol-Version: 2025-11-25
```

The gateway preserves MCP response status, content type, and MCP session headers. It strips inbound `Authorization` and `x-oceans-api-key` before proxying upstream.

## OAuth Boundary

Servers registered as `user_passthrough` or `oauth_obo` are visible in the registry but not proxyable yet. Calls to those servers return `403 mcp_upstream_auth_required` until Oceans has user-scoped OAuth credentials.
