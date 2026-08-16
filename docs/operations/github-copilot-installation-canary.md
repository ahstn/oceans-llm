# GitHub Copilot Installation-Token Canary

Use this canary before you enable a GitHub Copilot provider that uses GitHub App authentication. It sends a newly minted `ghs_` installation token directly to the Copilot HTTP API. It does not use the Copilot SDK or a stored user credential.

The canary tests these contracts:

- the App installation belongs to the expected organization
- the App has **All repositories** access and `copilot_requests: write`
- the minted token response contains the requested repository and permission
- `/models`, `/chat/completions`, and `/v1/messages` accept the installation token
- Chat Completions and Messages streaming return valid server-sent events
- a user-initiated forced function call returns a tool call, then an agent-initiated tool-result continuation succeeds
- a second, distinct installation token works against `/models`
- GitHub identifies the expected installation owner for billing attribution
- the organization AI-credit billing aggregate is readable, when an administrator token is supplied

This is an opt-in live test. It makes six inference requests and can consume Copilot AI credits. Do not run it in normal CI.

## Prerequisites

Prepare a GitHub App and installation as described in [GitHub's server-to-server authentication guide](https://docs.github.com/en/copilot/how-tos/copilot-sdk/auth/server-to-server-tokens):

1. Give the App the repository permission **Copilot Requests: Read & write**.
2. Install the App on the organization that must own the usage.
3. Give the installation **All repositories** access. GitHub currently requires this even though each canary token is scoped to one repository ID.
4. Enable Copilot requests from GitHub App installations for the organization.
5. Select one model that advertises `/chat/completions`, streaming, and tool calls.
6. Select one model that advertises `/v1/messages` and streaming.

Use a disposable App private key when possible. Store the key outside the repository and remove group and other access:

```bash
chmod 600 /secure/path/copilot-canary.private-key.pem
```

The canary refuses to use a key file with group or other permissions.

## Required environment

Set the App and installation values. Do not put them in a checked-in environment file.

```bash
export COPILOT_CANARY_APP_ID="<github-app-id>"
export COPILOT_CANARY_INSTALLATION_ID="<installation-id>"
export COPILOT_CANARY_REPOSITORY_ID="<numeric-repository-id>"
export COPILOT_CANARY_EXPECTED_OWNER="<organization-login>"
export COPILOT_CANARY_PRIVATE_KEY_PATH="/secure/path/copilot-canary.private-key.pem"
```

The repository ID must be numeric. You can read it without writing a token to disk:

```bash
gh api repos/OWNER/REPOSITORY --jq .id
```

## Discover the allowed models

Run the canary once without model variables:

```bash
mise run copilot-installation-canary >copilot-canary-discovery.json
```

The result is `INCOMPLETE`. The `models` check contains the enabled model IDs, advertised endpoints, and a safe projection of `capabilities.supports`. A projected support value is `true` only when `/models` returns the exact Boolean value `true`. Missing or different values become `false`. This first run still proves token minting, `/models` authorization, refresh, and cleanup. It does not send an inference request.

Select models from that exact response. Do not infer endpoint support from a model-name prefix.

```bash
export COPILOT_CANARY_CHAT_MODEL="<tool-capable-chat-model>"
export COPILOT_CANARY_MESSAGES_MODEL="<messages-model>"
```

## Map model evidence to a route

Use the inventory for the exact upstream model. Do not copy capability values from a different model or an older report. See [GitHub Copilot provider and route evidence](../configuration/configuration-reference.md#github-copilot-provider-and-route-evidence) for the complete provider and route schema.

Set each compatibility field only from the same model inventory. The current Copilot `/models` response has no developer-role support field, so the gateway keeps `developer_role` disabled for Copilot routes.

## Run the full canary

Run the test and save its non-secret JSON evidence:

```bash
mise run copilot-installation-canary >copilot-canary-report.json
```

The process returns these exit codes:

| Exit code | Result | Meaning |
| --- | --- | --- |
| `0` | `PASS` | Every required check passed, including a visible organization billing-usage increase. Request-level billing identity remains unavailable from public APIs. |
| `1` | `FAIL` | A required contract failed. Do not enable GitHub App mode. |
| `2` | `INCOMPLETE` | A required check was unavailable, usually because model IDs were not supplied. |

Each check has an explicit `PASS`, `FAIL`, or `UNAVAILABLE` status. The report records model IDs, endpoint counts, response shapes, expiry times, and numeric billing summaries. It never records the App JWT, private key, installation tokens, request bodies, or response content.

The script keeps each installation token in memory. It revokes every token that it minted before it exits. A forced process stop can prevent cleanup. If this occurs, delete the App private key and wait no more than one hour for the installation token to expire.

## Billing evidence

GitHub states that Copilot usage is attributed to the account that owns the App installation. The required `billing_owner_contract` check proves that the installation belongs to `COPILOT_CANARY_EXPECTED_OWNER` and records this documented contract.

The public [AI-credit usage endpoint](https://docs.github.com/en/rest/billing/usage#get-billing-ai-credit-usage-report-for-an-organization) is an organization and day aggregate. It does not return an installation ID or Copilot request ID. It can lag a live request. Therefore, it cannot prove that one aggregate row came from this canary when other organization traffic exists.

To capture the available aggregate before and after the canary, supply a separate GitHub token for an organization administrator. The canary does not persist or print this token:

```bash
COPILOT_CANARY_BILLING_TOKEN="$(gh auth token)" \
COPILOT_CANARY_BILLING_WAIT_SECONDS=300 \
mise run copilot-installation-canary >copilot-canary-report.json
```

Each GitHub and Copilot HTTP request has a 60-second deadline. Set `COPILOT_CANARY_REQUEST_TIMEOUT_MS` to a value from `1` through `600000` only when the target installation needs a different per-request deadline. The same deadline applies to token revocation.

The billing baseline and after snapshot must use the same UTC day. If the run crosses UTC midnight, the canary marks the comparison `UNAVAILABLE` and asks you to run it again. It does not compare two different daily aggregates.

`billing_usage_delta` is required. It is `PASS` only when the daily aggregate increases during the observation window. It is `UNAVAILABLE` when the endpoint cannot be read or no increase is visible, which keeps the final result `INCOMPLETE`. A visible increase supports the billing check, but it is not request-level proof when the organization has concurrent Copilot traffic.

GitHub's Copilot audit log does not include local prompt session data. Do not use the absence of an audit event as evidence that the request was not billed.

## Failure triage

Use the first failed check to select the next action:

| Failed check | Action |
| --- | --- |
| `installation_owner_and_scope` | Confirm the organization, **All repositories** selection, and App permission. Mint new tokens after permission changes. |
| `initial_installation_token` | Confirm the App ID, installation ID, repository ID, private key, and `copilot_requests: write` permission. |
| `models` | Confirm that the organization allows GitHub App Copilot requests. Treat `401` and `403` as a failed direct-token contract until corrected. |
| `chat_model_contract` or `messages_model_contract` | Select model IDs from the current `/models` inventory and its `supported_endpoints` values. |
| request or stream checks | Preserve the HTTP status and sanitized evidence. Recheck the tested header profile before changing request translation. |
| `token_refresh` | Do not rely on the provider's cached-token refresh until a second token succeeds. |
| `billing_owner_contract` | Stop. The App is installed on the wrong account or the installation could not be verified. |
| `token_cleanup` | Delete the App private key and allow the new tokens to expire before another test. |

After a successful canary, record the report timestamp, repository ID, organization, model IDs, header-profile versions, and commit SHA in the rollout decision. Keep the report only in an approved operational evidence store.
