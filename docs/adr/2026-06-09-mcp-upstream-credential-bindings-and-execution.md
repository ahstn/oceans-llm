# MCP Upstream Credential Bindings and Aggregate Execution

## Status

Accepted

## Context

The aggregate `/mcp` endpoint originally exposed search and describe only. That made tool discovery safe, but agents still needed a separate per-server route for execution and there was no durable place to bind user, team, or service-account upstream credentials.

Oceans API keys are gateway credentials. They must not be forwarded to upstream MCP servers, and upstream credentials must not be stored in server registry JSON, discovery runs, invocation logs, or admin responses.

## Decision

Add `mcp_upstream_credential_bindings` as a separate persistence boundary for execution-time upstream credentials. Bindings are scoped to one MCP server and one owner scope:

- `user`
- `team`
- `service_account`

Supported first-slice material kinds are `static_header`, `bearer_token`, and OAuth-shaped bearer material with optional expiry. Storage is either an encrypted blob or an external `secret_ref`.

Aggregate `/mcp` now exposes exactly three built-in tools:

- `search_tools`
- `describe_tool`
- `call_tool`

`call_tool` accepts a canonical `mcp://{server_key}/tools/{encoded_name}` address, optional arguments, and optional `schema_hash`. It resolves the catalog address, re-checks effective grants, rejects schema hash mismatches before upstream execution, resolves the credential, calls upstream `tools/call`, and logs the invocation.

Direct `/mcp/{server_key}` keeps proxy behavior, but uses the same credential resolver. It still strips Oceans API keys and forwards only MCP runtime headers plus resolved upstream auth.

## Implementation Notes

Credential resolution order is:

- user API key: user binding, then team binding
- service-account API key: service-account binding, then owning-team binding

Service accounts never borrow user credentials. Grant checks happen before credential lookup so denied addresses do not reveal whether credentials exist.

Encrypted blobs require `OCEANS_MCP_CREDENTIAL_ENCRYPTION_KEY`, a base64-encoded 32-byte runtime key. `secret_ref` credential bindings use `env/OCEANS_MCP_CREDENTIAL_*`. Existing gateway discovery credentials continue to use `env/OCEANS_MCP_DISCOVERY_*`.

Missing credentials return a stable `credential_required` MCP tool error from aggregate execution. Expired bindings return `credential_expired`.

## Consequences

This keeps registry, grants, credentials, and invocation logging as separate concerns. Admin APIs can list and revoke bindings without ever returning raw secrets.

OAuth browser setup, token refresh, OpenAPI, GraphQL, custom sources, CLI work, and sandboxed code execution remain out of scope.

No legacy HTTP+SSE compatibility fallback is added. Streamable HTTP remains the only MCP transport for this gateway path.
