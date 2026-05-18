# Direct GitHub OAuth SSO Implementation Plan

## Goal

Support self-hosted Oceans LLM deployments that authenticate users directly with a GitHub OAuth App, without requiring Authentik or another OIDC broker.

The desired operator flow is:

1. Admin creates a GitHub OAuth App for their own exposed Oceans LLM install.
2. Admin stores the GitHub OAuth app credentials in Oceans gateway config/env.
3. Users click **Sign in with GitHub** on the Oceans login page.
4. Oceans exchanges the GitHub OAuth code, fetches the verified GitHub identity/email, and creates an Oceans session.

## Key protocol decision

Keep direct GitHub login separate from OIDC.

OIDC is an identity layer on OAuth2 and provides discovery, ID tokens, nonce verification, standard `sub`, and standard identity claims. The existing gateway implementation depends on those properties.

GitHub browser login is OAuth2. It does not return an OIDC `id_token` for end-user login. GitHub's OIDC provider is for GitHub Actions workload identity, not web app user authentication.

Therefore GitHub direct SSO needs a first-class OAuth provider type rather than being forced through the existing OIDC provider path.

## Desired self-hosted GitHub setup

Each self-hosted operator creates their own GitHub OAuth App.

GitHub OAuth App fields:

```text
Application name:
  Oceans LLM, or an install/org-specific name

Homepage URL:
  https://<oceans-host>

Authorization callback URL:
  https://<oceans-host>/api/v1/auth/oauth/callback/github
```

Gateway config should store:

```yaml
auth:
  oauth:
    public_base_url: env.GATEWAY_PUBLIC_BASE_URL
    providers:
      - key: github
        label: GitHub
        provider_type: github
        client_id: env.GITHUB_OAUTH_CLIENT_ID
        client_secret: env.GITHUB_OAUTH_CLIENT_SECRET
        scopes:
          - read:user
          - user:email
        enabled: true
        jit:
          enabled: false
```

Required environment values:

```text
GATEWAY_PUBLIC_BASE_URL=https://<oceans-host>
GITHUB_OAUTH_CLIENT_ID=<GitHub OAuth App client ID>
GITHUB_OAUTH_CLIENT_SECRET=<GitHub OAuth App client secret>
```

## GitHub identity rules

Use:

- GitHub numeric user id from `GET https://api.github.com/user` as the durable provider subject.
- Primary verified email from `GET https://api.github.com/user/emails` as the email for invite matching/display.
- Scopes `read:user` and `user:email`.

Do not use GitHub username or email as the durable identity key.

Provider identity should be keyed by:

```text
(provider_id, provider_subject)
```

Email should only be used for invite matching after provider-specific verification.

## Current gaps

Current Oceans gateway support is OIDC-only in practice.

Important existing locations:

- `crates/gateway/src/http/identity.rs`
  - has `/api/v1/auth/oidc/start`
  - has `/api/v1/auth/oidc/callback`
  - requires an OIDC `id_token`
- `crates/gateway/src/config.rs`
  - validates `auth.oidc.providers`
  - requires `issuer_url`
  - requires `openid` scope
  - currently rejects configured `auth_mode: oauth`
- `crates/gateway-core/src/domain.rs`
  - has `AuthMode::Oauth`, but no full provider model
- `crates/gateway-store/migrations/V17__baseline.sql`
  - has `user_oauth_auth`, but no `oauth_providers` or OAuth login state
- `crates/gateway-store/src/store.rs`
  - has OIDC provider/state/auth methods only
- `crates/admin-ui/web/src/routes/login.tsx`
  - only fetches/renders OIDC providers
- `deploy/config/gateway.local.yaml`
  - only shows Authentik OIDC

## Implementation plan

### 1. Add OAuth config model

Add `auth.oauth` with providers.

Suggested Rust types:

- `AuthOauthConfig`
- `OauthProviderConfig`
- `OauthJitConfig` or a generic shared SSO JIT config

Validate:

- provider keys normalize like OIDC provider keys
- `provider_type` initially supports `github`
- `client_id` and resolved `client_secret` are non-empty
- scopes are non-empty and valid
- GitHub default scopes are `read:user`, `user:email`
- JIT membership team exists
- JIT cannot assign owner role

Add `seed_oauth_providers()`.

Enable config-declared users with:

```yaml
users:
  - email: user@example.com
    auth_mode: oauth
    oauth_provider_key: github
```

### 2. Add domain/storage support

Add domain records:

- `OauthProviderRecord`
- `SeedOauthProvider`
- `OauthLoginStateRecord`
- `UserOauthAuthRecord`

Add migrations for libsql and Postgres:

- `oauth_providers`
- `oauth_login_states`
- `user_oauth_links`
- extend or replace existing `user_oauth_auth` to include `oauth_provider_id` and `email_claim`

Prefer provider-id based storage to mirror OIDC:

```text
user_id
oauth_provider_id
subject
email_claim
created_at
```

Add store trait methods parallel to OIDC:

- `list_enabled_oauth_providers`
- `get_enabled_oauth_provider_by_key`
- `create_oauth_login_state`
- `consume_oauth_login_state`
- `get_user_oauth_auth`
- `get_user_oauth_auth_by_user`
- `create_user_oauth_auth`
- `delete_user_oauth_auth`
- `find_invited_oauth_user`
- `set_user_oauth_link`
- `clear_user_oauth_link`

### 3. Add OAuth routes

Add routes separate from OIDC:

```text
GET /api/v1/auth/oauth/providers
GET /api/v1/auth/oauth/start?provider_key=github
GET /api/v1/auth/oauth/callback/github
```

Keep OIDC routes unchanged.

### 4. Implement GitHub OAuth flow

Start route:

1. Load enabled OAuth provider.
2. Build callback URL from `GATEWAY_PUBLIC_BASE_URL`:
   `https://<oceans-host>/api/v1/auth/oauth/callback/github`.
3. Generate one-time state.
4. Generate PKCE verifier/challenge if supported by the implementation.
5. Redirect to `https://github.com/login/oauth/authorize` with `client_id`, `redirect_uri`, `scope`, `state`, and PKCE values when present.

Callback route:

1. Validate/consume state.
2. Exchange code at `https://github.com/login/oauth/access_token`.
3. Fetch `GET https://api.github.com/user`.
4. Fetch `GET https://api.github.com/user/emails`.
5. Select primary verified email.
6. Use GitHub numeric `id` as subject.
7. Run the same account resolution policy as OIDC:
   - existing provider subject link logs in
   - invited OAuth user with same verified email/provider activates and links
   - existing local/password email conflict rejects
   - JIT create only if enabled

Do not persist GitHub access tokens unless future API access requires it.

### 5. Enable OAuth user lifecycle

Update places where `AuthMode::Oauth` is currently rejected/unsupported:

- config user validation
- store seeding
- user create/update identity APIs
- onboarding reset
- auth proof checks
- onboarding response generation
- identity sync when switching auth modes

Add `oauth_provider_key` request/response fields where OIDC has `oidc_provider_key`.

### 6. Update admin API and UI

Backend:

- Add public OAuth provider listing.
- Add OAuth provider lists to identity payloads.
- Add OAuth onboarding/sign-in response variants.
- Regenerate OpenAPI.

UI:

- Login page renders OAuth providers alongside OIDC providers.
- User/team identity forms support OAuth auth mode and OAuth provider selection.
- Onboarding messages support GitHub sign-in URLs.

### 7. Docs and examples

Add an admin-facing docs subpage under `docs/access/` for GitHub SSO/OAuth setup. Link it from `docs/access/oidc-and-sso-status.md`.

Include:

- GitHub OAuth App fields
- callback URL
- required keys/env vars
- gateway config example
- JIT warning
- troubleshooting notes

Add disabled examples to gateway config files.

### 8. Tests

Add tests for:

- config validation
- provider listing
- OAuth state consume-once
- GitHub start redirect URL
- GitHub callback success with mocked GitHub endpoints
- no verified email failure
- email conflict rejection
- invited user activation
- JIT disabled/enabled behavior

## Other provider considerations

Do not over-generalize OAuth provider logic too early.

Recommended provider split:

- OIDC providers: `generic_oidc`, Authentik, Okta, Google, Microsoft Entra, GitLab OIDC
- OAuth providers: GitHub first, Discord later if needed

Provider-specific reminders:

- Google: prefer OIDC, use `sub`, not email.
- Microsoft Entra: prefer OIDC, use subject plus issuer/tenant context; email-like claims are mutable.
- GitLab: supports OIDC; prefer OIDC with `openid profile email`.
- Discord: OAuth-only style; would need an adapter like GitHub; use Discord snowflake id and require verified email.

## Security notes

- Always validate one-time state.
- Use PKCE where supported.
- Do not auto-link existing password users by email alone.
- Require verified provider email before invite/JIT matching.
- Default JIT to disabled for public providers like GitHub.
- Warn admins that JIT with `platform_admin` for GitHub can admit any accepted GitHub account unless additional allowlisting is added.
