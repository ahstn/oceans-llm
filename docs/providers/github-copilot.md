# GitHub Copilot

Currently, the GitHub Copilot provider supports two authentication modes:

- `github_app` (recommended) - uses a GitHub App installation token for organization-wide access.
- `github_user` - mode uses a GitHub user token for per-user access

Both modes exchange GitHub tokens with the Copilot HTTP API rather than using the Copilot SDK.

> [!WARNING]
> [GitHub's server-to-server authentication guide](https://docs.github.com/en/copilot/how-tos/copilot-sdk/auth/server-to-server-tokens) describes GitHub App authentication as a valid flow. The GitHub App registration does not currently expose the required `copilot_requests: write` permission. Until GitHub exposes this permission, do not use `github_app` mode for production workloads. Use `github_user` mode for per-user access instead.
>
> See also: [GitHub Copilot SDK issue #2304](https://github.com/github/copilot-sdk/issues/2304) for a discussion of the missing permission and its impact.

## GitHub user authentication

### Configure the gateway (admins)

Create the provider without a token:

```yaml
providers:
  - id: github-copilot-user
    type: github_copilot
    pricing_provider_id: openai
    auth:
      mode: github_user
```

Set a stable encryption key for encrypting user-provided Copilot tokens outside YAML:

```bash
export OCEANS_PROVIDER_CREDENTIAL_ENCRYPTION_KEY="$(openssl rand -base64 32)"
```

### Fetch and store a Copilot token (users)

Sign in to GitHub CLI with your user account:

```bash
gh auth status --hostname github.com
gh auth refresh --hostname github.com
gh auth token --hostname github.com
```

> [!WARNING]
>
> Treat the output of `gh auth token` as a password. Do not place it in shell history, YAML, logs, an issue, or a support message.

Store the token in Oceans using the following steps:

1. Sign in to the Oceans admin UI
2. Open **Identity > Users**.
3. Select your user profile.
4. Open **Provider Configuration**.
5. Find the `github_user` provider.
6. Paste the output of `gh auth token` and select **Save token**.

The UI does not return the stored token. Enter a new token to replace it. Select **Remove token** to revoke the Oceans copy. Removing it does not revoke the token at GitHub.

### Request isolation

Each request follows this sequence:

1. The gateway authenticates the gateway API key.
2. The gateway reads the key's stored user owner ID.
3. The Copilot provider loads the credential for that exact user and provider key.
4. The gateway decrypts the token and updates only that credential's last-used timestamp.
5. The Copilot provider sends the request with the selected token.

If any step cannot prove a user-owned credential, the request fails. No provider-level user token or cross-user fallback exists.

A disabled Oceans user cannot use an existing user-owned gateway API key. The encrypted credential remains stored until a platform admin removes it or deletes the user record.

## GitHub App authentication

Prepare a GitHub App and installation as described in [GitHub's server-to-server authentication guide](https://docs.github.com/en/copilot/how-tos/copilot-sdk/auth/server-to-server-tokens):

1. Give the App the **Copilot Requests** repository permission and select **Read & write**.
2. Install the App on the organization that must own the usage.
3. Give the installation **All repositories** access. GitHub currently requires this even though GitHub scopes each token to one repository ID.
4. Enable Copilot requests from GitHub App installations for the organization.
5. Select one model that advertises `/chat/completions`, streaming, and tool calls.
6. Select one model that advertises `/v1/messages` and streaming.
