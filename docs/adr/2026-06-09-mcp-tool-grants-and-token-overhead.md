# ADR: MCP Tool Grants and Token Overhead

## Status

Accepted.

## Context

External MCP registry and discovery give Oceans stable server and tool identities, but discovered tools should not automatically become callable. Tool definitions can also consume material context window, so operators need visibility into MCP overhead without mixing estimates into billing.

## Decision

Oceans stores named MCP toolsets, toolset membership, and explicit MCP grants. Grant subjects are API keys, users, teams, and service accounts. Grant targets are tools or toolsets.

Effective access is the union of active grants for the authenticated API key, owner user, owner service account, and active/owning team. Teams are access metadata only; runtime attribution remains user or service account. Disabled servers, inactive tools, disabled toolsets, revoked grants, and inactive memberships do not resolve as callable access.

The MCP gateway filters `tools/list` responses and rejects ungranted `tools/call` requests before upstream execution. Denials, allowed calls, upstream errors, timeouts, and invalid requests are recorded in MCP invocation logs.

MCP token-overhead estimates are cached in `mcp_tool_token_estimates` and summarized per request in `request_mcp_token_overheads`. Estimates are context-window telemetry, not billing truth, and are never written to `usage_cost_events`.

## Implementation

Database tables:

- `mcp_toolsets`
- `mcp_toolset_tools`
- `mcp_tool_grants`
- `mcp_tool_token_estimates`
- `request_mcp_token_overheads`

The estimate cache key includes provider family, model or encoding, server id, tool id/name, schema hash, description hash, MCP protocol version, and serializer version. Unsupported tokenizer families use a conservative low-confidence estimate and do not block requests.

## Trade-Offs

Filtering finite SSE `tools/list` responses requires buffering. The gateway fails closed for unsupported or oversized rewrite cases instead of leaking ungranted tools.

Revoked and stale grants remain auditable. They do not imply compatibility fallback or callable access.

## Follow-Ups

- Add richer admin UI controls for toolset and grant workflows.
- Add tokenizer-backed high-confidence estimates for known model families.
- Add user-scoped OAuth credentials before enabling `user_passthrough` and `oauth_obo` proxying.
