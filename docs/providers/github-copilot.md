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

### Store a Copilot token with the CLI (users)

Install the [Oceans CLI](../setup/skills.md#use-the-oceans-cli). Set `OCEANS_URL` to your gateway and supply a user-owned Oceans API key through `OCEANS_API_KEY`. These settings authenticate you to Oceans; the GitHub token is a separate credential.

Check that GitHub CLI uses the account with your Copilot access, then send its token directly to Oceans:

```bash
gh auth status --hostname github.com
gh auth token --hostname github.com | oceans providers set-copilot-token --token-stdin
```

You can also enter the token at a prompt that does not display your input:

```bash
oceans providers set-copilot-token
```

The command selects the provider automatically when the gateway has exactly one provider in `github_user` mode. If several providers exist, select the configured provider ID:

```bash
oceans providers set-copilot-token --provider github-copilot-user
```

The command also accepts `oceans providers set-copilot-token TOKEN`. Prefer the prompt or `--token-stdin`: a positional token can appear in shell history and process listings. Never place a real token in YAML, logs, an issue, or a support message. Use only one input method. Scripts must use an explicit token or `--token-stdin`; the command does not prompt when standard input is not a terminal.

`--url` overrides `OCEANS_URL`, and `--json` returns status metadata without the token. Remote gateways require HTTPS. The command permits HTTP only for loopback IP addresses and does not follow redirects.

A successful save confirms that Oceans stored the token. It does not confirm GitHub entitlement, model access, or token validity. Oceans does not refresh your GitHub user token. Run the command again to replace an expired or revoked token.

The credential belongs to your Oceans user and the selected provider, rather than to one API key. Replacement affects all your user-owned keys for that provider. Any active API key owned by your user can make this change, including a key with restricted model access. Service-account keys and disabled users cannot use this command. The command cannot read a saved token or change another user's credential.

### Manage stored tokens in the UI (platform admins)

Platform admins can store or replace a user's token through the existing admin UI:

1. Sign in to the Oceans admin UI as a platform admin.
2. Open **Identity > Users**.
3. Select the user's profile.
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

> [!NOTE]
> GitHub does not document where to enable "Copilot requests from GitHub App installations" (step 4). It may map to the organization Copilot policy **Allow use of Copilot CLI billed to the organization**, found under **Copilot CLI** in the organization's Copilot policy settings. This is unconfirmed. Enable that policy if the App is rejected with `401 Unauthorized`, and report back whether it resolves the error.
