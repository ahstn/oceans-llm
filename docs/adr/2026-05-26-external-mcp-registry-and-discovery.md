# ADR: External MCP Registry and Discovery Boundary

## Status

Accepted

## Context

Oceans LLM needs a registry for external MCP servers before it can safely add grants, toolsets, execution, and request-level MCP cardinality based on stable server/tool identity.

The MCP protocol mechanics should not depend on Oceans-specific auth, budgets, request logging, admin DTOs, or store traits. At the same time, user-added MCP server records need durable persistence, discovery diagnostics, and soft-disable behavior across libsql and Postgres.

Recommended server entries are useful operator affordances, but they are not tenant data and must not silently become executable registry records.

## Decision

Create a protocol-only `gateway-mcp` crate for JSON-RPC envelopes, Streamable HTTP client behavior, protocol-version headers, `tools/list` normalization, canonical schema hashing, and typed MCP client errors.

Store user-added external MCP servers and discovered tools in the database:

- `external_mcp_servers`
- `external_mcp_tools`
- `external_mcp_discovery_runs`

Keep recommended servers in `crates/gateway-service/data/recommended_mcp_servers.json` as a read-only static catalog. Admins must explicitly create a registry row before discovery or future execution.

Put Oceans policy and orchestration in `gateway-service/src/mcp_registry.rs`. Put HTTP DTOs, platform-admin enforcement, and OpenAPI annotations in `gateway/src/http/mcp_registry.rs`.

## Implementation Notes

Phase 2 supports Streamable HTTP discovery only. Discovery calls `tools/list`, validates object input schemas, canonicalizes schemas, and records schema hashes.

Tool identity is stable by `(mcp_server_id, upstream_name)`. Rediscovery keeps the existing tool id for unchanged upstream names, increments `schema_version` when the schema hash changes, and marks missing tools inactive.

Auth modes are stored as declarations:

- `none`
- `gateway_static_header`
- `gateway_bearer_token`
- `user_passthrough`
- `oauth_obo`

Discovery uses only `none` or gateway-managed secret references. User passthrough and OAuth on-behalf-of modes record `auth_required` until execution/grant flows provide per-user credentials.

## Trade-offs

The static recommended catalog is less dynamic than a DB-seeded catalog, but it avoids hidden identity creation and keeps curated suggestions reviewable in code.

Streamable HTTP-only support excludes stdio and SSE servers in this phase, but it keeps the first registry implementation focused on a deployable remote-server path.

Soft disable/archive semantics preserve discovery history and future audit relationships at the cost of no hard delete endpoint.

## Follow-ups

- Add grant/toolset data models that reference stable registry ids.
- Populate MCP invocation `server_id` and `tool_id` during execution.
- Use registry identities for request-log MCP cardinality.
- Add OAuth token exchange runtime and per-user credential handling.
- Consider stdio/SSE support only behind an explicit transport boundary.
