# Self-service Copilot credentials through the Oceans CLI

Date: 2026-10-04

Status: Accepted

## Context

The Oceans CLI introduced in [PR #407](https://github.com/ahstn/oceans-llm/pull/407) authenticates through a gateway API key. Copilot `github_user` credentials already belong to one managed user and one provider key, but their HTTP write endpoint requires a platform-admin browser session. Users need a direct way to supply their own GitHub token without giving it to an admin.

## Decision

Add `oceans providers set-copilot-token [TOKEN] [--token-stdin] [--provider KEY]`. Prefer a hidden terminal prompt or standard input. Keep the positional form for convenience, with its shell-history and process-argument exposure documented. Reuse `OCEANS_URL`, `OCEANS_API_KEY`, `--url`, and `--json`.

Add bearer-only `GET /api/v1/me/provider-credentials` and `PUT /api/v1/me/provider-credentials/{provider_key}` routes. Authentication runs before the write body is parsed. The server derives the user ID from an active user-owned API key and separately checks that the user is active. It never accepts a target user ID or falls back to a session cookie. Service-account keys cannot manage these credentials.

Discovery returns configured `github_user` provider IDs and only the caller's status. The CLI selects the sole eligible provider, or requires `--provider` when there are several. The write accepts only a token and returns metadata without secrets. Neither route exposes stored token values.

Any active user-owned key can manage its owner's Copilot credential. This explicitly expands API-key authority beyond the previous platform-admin write boundary. Model grants do not restrict credential management. Since credentials belong to a user and provider rather than an API key, a replacement affects every key owned by that user for that provider. This follows the CLI's existing self-service model without introducing a separate login or management-scope system.

## Implementation

Keep command parsing and dispatch in `oceans-cli`, with the provider workflow in its own module. The shared HTTP client owns transport and authentication; command groups own their API paths. Preserve existing Skills routes, gateway path prefixes, HTTPS requirements, disabled redirects, and loopback proxy controls.

Keep the new HTTP handlers and their narrow authentication extractor in the gateway's provider credential module. Reuse `ProviderCredentialService` for validation, encryption, replacement, and status. Preserve the existing admin endpoints and Copilot resolver. No schema change or provider-level token fallback is introduced.

Credential requests must not print raw response errors or submitted secrets. The CLI prints typed status metadata only. A successful save means stored, not accepted by GitHub. The runtime continues to resolve credentials per request, so later resolutions see replacements without a restart. Requests that already resolved the previous token can finish.

## Trade-offs and follow-up

- A compromised user-owned key can replace that user's credential and disrupt other clients. It cannot read the previous token or change another user's credential. A future management scope would be needed to separate those permissions.
- Syntactic validation does not prove Copilot entitlement. Upstream checks remain separate from storage and must not become an implicit paid inference call.
- Users remain responsible for replacing expired or revoked GitHub tokens. The gateway's stable encryption-key requirement remains unchanged.
- Token removal remains available to platform admins. A future CLI removal command must distinguish removing the Oceans copy from revoking the token at GitHub.

See the [GitHub Copilot provider guide](../providers/github-copilot.md) for user instructions.
