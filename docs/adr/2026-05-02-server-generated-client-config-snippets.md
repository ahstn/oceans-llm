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
- `AdminModelsService` builds snippets for routed models with enough route/provider metadata.
- The admin model list payload includes one-model `client_configurations` for row actions, and `POST /api/v1/admin/models/client-configs` renders a selected model set.
- The crate centralizes Anthropic thinking policy for this feature: safe-effort variants are emitted only for Claude 4.6/4.7/Mythos-style families, while manual-budget Claude models remain reasoning-capable without generated variants.

## Why

Server-side generation keeps pricing, model ids, token limits, route/provider labels, and thinking policy close to the source of truth. It also keeps the UI focused on presentation and copy behavior instead of embedding provider-specific config semantics in React code.

A separate crate gives this feature a clear ownership boundary. Future clients can add another `ClientConfigTemplate` implementation without expanding the admin service with more JSON construction details.

## Trade-offs

The server now ships presentation-oriented JSON snippets. That is intentional for this slice, but the templates should stay limited to end-user client configuration and should not become a second provider request-mapping layer.

Anthropic thinking policy currently relies on conservative model-name inference because the catalog does not yet expose a first-class typed thinking policy flag. This should move to explicit metadata when the provider/model policy surface is stable.

The list payload becomes larger for supported rows because one-model snippets are embedded. Multi-model snippets use the selected-model endpoint so the browser does not merge JSON strings and the list payload remains bounded by the visible page size.

## 2026-06-11 Update: Claude Code Multi-Block Snippets

Claude Code is now implemented as another `ClientConfigTemplate`. Client configurations now contain one or more code blocks so a single client tab can expose related files or alternate settings without the UI understanding client-specific semantics. OpenCode and Pi still emit one block. Claude Code emits a gateway `settings.json` block and a separate lower-token-usage `settings.json` block.

Claude Code uses an Anthropic-compatible gateway base URL and appends endpoints such as `/v1/messages` and `/v1/models`. The template derives that base from the OpenAI-compatible `/v1` gateway URL used by OpenCode and Pi so copied Claude Code settings do not point at the OpenAI client base.

## 2026-06-27 Update: Multi-Model Snippet Generation

Client configuration rendering now accepts a selected model set. OpenCode and Pi group selected models by client API style before rendering provider entries, because both clients keep the adapter at provider scope. Mixed Anthropic Messages and OpenAI-compatible selections therefore generate separate Oceans provider ids instead of one invalid provider.

Claude Code rendering filters the selected set to Anthropic Messages models and omits the Claude Code tab when none are selected. This keeps non-Anthropic gateway models available for OpenCode and Pi without producing invalid Claude Code `modelOverrides`.

The admin UI calls `POST /api/v1/admin/models/client-configs` for selected-model generation. This keeps provider grouping, Claude filtering, pricing metadata, and thinking-policy rendering in Rust instead of duplicating those rules in React.

## 2026-06-27 Update: Public Gateway Base URL

Generated snippets use `GATEWAY_CLIENT_CONFIG_BASE_URL` when the gateway process sets it, falling back to the local development URL otherwise. Helm exposes this through `gateway.clientConfigGatewayBaseUrl` and renders it as a gateway pod environment variable.

The browser origin is deliberately not used as the source of truth. Admin UI routing can differ from the public API origin local harnesses should call, especially behind ingress, path rewriting, or a split admin/API deployment.

## Follow-ups

- Replace Anthropic model-name inference with typed provider/model metadata when available.
- Consider validating OpenCode output against a pinned OpenCode config schema.
- Consider validating Claude Code output against the SchemaStore `claude-code-settings.json` schema.
- Add more client templates only through the `ClientConfigTemplate` boundary.
- Revisit whether deployment-specific API key placeholders should be configurable instead of fixed placeholders.
