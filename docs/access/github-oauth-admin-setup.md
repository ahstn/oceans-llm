# GitHub OAuth SSO Setup for Admins

`See also`: [OIDC and SSO](oidc-and-sso-status.md), [Identity and Access](identity-and-access.md), [Configuration Reference](../configuration/configuration-reference.md)

This page covers **direct GitHub OAuth login** for Oceans LLM (without Authentik in the middle).

## What You Configure in GitHub

Create a GitHub **OAuth App** for the specific self-hosted Oceans LLM install.

Required GitHub OAuth App fields:

- **Application name**: your operator-visible name (for example, `Oceans LLM`)
- **Homepage URL**: `https://<your-oceans-host>`
- **Authorization callback URL**: `https://<your-oceans-host>/api/v1/auth/oauth/callback/github`

> For self-hosted installs, this callback URL should match each operator's public Oceans LLM URL.

## GitHub Keys to Store

From GitHub, copy:

- `Client ID`
- `Client Secret`

Set them in your deployment secrets/env:

```text
GITHUB_OAUTH_CLIENT_ID=<github client id>
GITHUB_OAUTH_CLIENT_SECRET=<github client secret>
GATEWAY_PUBLIC_BASE_URL=https://<your-oceans-host>
```

## Oceans LLM Gateway Config

Configure the OAuth provider in gateway config:

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
        sso_email_verification_enabled: true
        allowed_email_domains:
          - example.com
        enabled: true
        jit:
          enabled: false
          global_role: user
          request_logging_enabled: true
```

`allowed_email_domains` is optional. Leave it empty or omit it to allow the existing invite/JIT rules to decide access without an email-domain guardrail. When it is set, GitHub OAuth can only complete for accounts whose selected primary email domain exactly matches one of the configured domains.

`sso_email_verification_enabled` defaults to `true`. With the default, Oceans only accepts the GitHub account's primary email when GitHub marks that email as verified. If GitHub returns no primary verified email, sign-in fails with `github account has no primary verified email`; ask the user to verify their primary email in [GitHub email settings](https://github.com/settings/emails), then retry sign-in.

Set `sso_email_verification_enabled: false` only as an operator escape hatch when you intentionally want Oceans to accept GitHub's primary email even if GitHub has not verified it. Domain restrictions still run against the selected primary email domain.

## Identity Mapping Behavior

Direct GitHub OAuth uses:

- provider subject: GitHub numeric user id (`/user`)
- email for invite/JIT matching: primary email from `/user/emails`; by default it must also be GitHub-verified
- optional domain restriction: exact domain part of the selected primary email

Oceans does **not** auto-link existing password users by email.

## Security Notes

- Keep `jit.enabled: false` unless you explicitly want auto-provisioning.
- Keep `sso_email_verification_enabled: true` unless your deployment has an explicit reason to trust unverified GitHub primary emails.
- When `jit.enabled: true`, set `allowed_email_domains` unless every eligible GitHub primary email should be allowed for JIT provisioning.
- Domain checks run after GitHub returns the selected primary email and before OAuth link creation, invited-user activation, JIT user creation, or session cookie issuance.
- Domain matching is case-insensitive and exact on the email domain. `alice@example.com` matches `example.com`; `alice@evil-example.com` does not.
- Do not grant broad admin roles through JIT unless constrained by your org policy.
- Rotate GitHub client secrets if leaked.
