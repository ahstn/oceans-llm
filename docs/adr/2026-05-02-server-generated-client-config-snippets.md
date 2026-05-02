# ADR: Server-Generated Client Config Snippets

- Date: 2026-05-02
- Status: Accepted

## Context

The admin Models page exposes the resolved model catalog, selected route metadata, pricing, token limits, and model capabilities that end users need when configuring local coding agents against the gateway's OpenAI-compatible endpoint.

OpenCode and Pi both support custom provider/model configuration, but their file shapes and thinking controls differ. Anthropic models add another layer of policy: newer Claude safe-effort families can use effort-style thinking values, while older Claude models require caller-supplied manual budget tokens. Generating snippets directly in the browser would duplicate this policy in TypeScript and make it harder to keep client instructions aligned with gateway routing behavior.

## Decision

Generate local client configuration snippets on the server from the same admin model summary data used by the Models page.

Implementation points:

- `crates/gateway-client-config` owns the config templating boundary.
- `ClientConfigTemplate` is the shared interface for client-specific renderers.
- OpenCode and Pi are implemented as separate templates with their own JSON shapes.
- `AdminModelsService` builds snippets only for Anthropic-labeled rows.
- The admin model list payload includes `client_configurations`, so the UI does not need a second endpoint.
- The crate centralizes Anthropic thinking policy for this feature: safe-effort variants are emitted only for Claude 4.6/4.7/Mythos-style families, while manual-budget Claude models remain reasoning-capable without generated variants.

## Why

Server-side generation keeps pricing, model ids, token limits, route/provider labels, and thinking policy close to the source of truth. It also keeps the UI focused on presentation and copy behavior instead of embedding provider-specific config semantics in React code.

A separate crate gives this feature a clear ownership boundary. Future clients can add another `ClientConfigTemplate` implementation without expanding the admin service with more JSON construction details.

## Trade-offs

The server now ships presentation-oriented JSON snippets. That is intentional for this slice, but the templates should stay limited to end-user client configuration and should not become a second provider request-mapping layer.

Anthropic thinking policy currently relies on conservative model-name inference because the catalog does not yet expose a first-class typed thinking policy flag. This is acceptable for the initial Anthropic-only slice, but it should move to explicit metadata when the provider/model policy surface is stable.

The list payload becomes larger for supported rows because snippets are embedded. Each row includes only the clicked model's configurations, so the payload remains bounded by the visible page size and avoids another request path.

## Follow-ups

- Replace Anthropic model-name inference with typed provider/model metadata when available.
- Consider validating OpenCode output against a pinned OpenCode config schema.
- Add more client templates only through the `ClientConfigTemplate` boundary.
- Revisit whether deployment-specific base URL/API key defaults should be configurable instead of fixed placeholders.
