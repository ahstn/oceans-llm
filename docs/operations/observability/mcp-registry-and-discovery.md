# MCP Registry and Discovery

`See also`: [MCP Servers](../../configuration/mcp-servers.md), [MCP Tool Access](../../access/mcp-tool-access.md), [MCP Client Setup](../../setup/mcp-client-setup.md), [Observability and Request Logs](../observability-and-request-logs.md), [MCP Invocations](mcp-invocations.md), [Request Logs](request-logs.md), [Data Relationships](../../reference/data-relationships.md), [Admin API Contract Workflow](../../reference/admin-api-contract-workflow.md), [ADR: External MCP Registry and Discovery Boundary](../../adr/2026-05-26-external-mcp-registry-and-discovery.md), [ADR: MCP Tool Grants and Token Overhead](../../adr/2026-06-09-mcp-tool-grants-and-token-overhead.md)

The external MCP registry is the control-plane record of MCP servers that Oceans LLM can discover and expose through the MCP gateway. It stores user-added server records in the database, keeps recommended server suggestions in a checked-in static catalog, discovers tool metadata through Streamable HTTP, and powers the admin diagnostics page at `/admin/mcp/servers`.

This page is maintainer and admin documentation for registry diagnostics. User-facing server setup lives in [MCP Servers](../../configuration/mcp-servers.md), and client setup lives in [MCP Client Setup](../../setup/mcp-client-setup.md).

Tool grants and toolsets are now part of the MCP access layer. OAuth token exchange runtime and stdio MCP servers remain out of scope.

## Admin API

The platform-admin API surface is:

- `GET /api/v1/admin/mcp/recommended-servers`
- `GET /api/v1/admin/mcp/servers`
- `POST /api/v1/admin/mcp/servers`
- `PATCH /api/v1/admin/mcp/servers/{server_id}`
- `POST /api/v1/admin/mcp/servers/{server_id}/disable`
- `GET /api/v1/admin/mcp/servers/{server_id}/tools`
- `POST /api/v1/admin/mcp/servers/{server_id}/discovery-refresh`
- `GET /api/v1/admin/mcp/toolsets`
- `POST /api/v1/admin/mcp/toolsets`
- `PATCH /api/v1/admin/mcp/toolsets/{toolset_id}`
- `POST /api/v1/admin/mcp/toolsets/{toolset_id}/disable`
- `PUT /api/v1/admin/mcp/toolsets/{toolset_id}/tools`
- `GET /api/v1/admin/mcp/grants`
- `PUT /api/v1/admin/mcp/grants`
- `DELETE /api/v1/admin/mcp/grants`
- `GET /api/v1/admin/mcp/credential-bindings`
- `PUT /api/v1/admin/mcp/credential-bindings`
- `DELETE /api/v1/admin/mcp/credential-bindings/{credential_binding_id}`
- `GET /api/v1/admin/mcp/effective-access`

All endpoints require an active platform-admin session. The admin contract is generated from gateway handler annotations; regenerate it with `mise run admin-contract-generate` after route or DTO changes.

## Admin UI

The admin UI route is:

```text
/admin/mcp/servers
```

The page shows:

- registered servers, active/disabled state, discovery status, tool count, and bounded last error
- selected server diagnostics, URL, auth mode, timeout, and discovery timestamps
- discovered tools with active state, schema hash, schema version, and first/last discovered timestamps
- recommended catalog import
- custom server creation
- edit, disable, and discovery refresh actions
- redacted upstream credential bindings for user, team, and service-account execution

Keep this page separate from `/admin/observability/mcp-invocations`. The registry page describes configured upstream servers and discovery status; the invocation page describes request-linked tool calls.

## Recommended Catalog

Recommended servers live in `crates/gateway-service/data/recommended_mcp_servers.json`.

The catalog is read-only runtime data:

- it is never auto-seeded into the database
- it is never treated as DB identity
- admins must explicitly register/import a catalog entry before discovery
- catalog keys are suggestions, not durable registry ids

When an admin creates a server with `recommended_catalog_key`, catalog values provide defaults for omitted fields. Overrides in the request body win. The resulting database row gets its own stable `mcp_server_id`.

## Database Registry

User-added MCP servers are stored in:

- `external_mcp_servers`: durable server identity, display data, auth declaration, discovery summary, and soft-disable state
- `external_mcp_tools`: latest known tools, normalized schemas, stable tool ids, schema hashes, schema versions, and active/inactive state
- `external_mcp_discovery_runs`: immutable discovery attempt diagnostics
- `mcp_toolsets`: named access bundles
- `mcp_toolset_tools`: stable tool membership for each bundle
- `mcp_tool_grants`: active and revoked grants from API keys, users, teams, and service accounts to tools or toolsets
- `mcp_upstream_credential_bindings`: principal-bound upstream credentials for execution-time auth
- `mcp_tool_token_estimates`: cached context-token estimates for MCP tool definitions
- `request_mcp_token_overheads`: request-level MCP context-overhead summaries, separate from spend accounting

Delete semantics are disable/archive semantics. There is no hard delete endpoint. Disabled servers are omitted from normal list views unless `include_disabled=true` is requested.

Rediscovery marks previously active tools inactive before upserting the newly discovered set. Existing tools keep their stable `mcp_tool_id` when the upstream tool name is unchanged. A changed input schema increments `schema_version`; unchanged schemas keep their current version.

## Gateway Data Plane

The public data-plane routes are:

```text
POST /mcp
GET /mcp/{server_key}
POST /mcp/{server_key}
DELETE /mcp/{server_key}
POST /code-mode-mcp
DELETE /code-mode-mcp
```

`/mcp` is a gateway-owned aggregate MCP server. It handles Streamable HTTP `POST` messages, returns `405` for `GET`, issues durable `MCP-Session-Id` values during initialize, and exposes only `search_tools`, `describe_tool`, and `call_tool` over granted active catalog rows.

`/code-mode-mcp` is the gateway-owned Code Mode MCP server. It shares the aggregate auth and durable session machinery, exposes only `explore` and `execute`, returns `405` for `GET`, and returns `404` when `mcp.code_mode.enabled` is false or absent.

Registry validation rejects the reserved server keys `mcp` and `code-mode-mcp` on create (`RESERVED_MCP_SERVER_KEYS` in `crates/gateway-service/src/mcp_registry.rs`); the server key is immutable on update, so an upstream registration can never collide with gateway-owned routes.

`/mcp/{server_key}` proxies Streamable HTTP requests to the active registered server URL. Runtime policy:

- authenticate inbound callers with Oceans API keys
- hide disabled and unknown servers as not found
- allow active servers with gateway-managed auth or principal-bound upstream credential bindings
- filter `tools/list` to the active tools granted to the caller
- reject ungranted `tools/call` requests before contacting upstream
- return `credential_required` or `credential_expired` when an execution-time binding is missing or expired
- strip inbound `Authorization` and `x-oceans-api-key` before proxying upstream
- forward only protocol/runtime-safe MCP headers and resolved upstream auth

Aggregate sessions are stored in `mcp_aggregate_sessions` as hashed signed tokens bound to the authenticated API key and owner metadata. Session ids are not portable across principals; cross-principal reuse is returned as not found.

Inbound credential contract details live in [Identity and Access](../../access/identity-and-access.md).

## Discovery Transport

Phase 2 supports Streamable HTTP only.

Discovery initializes the configured server URL over Streamable HTTP, sends the MCP protocol version header, and accepts JSON or `text/event-stream` JSON-RPC responses. Tool input schemas are normalized into canonical JSON before hashing. Non-object input schemas are rejected and recorded as discovery failures.

Discovery status is the server health signal for this slice. Do not add a separate ping health check or discovery-run history UI until the product needs those distinct diagnostics.

Stdio MCP servers, legacy HTTP+SSE transport, and tool federation are intentionally not implemented here.

## Auth Modes

Stored auth modes are declarations:

- `none`
- `gateway_static_header`
- `gateway_bearer_token`
- `user_passthrough`
- `oauth_obo`

Discovery can use only `none` or gateway-managed secret references. Gateway-managed credentials require an HTTPS `server_url` and use `auth_config.secret_ref` with the `env/OCEANS_MCP_DISCOVERY_*` form. `gateway_static_header` also requires `auth_config.header_name`.

Execution for `user_passthrough` and `oauth_obo` resolves `mcp_upstream_credential_bindings` after the tool grant check. User API keys may use a user binding and then a team binding. Service-account API keys may use a service-account binding and then the owning-team binding. Service accounts never borrow user credentials.

Encrypted credential blobs require `OCEANS_MCP_CREDENTIAL_ENCRYPTION_KEY`, a base64-encoded 32-byte key. Credential `secret_ref` entries must use `env/OCEANS_MCP_CREDENTIAL_*`. OAuth browser setup and token refresh are intentionally not implemented in this slice; `oauth_tokens` stores bearer-shaped material with optional expiry.

Never store raw tokens in:

- discovery runs
- tool metadata
- request logs
- MCP invocation logs
- admin API responses

Discovery diagnostics store bounded summaries and client error categories. HTTP failure summaries include the upstream status code, but not upstream response bodies.

## Metrics and Traces

Discovery refresh emits metrics:

- `gateway.mcp.discovery.refreshes`
- `gateway.mcp.discovery.refresh.duration`

Metric labels are bounded to `server_id`, `result`, and `status`. Do not add labels for URLs, header values, secrets, or raw upstream errors.

Discovery refresh and MCP proxy attempts run under tracing spans with redacted fields. Safe fields include server id, server key, upstream auth mode, caller owner kind, and status code.

## Failure Remediation

Use the registry page first:

- `auth_required`: discovery could not use gateway/shared credentials; configure a gateway-managed discovery credential or use execution-time credential bindings for calls.
- `credential_required`: add a user, service-account, or team credential binding for the server.
- `credential_expired`: rotate the binding or update its expiry.
- `failed`: inspect `last_error_summary`, validate URL reachability, timeout, protocol support, and secret environment variables.
- disabled server: re-create or update the desired server record; disabled servers are hidden from data-plane clients.
- zero tools after success: confirm the upstream server exposes tools over Streamable HTTP and returns object input schemas.

## Relationship to Observability

MCP invocation logs populate stable `server_id` and `tool_id` when the gateway handles `tools/call`. Policy-denied calls may not have a tool id if the requested upstream name is unknown or inactive.

Request-log MCP cardinality remains request-scoped. MCP token-overhead summaries are stored separately from usage cost events and are not billing inputs.

## Validation

Run:

```bash
mise run admin-contract-generate
mise run admin-contract-check
mise run lint
```

If docs tooling is available in the environment, also run the docs check before handoff.
