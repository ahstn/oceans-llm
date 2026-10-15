# Workspace connections

Workspace connections lets any signed-in user link or unlink their own account on OAuth-backed MCP servers, such as Google Drive or Google Docs, that a platform administrator has registered. When no such server is configured, the page explains that an administrator must register one first.

## Sub-features

- `connections-list` shows each available OAuth MCP server with its status and `Requested scopes`.
- `connections-empty` shows `No OAuth servers are available` when there is no active oauth_obo server.
- `connections-connect` starts the provider consent flow with `Connect` or `Reconnect`.
- `connections-disconnect` removes the user's binding with `Disconnect`.
- `connections-callback` shows a toast for `?oauth=connected` or `?oauth_error=<code>`. The callback also returns `?server=<server_id>`, and the success toast then names the server, such as `Google Drive connected`.
- `connections-connect-link` handles `?connect=<server_key>`, the `connect_url` that MCP clients receive with `credential_required` and `credential_expired`. It drops the parameter, then starts consent for that server. A connected server shows `<name> is already connected`. A connection that no longer matches the server's provider, resource, or required scopes lists as `Expired`, so the link starts consent again; an unknown server shows `No workspace connection is available for MCP server <key>.`; an unavailable server shows its availability error.

- `connections-authorized-clients` shows the `Authorized MCP clients` card when OAuth sign-in for MCP clients is enabled. It lists each client the user allowed, with its access, last token time, and expiry, and `Disconnect <client>` revokes it. With no clients it reads `No MCP clients are authorized.`

## How to get to it (user POV)

- Open `/admin/account/connections` directly; it has no sidebar link. It is a personal page and needs no page permission, and the OAuth callback also returns here.

## Driving it with control-oceans-admin

Preconditions:

- `control-oceans-admin doctor` passes, and you are signed in as `admin@local`.
- The default `gateway.yaml` has no `mcp.oauth` block and no oauth_obo MCP server, so only the empty state can be reached.

- **Open.** Open `/admin/account/connections`. The URL is `/admin/account/connections` and the `Workspace connections` card is visible.
- **Empty state.** The `No OAuth servers are available` alert is visible, and no `Connect` button renders.
- **API parity.** From the browser context, fetch `/api/v1/mcp/oauth/connections`. It returns an empty list, which matches the empty state.
- **Non-admin reach.** In a new browser context, sign in as `ben@platform.local` with `localdemo123` and open `/admin/account/connections`. The same empty state renders, so the page is not gated by permissions.
- **Error callback.** Open `/admin/account/connections?oauth_error=access_denied`. An error toast appears and the page stays on the empty state.
- **Connect link, unknown server.** Open `/admin/account/connections?connect=google_drive`. The toast `No workspace connection is available for MCP server google_drive.` appears, the URL drops the `connect` parameter, and the browser does not leave the page.
- **Connect, connect link and disconnect (unreachable by default).** These need `mcp.oauth.public_base_url`, a Google entry under `mcp.oauth.providers`, a registered oauth_obo server, and interactive Google consent. Report them as skipped and name that unmet precondition.
- **Authorized clients, empty.** The `Authorized MCP clients` card reads `No MCP clients are authorized.`, which matches `/api/v1/me/authorized-mcp-clients` (`enabled: true`, empty `items`).
- **Authorized clients, disconnect.** Authorize a client through the [MCP client OAuth](./mcp-client-oauth.md) consent flow, then reload the page. The card lists the client by name. Choose `Disconnect <client>`. A `<client> disconnected` toast appears, the row disappears, and the client's access token now gets `401` on `/mcp`.
- **Proof.** Capture the card, the alert, and the API response.

## Gotchas

- `Connect` redirects to Google. Never complete consent with a real account during verification.
- The list includes only active oauth_obo servers and the user's existing bindings. A registered MCP server without OAuth does not appear.
- `Disconnect` acts only on the signed-in user's own binding. It is not an admin view of other users.
- The status badge capitalizes the status from the API, so compare statuses case-insensitively.
- Do not record the OAuth `state`, codes, or tokens in evidence.
- The `connect_url` itself is built by the gateway. `http::mcp_gateway::credentials::tests` and `aggregate::tests::missing_oauth_credential_returns_the_connect_url` prove the URL, the message, and that no secrets appear. `src/test/routes/account-connections-route.test.tsx` proves the page side with a stubbed consent redirect.
