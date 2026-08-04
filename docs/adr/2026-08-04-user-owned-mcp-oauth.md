# User-Owned OAuth for Upstream MCP Servers

## Status

Accepted

## Context

Oceans could store a principal-bound bearer token, but it could not start browser consent, handle a callback, store a refresh token, or refresh an expired access token. That made `oauth_obo` a manual token label rather than a complete OAuth flow.

Google Drive and Google Docs provide separate hosted MCP servers. Clients should reach both through the aggregate Oceans endpoint and authenticate only with an Oceans API key. Google authorization must stay separate from login to Oceans.

## Decision

Oceans is the confidential OAuth client for upstream MCP servers. Provider client credentials and callback origins are runtime configuration. Each server registry row owns its provider key, OAuth resource, required scopes, and discovery mode.

A signed-in user starts one connection per server. Oceans creates a one-use state transaction with PKCE, redirects the browser to the provider, validates the callback, and stores a versioned token bundle in the existing encrypted user credential binding. Tokens never enter the client harness or admin response.

Execution refreshes the token five minutes before expiry. A local lock and short database lease serialize refreshes per binding across gateway replicas. A rotated refresh token replaces the prior value. An `invalid_grant` response revokes the binding and requires user consent again.

Public discovery is allowed only when the server declares `discovery_auth: none`. Discovery does not borrow a user's token. Direct proxy responses do not forward upstream authentication challenges to the client.

## Consequences

The aggregate and direct MCP routes use the same refreshed credential. Users can connect and revoke their own accounts. Platform admins can see redacted provider, scope, status, and expiry metadata, but not token material.

An Oceans disconnect revokes only the selected local binding. It does not call Google's project-wide revocation endpoint because that action can also invalidate another Google MCP connection that uses the same Cloud project. Users can remove the full project grant in Google Account settings.

The first provider type is Google. Drive and Docs use separate resources and connections even when they use one OAuth client and callback route. Read-only Google scopes reduce access, but `drive.readonly` is restricted and can require Google verification for external production applications.

Legacy OAuth rows that store only a bearer token remain usable until expiry. They cannot refresh and should be replaced through the connection flow.

OAuth state rows expire after ten minutes. Each new authorization attempt removes expired rows before it stores the next state, so abandoned PKCE verifiers do not grow without a retention bound.

## Follow Ups

- Run a deployed Google Drive and Google Docs canary that covers consent, callback session binding, refresh, reconnect, disconnect, read-tool discovery, read calls, and audit attribution.
- Extract MCP OAuth configuration from `config.rs` when another provider type or a wider provider policy makes the current cohesive block harder to maintain.

## See Also

- [MCP upstream credential bindings and execution](./2026-06-09-mcp-upstream-credential-bindings-and-execution.md)
