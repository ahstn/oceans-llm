# MCP Gateway Auth Alignment Interview

`See also`: [MCP Registry and Discovery](../operations/observability/mcp-registry-and-discovery.md), [Identity and Access](../access/identity-and-access.md), [Admin Control Plane](../access/admin-control-plane.md)

Date: 2026-05-27

Related issue: [#116](https://github.com/ahstn/oceans-llm/issues/116)

## Scope

This interview aligned the authentication contract for future MCP gateway endpoints where external MCP clients authenticate to Oceans LLM using existing Oceans API keys. It also established the safety boundary between inbound Oceans credentials and upstream MCP server credentials, especially for future per-user OAuth forwarding.

References reviewed during triage:

- LiteLLM MCP gateway docs
- LiteLLM MCP OBO auth docs
- agentgateway MCP authentication and authorization docs
- Portkey MCP Gateway authentication docs, including OAuth, external OAuth, identity forwarding, and OAuth client metadata

## Core Direction

Use a two-layer MCP auth model:

1. **Inbound gateway auth:** the agent or MCP client authenticates to Oceans.
2. **Upstream server auth:** Oceans authenticates to the external MCP server.

These layers are independent. Inbound Oceans API keys are consumed by Oceans and must not be forwarded to upstream MCP servers by default.

## Questions and Alignment

### 1. Inbound API Key Header

Question: For inbound MCP gateway auth, should Oceans accept the existing `Authorization: Bearer <oceans-api-key>` header, or require a distinct header?

Recommended answer: Accept `Authorization: Bearer <oceans-api-key>` as the primary/default contract, and optionally also accept `x-oceans-api-key` as an escape hatch for clients that reserve `Authorization` for their own OAuth flow.

Decision: **Accept `Authorization` by default, plus optional `x-oceans-api-key` fallback.**

### 2. Upstream OAuth Storage Boundary

Question: When Oceans later supports forwarding OAuth to upstream MCP servers, what should the durable credential ownership boundary be?

Recommended answer: Store upstream OAuth grants as user-scoped records keyed by `(external_mcp_server_id, oceans_user_id, oauth_provider/account subject)`, with encrypted refresh/access token material in a secrets table or vault reference. Never attach user OAuth credentials to the shared server row or to a team/service-account API key row.

Decision: **User-scoped upstream OAuth grants only.**

### 3. Service-Account-Owned API Keys and User OAuth Grants

Question: Should service-account-owned Oceans API keys be allowed to use per-user upstream OAuth grants?

Recommended answer: No. A service account has no human user identity, so it must not borrow a user's Google Drive, Slack, or other upstream grant. For OAuth-backed MCP servers, service-account API keys should receive `auth_required` or unauthorized responses unless a future explicit non-human client-credentials/server credential mode is configured separately.

Decision: **No: service-account keys cannot use user OAuth grants.**

### 4. Dual Header Precedence

Question: If both `Authorization: Bearer ...` and `x-oceans-api-key` are present on an inbound MCP request, what should happen?

Recommended answer: Reject the request if both are present and not identical. If both are identical, accept. This avoids silent credential confusion and prevents a client or proxy from accidentally authenticating as a different Oceans principal than intended.

Decision: **Accept only if identical; reject if different.**

### 5. MCP Auth Error Shape

Question: For unauthorized inbound MCP requests, should Oceans return plain HTTP auth errors or MCP authorization metadata/errors?

Recommended answer: Use HTTP-first behavior for missing or invalid gateway credentials: `401` with `WWW-Authenticate` and a JSON error body before JSON-RPC processing. Once authenticated but not authorized for a specific tool/server, use MCP-level authorization behavior such as filtered `tools/list` results, `403`, or JSON-RPC method errors depending on endpoint semantics.

Decision: **HTTP-first `401/403` before JSON-RPC processing.**

## Implementation Requirements

### Inbound Gateway Auth

- MCP gateway endpoints should authenticate callers through the existing Oceans API-key authenticator and ownership semantics.
- Preferred header: `Authorization: Bearer <oceans-api-key>`.
- Compatibility header: `x-oceans-api-key: <oceans-api-key>`.
- If both headers are present with different token values, reject the request.
- If both are present with the same token value, accept.

### Upstream Credential Isolation

- Do not forward inbound `Authorization` or `x-oceans-api-key` to upstream MCP servers.
- Strip or ignore inbound gateway auth headers when constructing upstream MCP requests.
- Upstream auth headers/tokens must come only from:
  - the registered MCP server auth configuration, or
  - a resolved per-user upstream OAuth grant in a future OAuth phase.
- Any future identity forwarding must be explicit and non-secret, such as signed identity claims, not raw gateway API keys.

### Future User-Scoped OAuth Grants

For user-owned upstream OAuth scenarios, such as User A connecting a Google Drive MCP server:

- Store credentials only in records scoped to User A.
- User B, User C, and other users must not be able to reuse User A's grant or see User A's files.
- Tool listing and tool invocation must resolve upstream credentials for the authenticated Oceans user only.
- Service accounts must not resolve or borrow user OAuth grants.

Recommended future storage shape:

```text
external_mcp_oauth_grants
- grant_id
- mcp_server_id
- user_id
- upstream_subject
- scopes
- token_secret_ref or encrypted token columns
- expires_at
- created_at
- updated_at
- revoked_at
```

This table is directional guidance for the future OAuth issue and is not required for #116 unless the implementation scope expands.

## Acceptance Criteria Mapping

- Explicit decision on `Authorization` vs explicit header: use `Authorization` by default plus `x-oceans-api-key` fallback.
- Existing API-key semantics: both accepted inputs authenticate through the current Oceans API-key model.
- No upstream leakage: inbound credentials must never be forwarded to upstream MCP servers by default.
- Unauthorized errors: use HTTP-first `401/403` before JSON-RPC processing.
- Docs: setup examples should cover Claude Code, Codex, Cursor, and raw MCP SDK clients, showing `Authorization` as preferred and `x-oceans-api-key` as the compatibility fallback.
