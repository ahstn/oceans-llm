# MCP Registry and Discovery

`See also`: [Observability and Request Logs](../observability-and-request-logs.md), [MCP Invocations](mcp-invocations.md), [Request Logs](request-logs.md), [Data Relationships](../../reference/data-relationships.md), [Admin API Contract Workflow](../../reference/admin-api-contract-workflow.md), [ADR: External MCP Registry and Discovery Boundary](../../adr/2026-05-26-external-mcp-registry-and-discovery.md)

The external MCP registry is the control-plane record of MCP servers that Oceans LLM may discover and later use for tool execution. Phase 2 stores user-added server records in the database, keeps recommended server suggestions in a checked-in static catalog, and discovers tool metadata through Streamable HTTP.

This page describes registry and discovery behavior. Tool grants, toolsets, chat/request execution, OAuth token exchange runtime, and stdio MCP servers are out of scope for this phase.

## Admin API

The platform-admin API surface is:

- `GET /api/v1/admin/mcp/recommended-servers`
- `GET /api/v1/admin/mcp/servers`
- `POST /api/v1/admin/mcp/servers`
- `PATCH /api/v1/admin/mcp/servers/{server_id}`
- `POST /api/v1/admin/mcp/servers/{server_id}/disable`
- `GET /api/v1/admin/mcp/servers/{server_id}/tools`
- `POST /api/v1/admin/mcp/servers/{server_id}/discovery-refresh`

All endpoints require an active platform-admin session. The admin contract is generated from gateway handler annotations; regenerate it with `mise run admin-contract-generate` after route or DTO changes.

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

Delete semantics are disable/archive semantics. There is no hard delete endpoint. Disabled servers are omitted from normal list views unless `include_disabled=true` is requested.

Rediscovery marks previously active tools inactive before upserting the newly discovered set. Existing tools keep their stable `mcp_tool_id` when the upstream tool name is unchanged. A changed input schema increments `schema_version`; unchanged schemas keep their current version.

## Discovery Transport

Phase 2 supports Streamable HTTP only.

Discovery initializes the configured server URL over Streamable HTTP, sends the MCP protocol version header, and accepts JSON or `text/event-stream` JSON-RPC responses. Tool input schemas are normalized into canonical JSON before hashing. Non-object input schemas are rejected and recorded as discovery failures.

Stdio MCP servers, legacy SSE transport, protocol proxying, tool federation, and execution-time routing are intentionally not implemented here.

## Auth Modes

Stored auth modes are declarations:

- `none`
- `gateway_static_header`
- `gateway_bearer_token`
- `user_passthrough`
- `oauth_obo`

Discovery can use only `none` or gateway-managed secret references. Gateway-managed discovery credentials require an HTTPS `server_url` and use `auth_config.secret_ref` with the `env/OCEANS_MCP_DISCOVERY_*` form. `gateway_static_header` also requires `auth_config.header_name`.

`user_passthrough` and `oauth_obo` are recorded so future execution and grants can require user-owned credentials. Discovery without a gateway-managed credential records `auth_required` rather than attempting to persist or forward a user token.

Never store raw tokens in:

- discovery runs
- tool metadata
- request logs
- MCP invocation logs
- admin API responses

Discovery diagnostics store bounded summaries and client error categories. HTTP failure summaries include the upstream status code, but not upstream response bodies.

## Relationship to Observability

MCP invocation logs already have nullable `server_id` and `tool_id` fields. Registry-backed execution can later populate those fields from `external_mcp_servers` and `external_mcp_tools` so request-log MCP cardinality can use stable registry identities.

Until execution is implemented, registry discovery only records server and tool metadata. It does not expose tools to model requests.

## Validation

Run:

```bash
mise run admin-contract-generate
mise run admin-contract-check
mise run lint
```

If docs tooling is available in the environment, also run the docs check before handoff.
