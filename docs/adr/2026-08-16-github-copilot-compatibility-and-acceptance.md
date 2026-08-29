# GitHub Copilot Compatibility Evidence and Production Acceptance

## Status

Accepted for implementation. Production use of GitHub App authentication requires the live installation-token canary in [GitHub Copilot Installation-Token Canary](../operations/github-copilot-installation-canary.md).

This decision supersedes the model-family routing, fixed request-header profile, and direct-token production-acceptance parts of [GitHub Copilot Gateway Provider Architecture](2026-08-14-github-copilot-provider.md).

> **Registration availability disclaimer (2026-08-29):** GitHub's current documentation and App registration surfaces are inconsistent. The Copilot SDK describes `copilot_requests` as a repository permission, GitHub's generic permission list classifies it as an account permission, and the standard installation-token REST schema does not list it. Current personal-account and organization-account registration forms also do not expose either form of the permission. This can indicate a staged rollout, private enablement, or a documentation defect. Until a target account can register the exact `copilot_requests: write` permission and pass the live canary, treat GitHub App authentication as unavailable. Do not substitute the unrelated **Copilot agent settings** permission.

## Context

Copilot model names do not prove which HTTP endpoints or request features an upstream model supports. GitHub can also change its editor compatibility profile independently of the gateway. Direct GitHub App installation-token use has documented support, but organization ownership and billing evidence still need a target-specific live check.

## Decision

Each GitHub Copilot route uses explicit compatibility metadata derived from a current `/models` response for the exact upstream model:

- `chat_api` selects `chat_completions` or `anthropic_messages`. The provider fails closed for chat when this field is absent.
- `supports_responses` and `supports_embeddings` enable only their named endpoints.
- `upstream_supports` records streaming, tool-call, vision, and structured-output evidence. Missing evidence is `false`.
- route `capabilities` remain admin policy and are intersected with the upstream evidence.

Structured-output eligibility is operation-specific. An Anthropic Messages chat endpoint does not support the OpenAI JSON-schema request contract. A Responses endpoint on the same route can use structured outputs when the route has explicit Responses and structured-output evidence.

Every inference request uses the named `vscode_chat_2026_06_01` compatibility profile:

- `OpenAI-Intent: conversation-agent`
- `X-Interaction-Type: conversation-agent`
- `X-Initiator: user` for a user turn
- `X-Initiator: agent` for an assistant or tool-result continuation
- `X-GitHub-Api-Version: 2026-06-01`

The profile values are stored in one checked-in compatibility contract that is tested against both the Rust provider and the live-canary client.

GitHub App mode requests repository-scoped installation tokens with `copilot_requests: write`. The direct installation-token contract remains a production gate until the live canary passes for the target organization. GitHub documents attribution to the App installation owner. The public billing API supplies an organization and day aggregate, not request-level or installation-level attribution.

The canonical provider and route syntax is in [Configuration Reference](../configuration/configuration-reference.md#github-copilot-provider-and-route-evidence).

## Trade-Offs

- Explicit evidence adds configuration work but prevents model-name guesses from enabling unsupported endpoints.
- A shared route can expose more than one API family, so some feature checks must account for the requested operation.
- A checked-in compatibility profile is deterministic, but maintainers must update and canary-test it when GitHub changes the upstream contract.
- Aggregate billing evidence can support rollout acceptance but cannot identify one canary request when concurrent organization traffic exists.

## Follow-Ups

- Consider dynamic `/models` discovery in a later decision. Until then, admins must derive route evidence from a current canary result.
- Support multi-tenant GitHub App installation resolution per request if dynamic credential routing is required.
