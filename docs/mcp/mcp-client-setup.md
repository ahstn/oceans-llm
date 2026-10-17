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

Clients authenticate with an Oceans API key, or with an OAuth access token from the gateway when [OAuth sign-in](#oauth-sign-in) is enabled. The gateway does not accept provider tokens, upstream MCP tokens, or query-string credentials at this endpoint.

## Connection info in the admin UI

Open **MCP → Tool Sets**, select a tool set, and choose **Connection Info**. The dialog provides the gateway endpoint and copyable Claude Code and Codex configuration. Set `OCEANS_LLM_API_KEY` in the environment used to launch the client, using an Oceans API key owned by a user or service account.

Connection Info uses `GATEWAY_CLIENT_CONFIG_BASE_URL` from the gateway process, as the Models page does. If it is unset, the endpoint is `http://127.0.0.1:3000/mcp`. A terminal `/v1` is removed before `/mcp` is added; deployment path prefixes are preserved. For example, `https://gateway.example.com/oceans/v1` becomes `https://gateway.example.com/oceans/mcp`.

Tool sets share this endpoint. Use **Manage access** to grant the selected set to the API key or its owner. The client can use all tools granted to that key; choosing a tool set in the admin UI does not restrict the client to that set. Save tool selection changes before clients use them.

## Before Connecting A Client

Ask an admin to confirm:

- an admin registered the upstream MCP server and discovery succeeded
- the required tools are active in the server's **Tools** dialog
- a toolset or direct tool grant exists for the API key owner
- any required upstream credential binding exists for the user, team, or service
  account that owns the API key

For Google Drive or Google Docs, the user who owns the Oceans API key must also connect the matching server under `/admin/account/connections`. This browser consent is separate from Google sign-in to Oceans. A client harness does not perform Google OAuth. The recommended Google entries request read-only scopes and keep only the matching read tools active during discovery. Write tools require a separate scope and tool-policy decision.

If any of those steps are missing, the client may connect successfully but see no
matching tools, or receive `credential_required` when it tries to execute one. That
error links to the connection page when the user can fix it themselves; see
[Connecting from an error](#connecting-from-an-error).

## Inbound Auth

Preferred header:

```http
Authorization: Bearer <oceans-api-key>
```

Secondary explicit header:

```http
x-oceans-api-key: <oceans-api-key>
```

If both headers are present, they must contain the same raw Oceans key after Bearer extraction. The gateway rejects a malformed `Authorization` header even when `x-oceans-api-key` is valid.

Valid user-owned and service-account-owned Oceans API keys can use `/mcp`. Direct `/mcp/{server_key}` proxy calls can target active servers with gateway-managed credentials or principal-bound upstream credential bindings.

### Scoped API keys

Give an MCP client its own key, limited to MCP. Then a leaked key can't call models or spend budget. Under **API keys**, set **Capabilities** to **Limited** and choose:

| Scope | Allows |
| --- | --- |
| `mcp:aggregate` | `/mcp` and its `search_tools`, `describe_tool`, and `call_tool` |
| `mcp:proxy` | `/mcp/{server_key}` |
| `inference` | Model routes, batches, and guardrail checks. Leave it off for an MCP-only key |

To limit the key further, set **MCP servers** to **Selected servers** and pick the servers it may reach. The key then never sees a server outside its list, even when its owner holds grants there. A listed server still needs a grant before any tool shows up.

A refused request gets `403` with a JSON-RPC error. Its `id` is `null`, because the gateway refuses before it reads the request:

```json
{
  "jsonrpc": "2.0",
  "id": null,
  "error": {
    "code": -32001,
    "message": "credential lacks the `mcp:aggregate` scope",
    "data": {"error": "insufficient_scope", "required_scope": "mcp:aggregate"}
  }
}
```

A per-server request for a server outside the key's list gets the same error without `required_scope`. Unknown servers get the same answer, so the error doesn't reveal which servers exist. On `/mcp`, a tool on an unlisted server reads as `MCP tool address is not granted`, the same as a tool with no grant.

Keys created before scopes existed keep full access until an admin edits them. Scope edits apply to the key's next request, without reissuing it. See [API key scopes](../access/identity-and-access.md#api-key-scopes) for the full route list.

## OAuth sign-in

Clients that follow the MCP authorization specification can connect without an API key. A platform admin enables `mcp.client_oauth` (see the [configuration reference](../configuration/configuration-reference.md)). Users then add only the MCP URL to their client:

```text
https://gateway.example.com/mcp
https://gateway.example.com/mcp/{server_key}
```

The client handles the rest:

1. Its first request gets `401` with `WWW-Authenticate: Bearer resource_metadata="https://gateway.example.com/.well-known/oauth-protected-resource/mcp", scope="mcp:aggregate"`. A per-server URL names its own metadata document and `scope="mcp:proxy"`.
2. It reads the protected-resource metadata, then the authorization-server metadata at `/.well-known/oauth-authorization-server`.
3. It registers at `/oauth/register` (RFC 7591 Dynamic Client Registration).
4. It opens `/oauth/authorize` in the browser with PKCE (`S256`) and `resource` set to the MCP URL. The user signs in to Oceans the usual way, with a password, OIDC, or GitHub, and sees the consent page at `/admin/oauth/consent`. It names the client, the host it returns to, and the MCP access requested. The user chooses `Allow` or `Deny`.
5. It exchanges the code at `/oauth/token` for an `oat_` access token (one hour by default) and, when it registered for refresh, an `ort_` refresh token.
6. It sends `Authorization: Bearer oat_…` on every MCP request and refreshes when the token expires.

What the token can do:

- It acts as the user who approved it. User and team grants apply exactly as they do for that user's API keys. Grants made to one specific API key do not apply.
- Its scopes are the [scoped API key](#scoped-api-keys) names, checked the same way. A token for `https://gateway.example.com/mcp` with `mcp:aggregate` reaches the aggregate endpoint. Add `mcp:proxy` to also reach every per-server route. A token for `https://gateway.example.com/mcp/{server_key}` reaches only that server.
- It works only on `/mcp` routes, only in the `Authorization` header. Model, admin, batch, and Skills routes reject it with `oauth_token_not_accepted`.
- MCP invocation records show the user, with `auth_method`, `oauth_client_id`, and `oauth_grant_id` in their metadata.

When access ends:

- The user disconnects it under `/admin/account/connections`, in the `Authorized MCP clients` card.
- A platform admin revokes it through `POST /api/v1/admin/mcp/authorized-clients/revoke`.
- Signing out of the admin UI ends every client that session approved.
- Deactivating the user revokes all of their grants.
- The client can revoke its tokens at `/oauth/revoke`.
- Reusing a refresh token that was already rotated revokes the grant.
- Tokens never outlive the session that approved them. Expect to sign in again when that session expires (30 days).

Consent is asked every time a client signs in. When the code goes to a loopback redirect URI, the consent page shows a warning, because Oceans cannot verify who published the client. Client ID Metadata Documents are not supported yet, so clients fall back to registration.

API keys keep working on the same URLs. Clients that support custom headers can use either method.

## Claude Code

Add the HTTP MCP server with the aggregate gateway URL and an Oceans API key header:

```json
{
  "mcpServers": {
    "oceans": {
      "type": "http",
      "url": "https://gateway.example.com/mcp",
      "headers": {
        "Authorization": "Bearer ${OCEANS_LLM_API_KEY}"
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
headers = { Authorization = "Bearer ${OCEANS_LLM_API_KEY}" }
```

## Cursor

Use a Streamable HTTP server entry:

```json
{
  "mcpServers": {
    "oceans": {
      "url": "https://gateway.example.com/mcp",
      "headers": {
        "Authorization": "Bearer ${OCEANS_LLM_API_KEY}"
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

The aggregate endpoint issues an `MCP-Session-Id` during `initialize`. Clients must send that header on later aggregate requests. The gateway binds the session to the authenticated Oceans API key, so it reports a session id copied to another principal as not found.

For direct proxying, send requests to `/mcp/{server_key}`. The gateway preserves MCP response status, content type, and upstream MCP session headers. It strips inbound `Authorization` and `x-oceans-api-key` before proxying upstream.

## Aggregate Versus Direct Proxy

Use aggregate `/mcp` when:

- an agent should search across registered MCP servers
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

Oceans refreshes configured OAuth bundles before expiry. A revoked or non-refreshable grant returns `credential_required`. A binding whose expiry has passed returns `credential_expired`. Direct proxy responses do not forward the upstream server's `WWW-Authenticate` challenge because the client must reconnect through Oceans.

### Connecting from an error

When a call fails with `credential_required` or `credential_expired`, the error says what to do next.

- **The user can connect the server.** The response includes a `connect_url` when all of these hold:
  - the server uses user OAuth (`oauth_obo`)
  - its OAuth provider is configured
  - `mcp.oauth.public_base_url` is set
  - the Oceans API key is user-owned

  The URL has this form:

  ```text
  {public_base_url}/admin/account/connections?connect={server_key}
  ```

  Opening it in a browser starts the provider consent flow for that server, after Oceans sign-in if needed. The flow returns to the same page with a toast naming the connected server. The user then retries the call. The URL contains only the gateway origin and the server key. It carries no API key, token, or OAuth state.
- **Only an administrator can fix it.** For example, the server uses admin-managed bindings (`user_passthrough`), or the caller is a service account. Here the message says to contact an Oceans administrator, and no `connect_url` is returned.

Where the guidance appears depends on the route and method:

| Route and method | Response |
| --- | --- |
| Aggregate `call_tool` | A tool result with `isError: true`. The text holds the message and URL. `structuredContent` has `error_code`, `server_key`, `address`, and `connect_url`. |
| Direct `tools/call` | HTTP 200 with a tool result with `isError: true`. The text holds the message and URL. `structuredContent` has `error_code`, `server_key`, and `connect_url`. |
| Direct `initialize`, `tools/list`, and other methods | HTTP 403. The error envelope has `code`, `message`, and `connect_url`. |

The direct route needs an upstream credential before `initialize` succeeds, so a client without one fails while connecting. Most clients show the 403 message, which includes the URL. Tool results are used for `tools/call` so that clients which only show tool output still show the next step to the model and the user. A worked example of the aggregate result:

```json
{
  "isError": true,
  "content": [{
    "type": "text",
    "text": "MCP credential is required for server `google_drive`. Connect your account at https://gateway.example.com/admin/account/connections?connect=google_drive, then retry the call."
  }],
  "structuredContent": {
    "address": "mcp://google_drive/tools/search",
    "server_key": "google_drive",
    "connect_url": "https://gateway.example.com/admin/account/connections?connect=google_drive",
    "error_code": "credential_required"
  }
}
```
