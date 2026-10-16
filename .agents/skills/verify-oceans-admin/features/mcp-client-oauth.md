# MCP client OAuth

MCP clients without a `gwk_` key can sign in through the gateway. An unauthenticated call to `/mcp` or `/mcp/{server_key}` gets a `401` whose `WWW-Authenticate` header points at the protected-resource metadata. The client then discovers the authorization server, registers, and sends the user to the `Allow <client> to use Oceans MCP?` consent page in the admin UI. After consent, the gateway issues an `oat_` access token and an `ort_` refresh token bound to that MCP resource. The tokens act as the signed-in user, are refused on non-MCP routes, and stop working when the user signs out or is deactivated.

## Sub-features

- `oauth-challenge` returns `401` with `WWW-Authenticate: Bearer resource_metadata="…", scope="mcp:aggregate"` on `/mcp`, and the per-server metadata URL with `scope="mcp:proxy"` on `/mcp/{server_key}`.
- `oauth-discovery` serves `/.well-known/oauth-protected-resource/mcp`, `/.well-known/oauth-protected-resource/mcp/{server_key}`, and `/.well-known/oauth-authorization-server`.
- `oauth-registration` accepts RFC 7591 registration at `/oauth/register` and returns a `client_id`.
- `oauth-consent` shows `/admin/oauth/consent?request_id=<id>` after sign-in, with the client name, the `Returns to` host, the `Access`, and `Allow` and `Deny`.
- `oauth-token` exchanges the code with PKCE at `/oauth/token` and returns `oat_` and `ort_` tokens.
- `oauth-mcp-call` accepts the `oat_` token on `/mcp` and records the invocation with auth method `oauth`.
- `oauth-refresh` rotates the refresh token; a reused refresh token revokes the grant.
- `oauth-revocation` ends access through `/oauth/revoke`, through `Disconnect` in the `Authorized MCP clients` card, and through sign-out of the approving session.
- `oauth-route-boundary` rejects `oat_` tokens on non-MCP routes such as `/v1/models`.

## How to get to it (user POV)

- An MCP client opens `/oauth/authorize?…` in the browser. The gateway redirects to `/admin/oauth/consent?request_id=<id>`, which sends a signed-out user to `/admin/login` first and back afterwards.
- The consent page has no sidebar link and needs no page permission. Any active signed-in user can open it.

## Driving it with control-oceans-admin

Preconditions:

- `control-oceans-admin doctor` passes, and you are signed in as `admin@local`.
- `gateway.yaml` enables `mcp.client_oauth`. Its issuer comes from `OCEANS_LOCAL_PUBLIC_BASE_URL`, which the `gateway-local` mise task template sets to `http://127.0.0.1:$GATEWAY_PORT`. Check that `/.well-known/oauth-authorization-server` reports the run's gateway origin as `issuer`, then run the protocol steps with `curl` or `fetch` against that origin.
- Generate a PKCE pair for the run: a 43-character verifier and its S256 challenge. Do not reuse a pair between runs.

- **Challenge.** `POST /mcp` with a `tools/list` JSON-RPC body and no `Authorization`. Expect `401` and `WWW-Authenticate` naming `/.well-known/oauth-protected-resource/mcp` and `scope="mcp:aggregate"`.
- **Discovery.** `GET` the metadata URL from the header. `resource` is `<issuer>/mcp` and `authorization_servers` is `[<issuer>]`. `GET /.well-known/oauth-authorization-server`. `issuer` matches, `code_challenge_methods_supported` is `["S256"]`, and `registration_endpoint` is present.
- **Registration.** `POST /oauth/register` with `{"client_name":"Verify client","redirect_uris":["http://127.0.0.1:9/callback"],"grant_types":["authorization_code","refresh_token"],"token_endpoint_auth_method":"none"}`. Expect `201` with a `client_id`.
- **Grant a tool (for the MCP call).** Through the admin API, register a temporary `auth_mode: none` MCP server that points at a local stub upstream, refresh its discovery, and grant one tool to the signed-in user. Disable the server and revoke the grant during cleanup.
- **Consent.** In the browser, open `/oauth/authorize?response_type=code&client_id=<id>&redirect_uri=http://127.0.0.1:9/callback&state=verify&code_challenge=<challenge>&code_challenge_method=S256&resource=<issuer>/mcp`. The URL becomes `/admin/oauth/consent?request_id=<id>`. The heading `Allow Verify client to use Oceans MCP?`, the `Returns to` host `127.0.0.1:9` with a `This device` badge, `All MCP servers you have access to`, and the loopback warning are visible. Capture an ARIA snapshot and a screenshot.
- **Allow.** Press `Allow`. The browser navigates to `http://127.0.0.1:9/callback?code=…&state=verify&iss=<issuer>`. Nothing listens there, so read the code from the failed navigation URL.
- **Token.** `POST /oauth/token` as a form with `grant_type=authorization_code`, the `client_id`, the code, the `redirect_uri`, the `code_verifier`, and `resource=<issuer>/mcp`. Expect `200`, `Cache-Control: no-store`, an `oat_` access token, an `ort_` refresh token, and `scope` `mcp:aggregate`. Redeeming the same code again returns `invalid_grant`.
- **MCP call.** `POST /mcp` with `Authorization: Bearer <oat_…>` and an `initialize` body. Expect `200` and an `mcp-session-id`. Send `notifications/initialized`, then `tools/list` with that session, and expect the three aggregate tools. Authorize again with `resource=<issuer>/mcp/<server_key>` for the temporary server and call its tool with `tools/call`. Expect `200`, and the invocation detail metadata shows `auth_method` `oauth` and the `oauth_client_id`. The same per-server token on `/mcp` returns `401` `invalid_token`.
- **API keys still work.** Create a temporary user API key for `alice@platform.local` (the bootstrap admin cannot own keys), send `initialize` to `/mcp` with it, expect `200`, then revoke the key.
- **Route boundary.** `GET /v1/models` with the same token. Expect `401`.
- **Refresh.** `POST /oauth/token` with `grant_type=refresh_token`, the `client_id`, and the refresh token. Expect a new `ort_` token. Send the old refresh token again. Expect `invalid_grant`, and the newest access token now fails on `/mcp` with `401`.
- **Revocation.** Repeat consent and token exchange, then `POST /oauth/revoke` with `token=<oat_…>` and the `client_id`. Expect `200`, after which `/mcp` returns `401`.
- **Disconnect from the account page.** Repeat consent and token exchange, then open `/admin/account/connections`. The `Authorized MCP clients` card lists `Verify client`. Choose `Disconnect Verify client`, and the access token then gets `401` on `/mcp`. As platform admin, `GET /api/v1/admin/mcp/authorized-clients?user_id=<id>` agrees with the card before and after.
- **Deny.** Start another authorization and press `Deny`. The browser returns to the callback with `error=access_denied` and `iss`.
- **Proof.** Record each check with `control-oceans-admin record`, with the consent screenshot and ARIA snapshot as artifacts.

## Gotchas

- Never record codes, verifiers, access tokens, or refresh tokens in evidence. Record only their prefixes and lengths.
- The consent page and `/oauth/authorize` must use the gateway origin that matches the issuer. Opening the UI port directly breaks the flow.
- The authorization code expires 60 seconds after `Allow`, so exchange it promptly.
- Signing out of the admin UI revokes every client that session approved. Sign out only at the end of the run.
- Consent is asked every time; the gateway never remembers it.
- `http::mcp_client_oauth::{flow_tests, registration_tests, lifecycle_tests, mcp_route_tests, authorized_clients_tests}` prove the full protocol against the real router, including MCP tool calls through a local upstream, without a browser. This recipe adds the browser consent path and the admin UI.
