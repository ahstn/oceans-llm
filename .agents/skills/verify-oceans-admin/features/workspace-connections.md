# Workspace connections

Workspace connections lets any signed-in user link or unlink their own account on OAuth-backed MCP servers, such as Google Drive or Google Docs, that a platform administrator has registered. When no such server is configured, the page explains that an administrator must register one first.

## Sub-features

- `connections-list` shows each available OAuth MCP server with its status and `Requested scopes`.
- `connections-empty` shows `No OAuth servers are available` when there is no active oauth_obo server.
- `connections-connect` starts the provider consent flow with `Connect` or `Reconnect`.
- `connections-disconnect` removes the user's binding with `Disconnect`.
- `connections-callback` shows a toast for `?oauth=connected` or `?oauth_error=<code>`.

## How to get to it (user POV)

- Under `Control Plane`, choose `Connections`. It is a personal page and needs no page permission.
- Open `/admin/account/connections` directly. The OAuth callback also returns here.

## Driving it with control-oceans-admin

Preconditions:

- `control-oceans-admin doctor` passes, and you are signed in as `admin@local`.
- The default `gateway.yaml` has no `mcp.oauth` block and no oauth_obo MCP server, so only the empty state can be reached.

- **Open.** Choose `Connections`. The URL is `/admin/account/connections` and the `Workspace connections` card is visible.
- **Empty state.** The `No OAuth servers are available` alert is visible, and no `Connect` button renders.
- **API parity.** From the browser context, fetch `/api/v1/mcp/oauth/connections`. It returns an empty list, which matches the empty state.
- **Non-admin reach.** In a new browser context, sign in as `ben@platform.local` with `localdemo123` and choose `Connections`. The same empty state renders, so the page is not gated by permissions.
- **Error callback.** Open `/admin/account/connections?oauth_error=access_denied`. An error toast appears and the page stays on the empty state.
- **Connect and disconnect (unreachable by default).** These need `mcp.oauth.public_base_url`, a Google entry under `mcp.oauth.providers`, a registered oauth_obo server, and interactive Google consent. Report them as skipped and name that unmet precondition.
- **Proof.** Capture the card, the alert, and the API response.

## Gotchas

- `Connect` redirects to Google. Never complete consent with a real account during verification.
- The list includes only active oauth_obo servers and the user's existing bindings. A registered MCP server without OAuth does not appear.
- `Disconnect` acts only on the signed-in user's own binding. It is not an admin view of other users.
- The status badge capitalizes the status from the API, so compare statuses case-insensitively.
- Do not record the OAuth `state`, codes, or tokens in evidence.
