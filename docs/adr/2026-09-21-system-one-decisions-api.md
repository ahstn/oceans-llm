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

## Amendment (2026-10-14): Dual Request Formats, `/v1/systemone`, and the OpenAI Provider

This amendment supersedes decisions 1, 2, and 7 above. Tracked in ahstn/oceans-llm#468.

OpenAI published a Decisions API with its own request format: an `input` (a string, or user messages with `input_text` and `input_image` parts) and an ordered list of `predicate`, `choice`, and `score` questions. OpenAI owns that specification. The gateway needs to accept it without breaking System One callers, and needs to send it to OpenAI-hosted Decisions models.

### Amended decisions

1. **Supersedes 1.** `POST /v1/decisions` accepts both formats and picks one from the body: `input` means OpenAI, `state` means System One. A body with both keys, or with neither, returns 400 with a message that names the problem. `POST /v1/systemone` accepts System One only and rejects `input` with a message that points to `/v1/decisions`. The response always uses the caller's format. When the caller and the upstream speak the same format, the gateway returns the upstream body unchanged apart from the public model name. System One callers therefore keep the same response, byte for byte.
2. **Supersedes 2.** `gateway-core` has one format-neutral request and response representation (`protocol::decisions`), plus a module per wire format that translates in both directions. Questions keep their caller key and an optional display name. Instructions and descriptions stay JSON values until a format needs text.
7. **Supersedes 7.** A provider route declares the format it speaks. `openai_compat` routes with `compatibility.openai_compat.decisions_api: openai` send the OpenAI format to `{base_url}/decisions`. Native TypeSafe and the alpha route keep System One. Plain `openai_compat` routes without either setting do not support Decisions.

### Translation rules

- **Format-aware route selection:** before weighted selection, the gateway drops routes whose format cannot carry the request. A model with routes in both formats therefore serves image or `safety_identifier` requests from its OpenAI routes, and single-option choices from its System One routes.
- **Rejected before any upstream call (400), when no route can carry the request:**
  - `safety_identifier` is OpenAI-only. Routes that speak System One reject it, whether it arrives in an OpenAI or a System One body.
  - System One providers reject image inputs. Audio and video inputs are out of scope.
- **Positional keys:** OpenAI answers correlate by position, so question names need not be unique. A question without a `name`, or whose `name` repeats an earlier one, gets the key `question_{index}`. If that collides with a named question, the key gets a numeric suffix. OpenAI answers keep the caller's original `name`, including `null`.
- **System One questions sent to OpenAI:**
  - Questions are sorted by key, because a System One questions map has no order.
  - `noul` criteria are added to the predicate instructions as "True when" and "False when" lines.
  - `choice` criteria become typed choices with descriptions.
  - `score` criteria become levels.
  - A single-option choice cannot be sent, because OpenAI requires at least two choices.
- **Score indices:** OpenAI reports `score` as a 0-based index into the levels. The System One `score` field is read the same way.
- **Bounds:** OpenAI requests are limited to 1–200 questions, 2–255 choices, 2–10 score levels and 128 images. Core validation still applies the 1–255 choice bound and 2–10 score-level bound to both formats.
- **Confidence when rendering for OpenAI callers:** System One answers carry no confidence. The renderer uses the reported confidence if present, then the probability of the selected choice (or of the nearest score level), and 1.0 as a last resort.
- **Upstream checks:** OpenAI answers must arrive in request order, with matching names and types. Every answer must include `name`, which may be `null`. A `refusal` is allowed for any question. System One upstreams cannot send a `refusal`; the gateway only renders one for System One callers when an OpenAI upstream refuses. A probability list cannot be longer than the question's options or levels. Shape drift fails as a provider transport error.

### Accounting

The handler prices the upstream's own `usage` object, whatever format the caller receives. OpenAI usage reports `input_tokens_details.cached_tokens`, `cache_write_tokens` and `output_tokens_details.reasoning_tokens`. The existing inclusive cache normalization records cached input separately. Reasoning tokens stay within completion tokens. Request logs record prompt, completion, and total counts from the rendered response, and those counts match in both formats. When an upstream response fails validation, the caller gets 502, but the gateway still records the response's billed `usage`.

OpenAI Decisions models such as `gpt-6-luna` bill input only. Without an exact catalog entry, operators set a route `pricing_override` with `output_usd_per_million_tokens: "0"` and OpenAI's published input and cached-input rates.

### Trade-offs

- Typed request errors now return 400 instead of axum's 422, because the handler parses the body after detecting its format. Both `/v1/decisions` and `/v1/systemone` behave this way.
- Folding `noul` criteria into instructions is lossy in the other direction. An OpenAI response rendered for a System One caller has no criteria to return, and none is needed.
- The 0-based score reading follows OpenAI's specification. If a System One provider reports 1-based scores, scores rendered for OpenAI callers will be off by one level.

### Follow-up

- Add Decisions-aware guardrail phases (still out of scope).
- Revisit audio and video inputs once a provider supports them.
