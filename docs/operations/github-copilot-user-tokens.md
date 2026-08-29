# GitHub Copilot User Tokens

Use GitHub user authentication when each Oceans user must use their own GitHub Copilot entitlement. Use GitHub App authentication when one organization installation must own all Copilot usage.

## Choose an authentication mode

| Mode | Credential owner | Token selection | Recommended use |
| --- | --- | --- | --- |
| `github_user` | One managed Oceans user | Trusted user owner of the gateway API key | Per-user Copilot access and entitlement |
| `github_app` | One GitHub App installation | Provider configuration | Organization server-to-server workloads |
| `bearer` | Gateway operator | One shared provider token | Short local tests only |

The `github_user` mode does not read identity from client headers. The gateway uses the user ID that was attached when the gateway API key was authenticated. Service-account keys cannot use this mode.

## Configure the gateway

Create the provider without a token:

```yaml
providers:
  - id: github-copilot-user
    type: github_copilot
    pricing_provider_id: openai
    auth:
      mode: github_user
```

Set a stable encryption key outside YAML:

```bash
export OCEANS_PROVIDER_CREDENTIAL_ENCRYPTION_KEY="$(openssl rand -base64 32)"
```

Store this value in the deployment secret manager. All gateway replicas must use the same value. Back up the key before an upgrade. The database contains only ciphertext, a nonce, and a key ID. Authenticated encryption binds each ciphertext to its provider and user IDs, so a copied row cannot be decrypted for a different user or provider.

## Get a GitHub token

Sign in to GitHub CLI as the user who owns the Copilot entitlement:

```bash
gh auth status --hostname github.com
gh auth refresh --hostname github.com
gh auth token --hostname github.com
```

Direct Copilot bearer authentication does not need an extra non-default OAuth scope. Do not add `read:user` only for this flow. The user must have an active GitHub Copilot entitlement.

Treat the output of `gh auth token` as a password. Do not place it in shell history, YAML, logs, an issue, or a support message.

## Store the token for a user

1. Sign in to the Oceans admin UI as a platform admin.
2. Open **Identity > Users**.
3. Select the managed user.
4. Open **Provider Configuration**.
5. Find the `github_user` provider.
6. Paste the output of `gh auth token` and select **Save token**.

The UI does not return the stored token. Enter a new token to replace it. Select **Remove token** to revoke the Oceans copy. Removing it does not revoke the token at GitHub.

## Request isolation

Each request follows this sequence:

1. The gateway authenticates the gateway API key.
2. The gateway reads the key's stored user owner ID.
3. The Copilot provider loads the credential for that exact user and provider key.
4. The gateway decrypts the token and updates only that credential's last-used timestamp.
5. The Copilot provider sends the request with the selected token.

If any step cannot prove a user-owned credential, the request fails closed. There is no provider-level user token and no cross-user fallback.

A disabled Oceans user cannot use an existing user-owned gateway API key. The encrypted credential remains stored until a platform admin removes it or the user record is deleted.

## Rotate or remove a token

Use `gh auth refresh` or the GitHub settings page to rotate the GitHub credential. Then replace the stored token in **Provider Configuration**. Remove the Oceans copy before you remove a managed user or disable their Copilot access.

For GitHub App setup and live organization validation, use [GitHub Copilot Installation-Token Canary](github-copilot-installation-canary.md).
