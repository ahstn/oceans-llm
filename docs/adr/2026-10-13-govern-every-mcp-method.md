# ADR: Govern Every MCP Method on the Direct Proxy

- Date: 2026-10-13
- Status: Accepted

## Current state

- [MCP Tool Access](../mcp/mcp-tool-access.md#other-methods-on-direct-mcpserver_key)

## Context

The direct proxy at `/mcp/{server_key}` parsed each request into `tools/list`, `tools/call`, or `Other`. Every other JSON-RPC method fell into `Other`, and so did requests with an empty body. The `Other` path resolved the upstream credential and forwarded the request. It did not check grants, run guardrails, or write an invocation record.

As a result, any valid API key could call `resources/read`, `prompts/get`, or `completion/complete` on any active server, including servers on which it had no tool grants. Results bypassed result guardrails, and nothing was logged. For servers that use a shared or service-account credential, the caller read data with someone else's upstream access.

## Decision

### 1. Classify every method and deny unknown ones

`parse_mcp_rpc_request` now returns one of these variants:

- `ToolsList` or `ToolsCall`, which keep their existing per-tool handling.
- `Initialize`, which is forwarded without a grant. The gateway removes `tasks` from the result's `capabilities`, because it does not proxy MCP tasks (see below).
- `Lifecycle`, which covers `ping`, `logging/setLevel`, the client notifications `notifications/initialized`, `notifications/cancelled`, `notifications/progress`, and `notifications/roots/list_changed` sent without an `id`, client replies to server requests (`jsonrpc: "2.0"`, an `id`, and exactly one of `result` or `error`), and `DELETE` session termination. These are forwarded.
- `EventStream`, which is a `GET` that opens the server-to-client stream.
- `Governed`, which covers the five resources methods, `prompts/list`, `prompts/get`, and `completion/complete`.
- `Unknown`, which is any other method, including any other `notifications/*` name and a client notification sent with an `id`.

`Unknown` methods get HTTP 403 with JSON-RPC code `-32601`, and the gateway logs them with error code `mcp_method_not_allowed`. A request echoes its `id`. A notification gets the same status and an error body without an `id`, as the transport allows for a message the server does not accept. Logged method names are truncated to 256 characters. A `POST` with an empty body gets HTTP 400 instead of being forwarded, and so does a `GET` or `DELETE` with a body. Only a `POST` carries a JSON-RPC message, and the upstream treats a `GET` as its event stream whatever the body says, so a body must not decide which checks apply.

The protocol will add methods over time. Deny-by-default means a new method needs a deliberate decision before callers can use it through the gateway.

MCP tasks are the first such method family. A `tools/call` with a `task` param returns a task ID, and the tool's result arrives later through `tasks/result`, which has no tool name to check grants or result guardrails against. The gateway therefore does not proxy tasks: `tasks/*` stays `Unknown`, `initialize` results lose the `tasks` capability, `tools/list` results lose each tool's `execution.taskSupport`, and a `tools/call` that still carries `task` gets HTTP 400. Without the capability and the tool-level hint, a client calls tools directly. A tool that requires tasks fails with the upstream server's error instead of starting a task the client could never finish. Upstream discovery and aggregate calls request protocol version 2025-06-18, which has no tasks.

### 2. Resources, prompts, and completions need a server grant

Grants are per tool, and resources and prompts have no grant model of their own. A governed method is forwarded only when the caller holds a grant on at least one active tool on the server, directly or through an active toolset. This uses the same grant rules as `tools/list` filtering, but `has_mcp_server_grant` runs it as a single `EXISTS` query per grant subject rather than loading the full tool list on every resource read. Without a grant, the gateway returns HTTP 403 with JSON-RPC code `-32001` and logs a `policy_denied` record with error code `mcp_server_not_granted`.

The `GET` event stream can carry resource update notifications, so it needs the same grant. It is long-lived, so it is not logged per call.

### 3. Read results go through result guardrails

`resources/read` and `prompts/get` return content that reaches the model, so their responses go through `enforce_direct_mcp_result`, which handles both JSON and finite `text/event-stream` bodies. List and completion results are not result-guarded, which matches `tools/list`. Request guardrails are not applied to these methods.

### 4. The invocation log records the method

Migration `V57__mcp_invocation_method` (libSQL and Postgres) adds `mcp_method` to `mcp_tool_invocations`, with a default of `tools/call`. It backfills `tools/list` and `invalid` rows from their existing markers, and adds a `(mcp_method, occurred_at)` index. For governed calls, the display key is the resource URI or prompt name when there is one, and the method name otherwise. `tool_id` is empty. Request params are stored as the arguments payload, subject to the existing payload retention settings.

The admin API exposes `mcp_method` on invocation summaries and accepts it as a list filter. The MCP Invocations page adds a `Method` column, filter, and detail row.

### 5. Sessions belong to their caller, and streams cannot resume

_Added 2026-10-18 after a review of the MCP gateway epic._

Streamable HTTP lets a client resume an interrupted SSE stream by sending `GET` with `Last-Event-ID`. The upstream then replays the rest of the stream, which may be the response to an earlier `POST`. The gateway forwarded that header and kept upstream `id:` lines, and a `GET` only needed a grant on any one tool. A replayed `tools/call` or `tools/list` response therefore reached the client without per-tool grant filtering, approved-definition substitution, result guardrails, or an invocation record.

The gateway now refuses resumption instead of governing replays:

- `Last-Event-ID` is no longer forwarded upstream.
- Every upstream SSE body, streamed or buffered, loses its `id` field, so clients have no event id to resume from. An event that held only an `id` is dropped.
- On the standalone `GET` stream, the gateway drops JSON-RPC responses (messages with `result` or `error` and no `method`). The transport allows only server requests and notifications there, so a compliant upstream loses nothing.

Governing replays would mean tracking which request each upstream event id belongs to, across replicas, and re-running the original request's grant check and guardrails on delivery. Resumption is optional in the transport, and a client that loses a stream can repeat the request.

Upstream `Mcp-Session-Id` values were also passed through unchanged. On a server with a shared credential, any caller who learned a session id could open its event stream or `DELETE` it; `DELETE` needs no grant. The gateway now issues its own session id: the upstream id in base64url, a `.`, and an HMAC-SHA256 tag (128 bits, keyed with the identity token secret, or with a random per-process key when that secret is unset or the public development fallback) over the server id, the API key id or OAuth grant id, and the owner. Inbound session ids are checked against the current caller before anything is forwarded. A missing tag, a tag for another caller or server, or the raw upstream id gets HTTP 404 with JSON-RPC code `-32001`, which MCP clients treat as an expired session. The signed format needs no session table, so it works across replicas that share a configured identity token secret.

## Trade-offs

- A grant on one tool gives access to all resources and prompts on that server. Finer control would need a resource and prompt grant model, which is a larger change.
- Clients that used another method through the proxy will now get 403 until the method is classified.
- A client whose stream drops mid-response cannot resume it through the gateway and must repeat the request. For `tools/call`, that runs the tool again.
- Rotating the identity token secret ends every proxied session; clients re-initialize after the `404`. When the secret is unset or the public development fallback, each process has its own key, so a session id only works on the replica that issued it.
- An `initialize` call still needs a working upstream credential, but it does not need a grant. A caller with no grants can learn that a server exists and see its declared capabilities. Server keys are already visible to anyone who has the client configuration.

## Follow-up

- Govern `tasks/*` if upstream servers start to require tasks. That needs the gateway to record which tool and grant started each task, so `tasks/result` can be checked and guarded like `tools/call`.

- Consider resource and prompt grants if teams need to expose some resources without granting tools.
- Consider request guardrails for `prompts/get` arguments and `completion/complete` input.
