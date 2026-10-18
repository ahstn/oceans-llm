# ADR: Return a Connect URL for Missing MCP Credentials

- Date: 2026-10-13
- Status: Accepted

## Current state

- [MCP Client Setup: Connecting from an error](../mcp/mcp-client-setup.md#connecting-from-an-error)

## Context

A user-owned key calling an `oauth_obo` server without a stored connection got `credential_required`. A key whose connection had expired got `credential_expired`. Neither error said how to fix the problem.

The fix is to open `/admin/account/connections` and press `Connect`. Nothing in the error pointed there, so users had to know the page existed. On the direct route, `tools/call` returned an HTTP 403 envelope. Many clients show that envelope as a transport failure rather than a message the model or the user can act on.

## Decision

### 1. One link format, built by the gateway

`connect_url` is `{mcp.oauth.public_base_url}/admin/account/connections?connect={server_key}`. It is returned only when the caller can fix the problem in a browser:

- the server is active and uses `oauth_obo`
- its OAuth provider is configured
- `public_base_url` is set
- the API key is user-owned

Service-account keys never get a URL, because they cannot use a person's connection. `user_passthrough` servers never get one either, because only an admin can create their bindings. In both cases the message says to contact an Oceans administrator.

The link holds only the public origin and the server key. It does not start OAuth on its own. The connections page reads `connect`, removes it from the URL, and calls the existing session-authenticated start endpoint. That endpoint creates the PKCE state and redirects to the provider.

The page skips consent for a server that already lists as `connected`. A stored connection granted for a different provider or resource, or missing a scope the server now requires, is refused by the gateway with `credential_required`. The connections list therefore reports it as `expired`, so the link starts consent again instead of saying the server is already connected.

A crafted link can therefore do no more than open the provider's consent screen for the signed-in user's own account.

### 2. Name the connected server on return

The page now sets the OAuth start `redirect_to` to `/admin/account/connections?server={server_id}`. The callback appends `oauth=connected`, so the success toast can name the server that was connected.

### 3. Tool calls fail as tool results

| Route and method | Response |
| --- | --- |
| Aggregate `call_tool` | Unchanged shape: a tool result with `isError: true`. The text adds the next step, and `connect_url` is added to `structuredContent`. |
| Direct `tools/call` | Changed from HTTP 403 to HTTP 200 with a JSON-RPC result with `isError: true`. `structuredContent` has `server_key`, `error_code`, and `connect_url`. |
| Direct `initialize`, `tools/list`, and governed or lifecycle methods | Unchanged HTTP 403 envelope, with the next step in `message` and an added `connect_url` member. |

Other upstream preparation errors keep their existing responses. The invocation log still records `tools/call` credential failures as `unauthorized` with the original error code.

## Trade-offs

- A direct `tools/call` credential failure is no longer an HTTP error. Callers that keyed off the 403 must read `isError` and `structuredContent.error_code` instead. The aggregate route already worked this way.
- The direct route needs a credential before `initialize` succeeds. A user without a connection sees the 403 while connecting, and some clients may hide its message.
- The URL is absolute only when `public_base_url` is set. Without it, users get the contact-an-administrator message even though the page exists.

## Follow-up

- Consider letting `initialize` succeed without an upstream credential on the direct route, so the first visible failure is a tool result.
