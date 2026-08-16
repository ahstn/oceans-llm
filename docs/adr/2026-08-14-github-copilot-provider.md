# GitHub Copilot Gateway Provider Architecture

## Status

Accepted for implementation. Production use of GitHub App authentication requires the live installation-token canary in [GitHub Copilot Installation-Token Canary](../operations/github-copilot-installation-canary.md).

## Context

Organizations use GitHub Copilot for model access, but cannot currently route Copilot requests through `oceans-llm` with centralized policy, observability, spend control, and provider routing.

There are two primary integration models for GitHub Copilot:
1. **Copilot CLI/SDK JSON-RPC Runtime Adapter:** Run the official `github-copilot-sdk` runtime (a CLI process over stdio/TCP) as a child process or sidecar. While this provides agentic workflows, multi-tool loops, and filesystem hooks, it introduces significant complexity for a stateless LLM gateway (process lifecycle, supervisor overhead, connection management, restart storms during token refresh, and protocol translation).
2. **Dedicated Native Rust HTTP Provider (`github_copilot`):** Call the GitHub Copilot API surface (`https://api.githubcopilot.com`) directly via HTTP/SSE. This fits the existing `gateway-providers` architecture (`VertexProvider`, `BedrockProvider`, `OpenAiCompatProvider`), offering high performance, connection pooling, deterministic streaming, and zero subprocess overhead.

Authentication for organization workloads is supported via **GitHub App Installation Authentication**:
- The GitHub App has the repository permission **Copilot Requests: Read & write** (`copilot_requests: write`).
- The App is installed on the organization with **All repositories** access.
- The gateway signs a short-lived RS256 JWT using the App ID and private key, then requests an installation access token (`ghs_...`) scoped to repository IDs and `copilot_requests: write`.
- The provider sends the minted `ghs_` token directly as a `Bearer` token to `https://api.githubcopilot.com` without calling legacy user-token exchange endpoints (`/copilot_internal/v2/token`). This direct installation-token contract remains a production gate until the live canary passes for the target organization.
- Tokens expire after one hour and are rotated automatically before expiry.

## Decision

Add a first-class provider type named `github_copilot` in `gateway-providers` and expose it through gateway declarative configuration.

### Provider Architecture & Transport
1. **Direct HTTP Integration:** The provider executes requests directly against `https://api.githubcopilot.com` (or configurable base URL / Enterprise hosts).
2. **Endpoint Routing by Explicit Compatibility Metadata:**
   - Each route declares `compatibility.github_copilot.chat_api` as `chat_completions` or `anthropic_messages` from tested `/models` metadata.
   - The provider fails closed when chat endpoint metadata is absent. It does not infer an endpoint from a model-name prefix.
   - Responses and embeddings are enabled only when the route metadata declares that support.
   Every request sends:
   - `Authorization: Bearer <token>`
   - `Editor-Version: <configured | default vscode/1.126.0>`
   - `Copilot-Integration-Id: <configured | default vscode-chat>`
   - the named `vscode_chat_2026_06_01` compatibility profile, including `OpenAI-Intent: conversation-agent` and `X-Interaction-Type: conversation-agent`
   - `X-Initiator: user` for a user turn, or `X-Initiator: agent` for an assistant or tool-result continuation
   - `X-GitHub-Api-Version: 2026-06-01`

### Authentication Modes
- `github_app`: Authenticates using `app_id`, `private_key` (PEM string or file path), `installation_id`, and `repository_id`. Generates short-lived JWTs and mints/caches `ghs_` installation tokens with automatic refresh.
- `bearer`: Static bearer token for testing, development, or fixed token environments.

### Usage & Cost Accounting
- GitHub Copilot bills organizations through GitHub AI Credits based on token usage.
- The provider extracts token usage from standard OpenAI and Anthropic response payloads (`prompt_tokens`, `completion_tokens`, `total_tokens`).
- When a `pricing_provider_id` is not configured, routes are marked as unpriced or priced via explicit route-level `pricing_override`.

## Implementation

- `crates/gateway-providers/src/copilot/mod.rs`: Provider client, configuration, headers, and request building.
- `crates/gateway-providers/src/copilot/auth.rs`: GitHub App JWT creation and `GitHubAppInstallationTokenSource`.
- `crates/gateway-providers/src/lib.rs`: Export `CopilotProvider`, `CopilotProviderConfig`, and `CopilotAuthConfig`.
- `crates/gateway/src/config/providers.rs`: Parse `github_copilot` provider configuration in YAML/JSON.
- `crates/gateway/src/config.rs`: Register and validate `github_copilot` in the runtime provider registry.
- `crates/gateway-service/src/pricing_catalog/target.rs`: Define catalog pricing targets for `github_copilot`.

## Trade-Offs

- **Direct HTTP vs. Official SDK:** Direct HTTP avoids subprocess overhead and provides lower latency, but requires maintaining the header identity and endpoint contracts if GitHub updates its API surface.
- **Server-to-Server Attribution:** GitHub documents attribution to the App installation owner. Production enablement still requires the canary to verify the expected organization and observe the available organization billing aggregate. The public billing API does not provide request-level or installation-level attribution.

## Follow-Ups

- Consider dynamic `/models` discovery in a later decision. Until then, operators must derive explicit route metadata from a current canary result.
- Support multi-tenant GitHub App installation resolution per request if dynamic credential routing is required in the future.
