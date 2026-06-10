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

Direct per-server proxying remains available when a client should talk to one upstream server:

```text
https://<gateway-origin>/mcp/{server_key}
```

Deployments with Code Mode enabled also expose a code-driven endpoint:

```text
https://<gateway-origin>/code-mode-mcp
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

## Code Mode Endpoint

`/code-mode-mcp` is a separate gateway-owned MCP server that is available only when an admin has enabled Code Mode. When it is not enabled, the route returns not found.

Auth and session behavior are identical to `/mcp`: the same Oceans API-key headers, the same `MCP-Session-Id` issued during `initialize`, and the same principal binding. Sessions are not shared between `/mcp` and `/code-mode-mcp`; initialize each endpoint separately.

`tools/list` returns exactly two tools:

- `explore`: run JavaScript that searches and describes your granted tools, then returns a small projection. Tool calling is not available in explore.
- `execute`: run JavaScript that searches, describes, and calls your granted tools.

Both tools take one required argument:

```json
{
  "code": "const { items } = await oceans.searchTools({ query: \"github\" }); return items.length;"
}
```

`code` is the body of a JavaScript async arrow function. The gateway wraps it as `(async () => { ...code... })()`, so `return` produces the tool result and `await` is allowed at the top level.

### The `oceans` API

The sandbox exposes a single frozen `oceans` object. Every call is re-authorized by the gateway against your grants:

- `oceans.searchTools({ query?, limit?, offset?, server_key? })`: search your granted tools. An empty query lists everything granted. Returns `{ items, total, next_offset, ranker }`, where each item has an `address`, a `score`, and `server`/`tool` summaries, and `ranker` names the ranking strategy that produced the scores.
- `oceans.describeTool({ address })`: fetch the full persisted schema for one granted tool by its canonical `mcp://{server_key}/tools/{tool_name}` address. Returns the tool's `input_schema`, `schema_hash`, and `schema_version`.
- `oceans.callTool({ address, arguments?, schema_hash? })`: call one granted tool by canonical address. Available in `execute` only. Returns the upstream tool result.

A realistic `execute` body:

```javascript
const { items } = await oceans.searchTools({ query: "create issue", server_key: "github" });
if (items.length === 0) {
  throw new Error("no granted issue-creation tool found");
}
const detail = await oceans.describeTool({ address: items[0].address });
console.log("calling", detail.address);
const result = await oceans.callTool({
  address: detail.address,
  arguments: { title: "Bug report", body: "Details..." },
  schema_hash: detail.tool.schema_hash,
});
if (result.isError) {
  throw new Error(result.structuredContent.error_code + ": " + result.content[0].text);
}
return result;
```

`console.log`, `console.warn`, and `console.error` lines are captured and returned in the tool response alongside the result.

### How Explore Differs From `search_tools`

Aggregate `/mcp` `search_tools` returns raw search results to the client, and every follow-up describe or call is another MCP round trip through the model. `explore` runs the filtering and composition inside the sandbox: one `explore` call can search, describe several candidates, and return only the small projection the agent actually needs, keeping intermediate results out of the model's context window.

### Sandbox Semantics

- There is no event loop in the sandbox. Every `await oceans.*()` completes synchronously from the code's point of view; `Promise.all` works but runs calls one after another. Timers such as `setTimeout` do not exist.
- Grant/capability denials and invalid arguments throw ordinary catchable exceptions, so `try`/`catch` works as expected. Structured tool errors (`credential_required`, `credential_expired`, `tool_schema_changed`) and upstream failures do **not** throw: `oceans.callTool` resolves with the same aggregate-style `{content, isError: true, structuredContent: {error_code}}` result object `/mcp` clients receive — check `result.isError` before using a tool result.
- The sandbox has no network, filesystem, environment, or module imports. Tool access goes through `oceans.*` only.
- Executions are bounded by time, memory, output size, log volume, and a per-execution `oceans.*` call count. Oversized output is truncated and marked with `--- TRUNCATED ---`.

## Upstream Credentials

Oceans API keys are never forwarded upstream. For servers that need caller-specific credentials, platform admins create MCP credential bindings for a user, team, or service account. User-owned keys use a user binding first and then an allowed team binding. Service-account keys use a service-account binding first and then the owning-team binding. A service account never borrows a user's credential.

Missing upstream credentials return a structured `credential_required` tool error from aggregate `call_tool`, or a normal gateway error from direct `/mcp/{server_key}` proxying. Expired bindings return `credential_expired`.
