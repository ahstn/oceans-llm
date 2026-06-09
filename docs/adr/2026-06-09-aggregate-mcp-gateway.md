# ADR: Aggregate MCP Gateway Endpoint

Date: 2026-06-09

## Status

Accepted

## Decision

Oceans exposes two MCP data-plane shapes:

- `/mcp`: a gateway-owned aggregate MCP server for catalog discovery
- `/mcp/{server_key}`: a direct Streamable HTTP proxy to one registered upstream MCP server

The aggregate endpoint exposes only `search_tools` and `describe_tool` in this slice. It does not expose every upstream tool directly and does not execute upstream tools. Tool execution through aggregate addresses is a separate follow-up capability.

Aggregate sessions are durable transport state in `mcp_aggregate_sessions`. The gateway stores hashed signed session tokens, binds each session to the authenticated API key and owner metadata, and treats cross-principal reuse as not found.

The aggregate endpoint implements Streamable HTTP directly and intentionally does not add legacy HTTP+SSE fallback routes.

## Implementation

`crates/gateway-mcp` owns protocol-only server primitives: JSON-RPC request classification, initialize/tools responses, notification handling, and JSON-RPC error envelopes.

`crates/gateway` owns HTTP routing, Oceans API-key authentication, aggregate session issuance/validation, and JSON-RPC adaptation.

`crates/gateway-service` owns catalog behavior over effective MCP grants, including lexical search, canonical `mcp://{server_key}/tools/{tool_name}` addresses, and persisted schema description.

`crates/gateway-store` owns durable session storage and grant-filtered catalog access for LibSQL and Postgres.

## Rationale

The aggregate endpoint gives agents one stable MCP entry point without requiring users to toggle per-source MCP servers during a task. Keeping direct `/mcp/{server_key}` proxying preserves the existing simple path for clients that intentionally want one upstream server.

Returning only discovery tools from aggregate `tools/list` keeps the agent context small and avoids flooding clients with every granted upstream schema. Search and describe let agents pull the details they need on demand.

Durable sessions avoid process-local stickiness and support multi-replica gateways without requiring clients to reconnect whenever a request lands on a different instance.

No legacy HTTP+SSE fallback is added because this route is a new Streamable HTTP endpoint. Adding compatibility fallback would increase routing complexity and preserve an older transport pattern this project does not need for new gateway-owned functionality.

## Follow-Ups

- Add aggregate execution for canonical tool addresses in a separate issue.
- Add semantic ranking only after lexical ranking proves insufficient.
- Add invocation logging for aggregate execution when upstream calls are introduced.
