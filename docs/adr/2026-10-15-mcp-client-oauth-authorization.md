# ADR: OAuth Authorization for MCP Clients Connecting to the Gateway

- Date: 2026-10-15
- Status: Accepted
- Issue: #441 (epic #458)

## Current state

- [MCP Client Setup: OAuth sign-in](../mcp/mcp-client-setup.md#oauth-sign-in)
- [OIDC and SSO](../access/oidc-and-sso-status.md)

## Context

`/mcp` and `/mcp/{server_key}` accept only a `gwk_` API key, sent as `Authorization: Bearer` or `x-oceans-api-key`. Many MCP clients cannot send a custom header. They expect to discover an authorization server from a `401`, register, send the user through a browser sign-in, and receive tokens. Those clients cannot connect to Oceans today.

Where a client can send headers, the user has to create a long-lived key and paste it into a config file. That key is not tied to the user's SSO session and keeps working after they sign out.

This ADR targets MCP protocol revision `2026-07-28`, the current revision when it was written. That revision's authorization section requires:

- Protected Resource Metadata (RFC 9728) with at least one `authorization_servers` entry, and a `WWW-Authenticate: Bearer resource_metadata="…"` challenge on `401`.
- Authorization Server Metadata (RFC 8414) or OpenID Connect Discovery.
- OAuth 2.1 with PKCE. `code_challenge_methods_supported` must be present, or clients refuse to continue.
- Resource Indicators (RFC 8707). Clients send `resource` on both the authorization and token requests, and the server must reject tokens not issued for it.
- Exact redirect URI matching. Redirect URIs must be HTTPS or loopback.
- Short-lived access tokens, and refresh-token rotation for public clients.
- Client registration through Client ID Metadata Documents (SHOULD), pre-registration, or Dynamic Client Registration (RFC 7591, MAY, deprecated but kept for compatibility).
- The `iss` authorization response parameter (RFC 9207) is a SHOULD and is expected to become a MUST.
- `403` with `error="insufficient_scope"` when a token lacks a required scope.

## Decision

The gateway is both the protected resource and the authorization server for its MCP routes. Sign-in reuses the admin-UI session, so password, OIDC, and GitHub sign-in all work without changes. An access token resolves to the same Oceans user as that user's API keys. The MCP grant, guardrail, and invocation-logging code then runs unchanged.

### 1. Issuer, resources, and endpoints

The feature is off unless `mcp.client_oauth.enabled` is `true`. The issuer is `mcp.client_oauth.public_base_url`. When that field is not set, the issuer falls back to `mcp.oauth.public_base_url`, then `auth.oidc.public_base_url`, then `auth.oauth.public_base_url`. Startup fails if the feature is enabled and none of these is set. The issuer must be an HTTPS origin. Plain `http` is accepted only for loopback hosts, so the local stack works. The issuer is never derived from request headers, because the issuer and resource URIs are the values that tokens are bound to.

| Resource identifier | Route |
| --- | --- |
| `{issuer}/mcp` | Aggregate endpoint `/mcp`, and, with `mcp:proxy`, every per-server route |
| `{issuer}/mcp/{server_key}` | Only the per-server route `/mcp/{server_key}` |

| Endpoint | Purpose |
| --- | --- |
| `GET /.well-known/oauth-protected-resource/mcp` | RFC 9728 metadata for `{issuer}/mcp` |
| `GET /.well-known/oauth-protected-resource/mcp/{server_key}` | RFC 9728 metadata for one server |
| `GET /.well-known/oauth-authorization-server` | RFC 8414 metadata. The issuer has no path, so this is the only location |
| `GET /oauth/authorize` | Authorization endpoint (code flow) |
| `POST /oauth/token` | Token endpoint (`authorization_code`, `refresh_token`) |
| `POST /oauth/register` | Dynamic Client Registration (RFC 7591) |
| `POST /oauth/revoke` | Token revocation (RFC 7009) |

There is no root `/.well-known/oauth-protected-resource` document. RFC 9728 requires the `resource` in a document to match the URL it was fetched for. A root document would therefore name `{issuer}`, which is not an MCP resource. Clients find the per-route document from the `WWW-Authenticate` header first, and from the path-suffixed well-known URI second.

The metadata advertises `response_types_supported: ["code"]`, `grant_types_supported: ["authorization_code", "refresh_token"]`, `code_challenge_methods_supported: ["S256"]`, `token_endpoint_auth_methods_supported: ["none", "client_secret_post", "client_secret_basic"]`, and `authorization_response_iss_parameter_supported: true`. The authorization response always carries `iss`, including error responses.

### 2. Token format: opaque, stored as a hash

Access tokens are `oat_` plus 32 random bytes, base64url-encoded. Refresh tokens use the `ort_` prefix in the same format. The gateway stores only the SHA-256 hash of each token and looks tokens up by that hash. A fast hash is enough here because the tokens are 256-bit random values. Argon2, used for `gwk_` secrets, is meant for secrets that could be guessed.

We chose opaque tokens over JWTs because:

- The resource server and the authorization server are the same process with the same database. A JWT's offline verification gains nothing, since the gateway already reads the database on every MCP request to resolve grants.
- Revocation has to take effect at once when a user is deactivated or signs out. A JWT stays valid until it expires unless every request also checks a denylist.
- There is no signing key to rotate and no JWKS endpoint to publish.

The trade-off is one indexed lookup per request. API keys already pay that cost.

The prefix lets the MCP routes choose the credential type before any lookup. `gwk_` goes to the API-key authenticator and `oat_` goes to the OAuth validator. Anything else is rejected.

### 3. Storage and lifetimes

Four tables back the flow. They use a `gateway_oauth_` prefix to keep them apart from the `mcp_oauth_*` tables, which hold Oceans' own tokens for upstream servers.

| Table | Holds |
| --- | --- |
| `gateway_oauth_clients` | Registered clients: name, URIs, redirect URIs, grant types, auth method, and a secret hash for confidential clients |
| `gateway_oauth_authorization_requests` | One row per `/oauth/authorize` call: client, redirect URI, `state`, PKCE challenge, resource, and scope. After consent it also holds the user, the approving session, and the code hash. Each row can be used once |
| `gateway_oauth_grants` | One row per approved consent, so one token family: client, user, approving `user_sessions` row, resource, scope, and revocation |
| `gateway_oauth_tokens` | Access and refresh token hashes, each with an expiry. A refresh token also records when it was consumed |

| Item | Lifetime |
| --- | --- |
| Pending authorization request | 10 minutes |
| Authorization code | 60 seconds, single use |
| Access token | `access_token_ttl_seconds`, default 3600 (1 hour), maximum 86400 |
| Refresh token | `refresh_token_ttl_seconds`, default 2592000 (30 days) |

Every expiry is capped at the approving session's `expires_at`. A token family never outlives the SSO session that approved it.

**Refresh rotation.** Each refresh returns a new refresh token and consumes the old one, for public and confidential clients alike. If a consumed refresh token is presented again, the gateway revokes the whole grant, following the OAuth 2.1 reuse-detection guidance. A refresh request may name the same resource as the grant, or none. It may narrow the scope but never widen it.

**Code replay.** If an authorization code is redeemed twice, the second attempt fails and the grant created by the first is revoked. Only the client the code was issued to, presenting its PKCE verifier, can trigger that revocation, so a code leaked through logs cannot be used to end someone else's grant. The token endpoint creates the grant and tokens first and consumes the code last, with a conditional update. A failed write leaves the code redeemable for a retry. When two requests race, the loser revokes both its own grant and the winner's. Refresh rotation uses the same order. When the authorization request sent `redirect_uri`, the token request must repeat it exactly (RFC 6749 section 4.1.3).

### 4. Audience binding (RFC 8707)

Both the authorization and token requests accept `resource`. The gateway normalizes the value with a URL parser, which lowercases the scheme and host and strips a trailing slash. It must then equal `{issuer}/mcp` or `{issuer}/mcp/{server_key}` for an active server. Any other value returns `invalid_target`.

- **Authorization request without `resource`:** the gateway uses `{issuer}/mcp`.
- **Token request with `resource`:** the value must equal the authorized resource.
- **Token request without `resource`:** the token is bound to the resource that was authorized, so it never gets a wider audience than the user approved. The MCP spec requires clients to send `resource` on both requests, but RFC 8707 makes it optional at the token endpoint. Rejecting an omission would break clients that send it only at authorization, without improving security.

At request time, the token's resource must cover the route:

- `{issuer}/mcp` covers `/mcp` and every `/mcp/{server_key}`.
- `{issuer}/mcp/{server_key}` covers only that route.

Non-MCP routes never accept an `oat_` token. Inference, admin, batch, and Skills routes go through the `gwk_` parser, and it rejects every `oat_` token as a malformed key. The gateway never forwards an `oat_` token to an upstream MCP server. Upstream credentials still come from the existing binding and connection code.

### 5. Scope model, shared with #446

| Scope | Allows |
| --- | --- |
| `mcp:aggregate` | The aggregate endpoint `/mcp` |
| `mcp:proxy` | Per-server routes `/mcp/{server_key}` within the token's resource |

These are the capability names #446 proposes for API keys. A future scoped `gwk_` key and an OAuth token with the same scopes will therefore reach the same routes. The token's resource does the job of #446's optional server list.

If the client sends no scope, the gateway picks one from the resource: `mcp:aggregate` for `{issuer}/mcp`, and `mcp:proxy` for a per-server resource. Each protected-resource metadata document lists every scope its resource accepts: both for `{issuer}/mcp`, and `mcp:proxy` for a server. An unknown scope returns `invalid_scope`. `mcp:aggregate` with a per-server resource also returns `invalid_scope`. The `401` challenge names the scope its route needs. A token without that scope gets `403` with `error="insufficient_scope"`.

Scopes only narrow access. A token never reaches a tool that the user's own grants do not allow. `offline_access` is not used. Whether a refresh token is issued depends on whether the client registered the `refresh_token` grant type.

### 6. Consent screen

`/oauth/authorize` checks the client, the redirect URI, PKCE (S256 only, required), the resource, and the scope. When the client or the redirect URI is invalid, the gateway shows an error page and never redirects. For any other error, it redirects to the registered URI with `error` and `iss`. A valid request is stored, and the browser is sent to `/admin/oauth/consent?request_id=…`. That page uses the admin UI's normal login redirect, so a signed-out user signs in with any configured method and returns to the page afterwards.

The consent page shows:

- the signed-in user
- the client name, marked as provided by the client
- the redirect URI's host, which the spec says must be shown
- a warning when the redirect URI for this request is loopback, judged per request rather than per registration
- the MCP server, or "all MCP servers you have access to"
- the requested scopes

Approve and Deny post to session-authenticated APIs. Approving binds the request to the user and their current `user_sessions` row, and returns the redirect with `code`, `state`, and `iss`. Denying returns `error=access_denied`.

Consent is asked for every authorization. It is never remembered. Dynamic registration lets anyone register a client, so remembered consent would let such a client get tokens silently later.

Only active users can approve. A user who has no MCP grants can still approve, but gets no tools, exactly like an API key with no grants.

### 7. Client registration: Dynamic Client Registration first

`POST /oauth/register` implements RFC 7591 without an initial access token. It accepts `redirect_uris` (required), `client_name`, `client_uri`, `logo_uri`, `grant_types`, `response_types`, `token_endpoint_auth_method`, `software_id`, `software_version`, and `application_type`. It applies these rules:

- Every redirect URI must be HTTPS, or `http` on `localhost`, `127.0.0.1`, or `[::1]`. Fragments are rejected. Custom URI schemes are rejected, as the MCP spec requires.
- Matching is exact. The one exception follows RFC 8252: a loopback redirect URI may use any port.
- `grant_types` defaults to `authorization_code` and must be a subset of `authorization_code` and `refresh_token` that includes `authorization_code`. `response_types` may only be `code`.
- Registration accepts at most 10 redirect URIs and a 16 KiB body. Text fields are limited to 200 printable characters, and `client_uri` and `logo_uri` must be HTTPS.
- `token_endpoint_auth_method` defaults to `client_secret_basic`, as RFC 7591 specifies. MCP clients without a backend register with `none`. `client_secret_post` and `client_secret_basic` get a generated `ocs_` secret. Only its Argon2 hash is stored, and the secret is returned once.
- A registered client can do nothing until a user approves it on the consent screen.
- Argon2 (secret hashing at registration, secret checks at the token and revocation endpoints) runs on the blocking pool, at most four at a time. A request that waits more than five seconds for a slot gets `503 temporarily_unavailable`, so unauthenticated floods cannot starve the async executor. Per-IP throttling is a follow-up.

`mcp.client_oauth.dynamic_client_registration` (default `true`) turns the endpoint off. When it is off, `registration_endpoint` is left out of the metadata.

We ship Dynamic Client Registration before Client ID Metadata Documents, even though the spec now prefers the latter:

- Most deployed MCP clients still register dynamically, because earlier revisions required it.
- Client ID Metadata Documents make the gateway fetch a URL the client chooses. That needs an outbound fetcher that blocks requests to private addresses (SSRF). The gateway does not have one yet. Upstream metadata discovery in #442 needs the same guard, so we will build it once and share it rather than build two.

Until then the metadata omits `client_id_metadata_document_supported`. Clients that follow the spec's priority order then fall back to registration.

### 8. Coexistence with `gwk_` keys

| | `gwk_` API key | `oat_` OAuth token |
| --- | --- | --- |
| Routes | All routes, as today | `/mcp` and `/mcp/{server_key}` only |
| Headers | `Authorization: Bearer` or `x-oceans-api-key` | `Authorization: Bearer` only |
| Grant subjects | The key, its owner user or service account, and the owner's team | The user and the user's team |
| Owner kinds | User or service account | User only |

API-key behavior does not change. Grants given directly to a specific key do not apply to OAuth tokens. They belong to that key, and there is no key in an OAuth call. User and team grants resolve exactly as they do for that user's keys.

Rate limits (#25) treat each OAuth grant as its own credential. An `api_key`-scoped policy counts every grant separately, and a policy pinned to one key ID never matches a grant. `user`- and `global`-scoped policies apply to OAuth calls the same way they apply to the user's keys.

In code, an OAuth principal is an `AuthenticatedApiKey` whose `oauth` field is set. That keeps the type every MCP handler already takes. In that case `id` is the grant ID, which is a stable identity for aggregate-session binding but not an `api_keys` row. `api_key_id()` returns `None` for OAuth principals, and `auth_method()` reports `api_key` or `oauth`. Grant resolution, invocation logging, and aggregate sessions read those accessors instead of `id`. An OAuth principal only reaches MCP handlers, because no other route accepts `oat_` tokens.

Invocation logs leave `api_key_id` empty for OAuth calls and add `auth_method`, `oauth_client_id`, and `oauth_grant_id` to the invocation's `metadata`. #448 owns the invocation schema and can promote those to columns without a data migration from this change. Aggregate sessions store either `api_key_id` or `gateway_oauth_grant_id`, never both, so a session opened with one credential cannot be reused with another.

### 9. Revocation

Every MCP request with an `oat_` token checks all of these:

- the token's hash, expiry, and kind (`access`)
- the grant has not been revoked
- the approving `user_sessions` row has not been revoked or expired
- the user is still active

So access stops at once when:

- **The user signs out.** Logout revokes the session, and every grant that session approved stops working.
- **The user is deactivated.** Deactivation revokes all of the user's sessions and, explicitly, all their grants.
- **The client revokes a token.** `/oauth/revoke` revokes the grant behind the token, access or refresh. RFC 7009 permits revoking the refresh token along with an access token. Unknown tokens return `200`, as the RFC requires.
- **A refresh token is reused.** The gateway revokes the grant (section 3).
- **The user disconnects the client.** The `Authorized MCP clients` card on `/admin/account/connections` lists the user's live grants and revokes one with `Disconnect`. The card uses `GET /api/v1/me/authorized-mcp-clients` and `DELETE /api/v1/me/authorized-mcp-clients/{grant_id}`. A user can only revoke their own grants.
- **A platform admin revokes it.** `GET /api/v1/admin/mcp/authorized-clients?user_id=…` lists a user's live grants. `POST /api/v1/admin/mcp/authorized-clients/revoke` revokes one grant, or every grant when `grant_id` is omitted.

The grant's `revoked_reason` records which path ended it: `user_revoked`, `admin_revoked`, `client_revoked`, `user_deactivated`, `refresh_token_reused`, or `authorization_code_replayed`.

### 10. Cleanup

An hourly loop runs while the feature is enabled. It deletes:

- dynamically registered clients that never received a grant, once they are older than `mcp.client_oauth.unused_client_retention_days` (default 7)
- authorization requests and tokens that expired more than a day ago, unless their grant can still be used

A consumed code or refresh token is the only record that links a replay to its grant. These rows are therefore kept while the grant is not revoked and its approving session has not ended, so a replay at any point in the grant's life still revokes it. After that, the one-day grace still lets a late replay meet the record. The rows a live grant keeps are bounded by the session's lifetime. Grants are never deleted, because they are the audit trail for which client acted for a user. Invocation metadata refers to them by ID.

A response that streams events, such as a server's `GET` stream, outlives the check made when it opened. The gateway therefore closes an OAuth caller's event stream when its access token expires. It also rechecks the token, grant, session, and user every 30 seconds, and closes the stream as soon as any of them stops being valid. Revocation reaches an open stream within that interval.

Turning `mcp.client_oauth` off stops all OAuth access, but it does not revoke anything. Grants come back if the feature is turned on again. Listing and revoking authorized clients therefore keep working while it is off, both on the account page and through the admin API. With no issuer configured, the listing shows every live grant.

## Trade-offs

- Signing out of the admin UI also disconnects the MCP clients that session approved. That is the guarantee #441 asks for, but it can surprise someone who signs out to switch accounts. The consent page says so.
- OAuth calls cannot use grants given to a specific key. An admin who relies on per-key MCP grants has to give the user or team the grant before OAuth users see those tools.
- Asking for consent on every authorization adds a click each time a client signs in again. Remembered consent can come later, with an allowlist of trusted clients.
- Open registration lets anyone create client rows. The rows are inert without consent, and cleanup removes the unused ones after the retention window. Until then they take space.
- The issuer must be configured. A deployment that serves the gateway at more than one origin can expose OAuth on only one of them.
- Plain `http` loopback issuers break the HTTPS rule for authorization servers. They are allowed only so the local stack and tests work.

## Follow-up

- Client ID Metadata Documents, once #442 provides an SSRF-safe fetcher.
- An admin UI view of a user's authorized clients. The admin API exists; the user management sheet does not show it yet.
- Pre-registered clients that admins manage, with an option to skip consent for trusted clients.
- CORS on the metadata, token, registration, and revocation endpoints for browser-based clients.
- Show `auth_method` in the invocation-log UI and filters (#448).
- Capability scopes on API keys (#446), using the same names.
