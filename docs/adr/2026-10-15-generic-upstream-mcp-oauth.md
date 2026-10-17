# ADR: Generic Upstream MCP OAuth with Discovery and Dynamic Client Registration

- Date: 2026-10-15
- Status: Accepted
- Extends: [User-Owned OAuth for Upstream MCP Servers](./2026-08-04-user-owned-mcp-oauth.md)
- Updated: 2026-10-17. Discovery, registration, and token requests for server-level clients now go through the [SSRF-safe outbound fetcher](./2026-10-17-ssrf-safe-outbound-fetch.md), scoped to the address class of the admin-entered server.

## Current state

- [MCP Servers: OAuth for Any MCP Server](../configuration/mcp-servers.md#oauth-for-any-mcp-server)
- [MCP Servers: Gateway-Managed Upstream Credentials](../configuration/mcp-servers.md#gateway-managed-upstream-credentials)

## Context

Per-user upstream OAuth supported one provider type, `google`. Its client credentials lived in gateway configuration, and every server row had to name its scopes. Other OAuth-protected MCP servers could only use a shared static credential. That over-grants access, and adding each server took manual setup.

The MCP authorization spec describes how a client finds an upstream's authorization server and registers with it:

- protected-resource metadata (RFC 9728)
- authorization-server metadata (RFC 8414), or OpenID Connect discovery
- dynamic client registration (RFC 7591)
- Client ID Metadata Documents
- resource indicators (RFC 8707)

Static API keys could only use a fixed header or a bearer token. Upstreams that expect `Authorization: Token <key>` or `?api_key=<key>` could not be configured.

## Decision

### 1. Two sources for an OAuth client

An `oauth_obo` server row gets its OAuth client from one of two places:

- **A configured provider.** The server names it with `provider_key`. The provider type is `google`, which is unchanged, or the new `oauth2`. An `oauth2` provider has explicit `authorization_url` and `token_url`, a `client_id`, and an optional `client_secret` secret reference. It can also set default `scopes` and a token endpoint auth method. Its callback is `/api/v1/mcp/oauth/{key}/callback`, like Google's.
- **The server's own authorization server.** The row holds `issuer`, `authorization_endpoint`, `token_endpoint`, an optional `registration_endpoint`, and `client_registration`. Its token bundles use the reserved provider key `mcp_server`. All such servers share the callback `/api/v1/mcp/oauth/callback`, so one redirect URI works for every registration.

The second source needs no gateway configuration change. The admin registers the server in the UI.

### 2. Discovery is an admin-time read

`POST /api/v1/admin/mcp/oauth-discovery` reads the metadata for a server URL. The steps are:

1. Send an unauthenticated `initialize` request and read the `Bearer` challenge's `resource_metadata` and `scope`.
2. Read the protected-resource metadata, from the challenge's URL or from the RFC 9728 well-known paths.
3. Read the first authorization server's RFC 8414 metadata, falling back to OpenID Connect discovery.
4. Check that the issuer matches.

The endpoint returns what it found and a suggested auth config. It stores nothing. The admin applies the suggestion in the server form and may edit it before saving.

Default scopes come from the challenge's `scope`, then from the resource's `scopes_supported`. `resource_indicator` is set when the server publishes protected-resource metadata, because it then identifies itself as an RFC 8707 resource. A server without that metadata is treated as its own authorization server, which matches the earlier MCP spec, and discovery returns a warning.

Discovery does not run on the request path. Each connection uses the saved settings, so a change in upstream metadata cannot redirect tokens until an admin reviews and saves it.

### 3. Client registration

`client_registration` is one of:

- `dynamic`: Oceans registers on the first connection attempt. It stores the client in `mcp_oauth_clients`, one row per server. The client secret and registration access token are encrypted with the MCP credential key, like credential bindings. An insert-if-absent write lets concurrent first connections converge on one client. Oceans registers again only when the issuer or callback changes or the secret expires. A refresh never registers, because a new client could not use the old refresh token.
- `metadata_document`: the client ID is the URL of a Client ID Metadata Document that the gateway serves at `/api/v1/mcp/oauth/client-metadata.json`. The client is public and relies on PKCE.
- `manual`: an admin-supplied `client_id` and optional `client_secret_ref`.

Discovery recommends them in that order.

### 4. One flow and refresh path

All sources resolve to the same client value: client ID, optional secret, endpoints, token endpoint auth method, and redirect URI. The existing PKCE state, callback checks, refresh lease, local lock, and compare-and-swap code all use that value. The token endpoint auth method can be `client_secret_basic`, `client_secret_post`, or `none`. Google keeps `client_secret_post` and its offline-access parameters.

The token bundle stays at version 1 and gains two optional fields:

- `issuer`, so a bundle stops validating when a server switches authorization servers
- a nullable `refresh_token`, because some providers issue none

Without a refresh token, the binding lasts until the access token expires, and then the user is asked to reconnect. A token with neither a refresh token nor `expires_in` does not expire. A refreshable token without `expires_in` still assumes one hour.

The connect URL from [the connect URL ADR](./2026-10-13-mcp-credential-connect-url.md) now checks availability through the resolved client source. Servers with their own authorization server get a URL whenever `public_base_url` is set.

### 5. Static credential placement

`gateway_static_header` accepts either of these, plus `secret_ref`:

- `header_name`, with an optional `value_template` that contains `{{api_key}}` exactly once
- `query_param`

No new auth mode was added. A new mode would mean rebuilding the SQLite table to change its `CHECK` constraint. The admin UI labels the mode **Gateway static credential**.

The query value is never stored in `server_url`. The gateway adds it with `McpQueryCredential` when it builds each request. That type redacts the value from `Debug` output, and transport errors drop the URL, so the value stays out of logs, traces, and error messages.

## Consequences

- Admins can register an OAuth-protected MCP server by URL, and users connect from `/admin/account/connections` without an admin-created OAuth app.
- Upstreams without dynamic registration work through a metadata document, a per-server `manual` client, or an `oauth2` provider.
- `mcp_server` is a reserved provider key. `oauth2` provider keys may contain only letters, digits, `-`, and `_`, because they appear in callback paths.
- `scopes` is now optional on `oauth_obo` servers. An empty list sends no `scope` parameter, unless the provider has defaults, and requires no specific grant.
- The callback URI shared by server-level clients is part of every dynamic registration. Changing `mcp.oauth.public_base_url` causes each dynamic server to register again on its next connection. Existing connections then fail to refresh and need reconnecting.
- Oceans does not use the registration access token yet. It stores it encrypted so a later change can update or delete the registration.
- Discovery follows only the first listed authorization server and warns when there are more.

## Follow-ups

- Run a canary that registers a public reference MCP server through dynamic registration and through a metadata document, then connects, refreshes, and disconnects.
- Use the stored registration access token to delete the upstream client when an admin disables a server.
- Consider per-user static API keys sent as a template or query parameter. Today only gateway-managed static credentials support them.
