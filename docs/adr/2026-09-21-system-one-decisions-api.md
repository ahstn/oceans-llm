# System One Decisions API and TypeSafe Provider

## Status

Accepted.

## Context

TypeSafe Jev is a structured decision model. It evaluates a JSON `state` against named `noul`, `choice`, and `score` questions and returns typed answers with probabilities. It does not generate chat text.

TypeSafe and OpenRouter expose the same core body through different paths:

- TypeSafe: `POST /v1/systemone`
- OpenRouter: `POST /api/alpha/decisions`

Mapping this traffic onto Chat Completions or Responses would hide a distinct protocol, weaken route capability checks, and force clients to unwrap generated text that does not exist.

## Decision

1. Expose `POST /v1/decisions` as a first-class gateway API family.
2. Add typed public and core request contracts for `state` and named `noul`, `choice`, and `score` questions.
3. Add `decisions` to request requirements and route/provider capabilities. It defaults to `false`.
4. Require Decisions routes to disable Chat Completions, Responses, embeddings, and streaming.
5. Add a native `typesafe` provider that sends bearer-authenticated requests to `/v1/systemone`.
6. Reuse the `openai_compat` provider connection for OpenRouter, but require `compatibility.openrouter.api: decisions` and route it to `/api/alpha/decisions`.
7. Keep `/v1/decisions` as the adapter default for future providers that adopt the gateway-owned contract.
8. Validate choice and score bounds at the gateway edge. Validate that each provider response contains every requested answer with a matching type.
9. Skip inference guardrails for Decisions. Current guardrail phases consume chat, model-response text, and generated-tool shapes; treating arbitrary decision state as chat would create misleading policy behavior.
10. Keep the normal authentication, model access, route selection, budget, request-log, provider-attempt, error mapping, and usage-accounting paths.
11. Normalize `input_tokens` and `output_tokens` through the existing prompt/completion accounting fields.
12. Use the refreshed OpenRouter fallback catalog entry for `typesafe/jev-1.13`. Native aliases such as `jev-latest` require an explicit route pricing override when no exact catalog key exists.

## Implementation

- `gateway-core` owns typed Decisions requests, question kinds, translations, request requirements, and the `decisions` capability.
- `gateway-providers/src/decisions.rs` owns shared body construction, headers, URL safety, and response-shape checks.
- `gateway-providers/src/openai_compat/decisions.rs` owns OpenRouter and default compatible routing.
- `gateway-providers/src/typesafe.rs` owns native TypeSafe authentication and `/v1/systemone` transport.
- The gateway handler owns the public `/v1/decisions` lifecycle and explicitly omits guardrail calls.
- Admin model metadata exposes Decisions support. Decisions-only routes do not generate chat-client configuration.

## Why

A separate API family preserves the provider contract and makes routing failures deterministic. Provider-specific paths remain adapter details, so callers use one stable Oceans endpoint. Shared transport helpers avoid duplicate request construction while separate adapters keep authentication and URL ownership clear.

The explicit capability default prevents existing permissive routes from becoming eligible after an upgrade. The decisions-only rule also prevents a single Jev route from appearing to support chat or streaming.

## Trade-offs

- Clients need explicit Decisions integration; existing OpenAI chat clients cannot use Jev without adding this endpoint.
- OpenRouter's upstream path is alpha and can change. The adapter isolates that path and validates response shape so drift fails loudly.
- Guardrails do not inspect Decisions state or answers. A future guardrail design needs typed Decisions phases instead of reusing chat phases.
- Native TypeSafe pricing aliases are not inferred. Admins must set an exact catalog identity or a route pricing override.
- The first implementation executes one provider route and does not add retry or fallback behavior.

## Follow-up

- Add another provider only after its request and response shape matches the gateway contract and its endpoint path is explicit.
- Add Decisions-aware guardrail phases only with defined state, question, answer, and transformation semantics.
- Revisit the OpenRouter alpha path when OpenRouter publishes a stable endpoint.

## Validation

Provider round-trip tests cover authentication, body mapping, endpoint routing, upstream errors, and malformed answers. Config tests cover valid TypeSafe and OpenRouter routes plus conflicting capabilities. Gateway verification covers the Models and Request Logs UI, a bounded live Jev call through OpenRouter, usage accounting, and request-log evidence.
