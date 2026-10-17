# ADR: Capability Scopes and MCP Server Lists for Gateway API Keys

- Date: 2026-10-17
- Status: Accepted
- Issue: [#446](https://github.com/ahstn/oceans-llm/issues/446), part of [#458](https://github.com/ahstn/oceans-llm/issues/458)

## Current state

- [Identity and Access: API key scopes](../access/identity-and-access.md#api-key-scopes)
- [MCP Client Setup: scoped API keys](../mcp/mcp-client-setup.md#scoped-api-keys)
- [ADR: OAuth Authorization for MCP Clients](2026-10-15-mcp-client-oauth-authorization.md), section 5, which named the shared scopes

## Context

A `gwk_` key carries everything its owner may do. A key made only to connect an MCP client can also call models and spend budget. A key made for a coding agent's model traffic can call every MCP tool its owner is granted. A leaked key exposes both. There is no way to issue a narrow key for one integration.

OAuth access tokens for MCP clients (#441) already carry scopes: `mcp:aggregate` for `/mcp` and `mcp:proxy` for `/mcp/{server_key}`. A token's resource narrows it to one server. API keys have no equivalent.

## Decision

### 1. Scope names

An API key carries either no scope list, meaning full access, or a non-empty list drawn from:

| Scope | Routes |
| --- | --- |
| `inference` | `/v1/messages`, `/v1/messages/count_tokens`, `/v1/chat/completions`, `/v1/responses`, `/v1/embeddings`, `/v1/decisions`, `/v1/systemone`, their unversioned aliases, `/v1/models`, `/models`, `/v1/model-metadata`, `/api/v1/batches*`, and `/api/v1/guardrails/evaluate` |
| `mcp:aggregate` | The aggregate endpoint `/mcp` |
| `mcp:proxy` | Per-server routes `/mcp/{server_key}` |

`mcp:aggregate` and `mcp:proxy` are the OAuth scope names, so a key and a token with the same scope reach the same routes.

`inference` covers model listing because clients list models before calling them. It covers batches because a batch runs inference and spends the same budget. It covers guardrail evaluation because harness hooks call it with the key they use for model traffic.

Every other route that accepts a key needs full access: the Skills API, `/api/v1/me/provider-credentials`, and the review agent action routes. A scoped key gets `insufficient_scope` there. New key-accepting routes need full access by default, so a route added without a scope never opens to scoped keys by accident.

`observability:read` is not accepted yet. No observability route accepts an API key today, so the scope would grant nothing. The name is reserved for the first key-readable observability route, such as SIEM export (#452).

Admin and session routes never accept a key, so scopes don't affect them.

### 2. MCP server list

A key may also carry a list of MCP server keys. With a list, the key reaches only those servers:

- `/mcp/{server_key}` for a server outside the list returns `insufficient_scope`. The check runs before the server is looked up, so a key can't tell an unlisted server from one that doesn't exist.
- On `/mcp`, `search_tools`, `describe_tool`, and `call_tool` see only tools on listed servers. A tool on another server reads as not granted, exactly like a tool the owner has no grant for.

Without a list, a key reaches every server its owner's grants reach. The list only narrows. A listed server still needs a grant before any tool is visible.

A server list needs an MCP scope, or full access. Saving one with `["inference"]` alone is rejected, because it would do nothing. Server keys must name registered servers when saved. Server keys never change and servers are disabled rather than deleted, so a saved list keeps meaning the same servers.

OAuth tokens don't use the list. A token's resource already binds it to one server or to all of them.

### 3. Backward compatibility

Migration `V65__api_key_scopes` adds two nullable columns to `api_keys` on both backends:

| Column | `NULL` means | Otherwise |
| --- | --- | --- |
| `scopes` | Full access | Space-delimited scope names, the OAuth `scope` format |
| `mcp_server_keys` | Every granted server | Space-delimited server keys |

Existing rows get `NULL` in both, so every existing key keeps full access until an admin edits it. The create API leaves both `NULL` when the request omits them. Saved values are normalized: scopes in catalogue order, server keys sorted, duplicates removed. That way, two saves of the same access compare equal.

The columns live on the key row, not in a join table, so authentication reads them in the query it already runs. A request costs no extra lookup.

### 4. Enforcement point

Scopes are checked once, on the authenticated credential, before the handler runs:

- `AuthenticatedApiKey::require_scope(scope)` answers "does this credential have scope X" for both credential types. For a key it reads the key's scopes. For an OAuth token it reads the token's scopes. The OAuth validator uses the same method, so both credential types fail the same way.
- `GatewayService::authenticate_for_scope` and `authenticate_bearer_token_for_scope` authenticate a key and require a scope. The inference extractor, model listing, batches, guardrail evaluation, and the MCP routes call them.
- `GatewayService::authenticate` and `authenticate_bearer_token`, used by every other key-accepting route, require full access.
- `AuthenticatedApiKey::allows_mcp_server(server_key)` checks the server list. The per-server MCP handler calls it right after authentication. On `/mcp`, `McpCatalog::authorized_catalog` calls it, the one place where search, describe, and call resolve tools. A filter on a server outside the list returns nothing without resolving grants. Without a filter, each listed server is resolved on its own through the existing one-server query, so tools on unlisted servers are never loaded. Grant resolution itself doesn't change.

Scopes are read from the database on every request, so an edit takes effect on the key's next request. An MCP event stream that is already open keeps running until the client reconnects.

### 5. Errors

All three refusals are `403` with code `insufficient_scope`:

- a missing scope (`AuthError::InsufficientScope`, which replaces `OauthInsufficientScope`)
- a route that needs full access (`AuthError::FullAccessRequired`)
- a server outside the key's list (`AuthError::McpServerNotInScope`)

On inference routes the body is the OpenAI-compatible error envelope. Anthropic routes use the Anthropic error shape, as they do for other errors. On MCP routes the body is a JSON-RPC error, code `-32001`, with `data.error = "insufficient_scope"` and, for a missing scope, `data.required_scope`. The check runs before the body is read, so the error's `id` is `null`. When OAuth for MCP clients is enabled, an MCP `403` also carries `WWW-Authenticate: Bearer error="insufficient_scope"` naming the scope, for both credential types.

### 6. Admin API and audit

- `POST /api/v1/admin/api-keys` accepts optional `scopes` and `mcp_server_keys`.
- `PUT /api/v1/admin/api-keys/{api_key_id}/scopes` replaces both. `null` restores full access or every server. It uses the `update_api_key` permission and the same ownership checks as the existing `PATCH`.
- Listings return `scopes` and `mcp_server_keys`, with `null` for full access, plus `available_scopes` and `mcp_servers`, the options for the create and edit forms.

The `PATCH` that edits model access doesn't change. A separate route means an older client that sends only model fields can't widen a scoped key by leaving scope fields out.

The store replaces scopes the way it replaces model access: it reads the row under the write lock, writes only when the value differs, and returns the value it replaced. The audit entry `api_key.scopes_updated` records `before` and `after` as `{scopes, mcp_server_keys}`. A save that changes nothing records nothing. `api_key.created` records the scopes in its `after` summary.

### 7. Rate limits

A scoped key keeps its ID. `api_key`, `user`, and `global` rate-limit policies (#25) apply to it exactly as before. A request refused for scope is rejected at authentication, before rate limiting, so it doesn't count against a limit.

## Trade-offs

- Scopes are coarse. `inference` covers batches and guardrail evaluation, so a key can't be limited to synchronous calls only. Finer scopes can split `inference` later without changing existing keys, because full access stays `NULL`.
- Skills, provider credentials, and review agent actions need full access. A key for one of those integrations can't be narrowed yet.
- Server lists name servers by key, not by ID. Keys are immutable and servers are never deleted, so the two are equivalent today. If server deletion is ever added, its cleanup must also remove the key from saved lists.
- An open MCP event stream outlives a scope edit until the client reconnects. OAuth streams are rechecked every 30 seconds. API key streams have never been rechecked, and this change doesn't add it.

## Follow-up

- `observability:read`, once an observability route accepts API keys (#452).
- Scopes for the Skills API and review agent actions, if narrow keys for those integrations are needed.
- Show scopes in invocation-log and request-log views (#448).
- Expiry and rotation (#449) should carry a key's scopes over to its replacement.
