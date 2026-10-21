# Anthropic Messages

Clients use `POST /v1/messages` (or `/messages`) with Anthropic messages, tools, thinking and effort controls. Oceans authenticates and routes the request, returns a native Message or SSE stream, and records the request and provider attempt. This recipe combines deterministic native-adapter regressions with one bounded request through the real public gateway; neither layer substitutes for the other.

## Sub-features

- `messages-http` covers authentication, the Messages error envelope, model access, routing and the unversioned alias.
- `messages-json` covers native message content, stop reasons, tool-use IDs and input/output usage.
- `messages-stream` covers ordered SSE, content/signature deltas, completion, errors and final usage.
- `messages-thinking` covers adaptive display, manual thinking budgets, supported disabled/between-tools modes and field-specific validation errors.
- `messages-effort` covers `low`, `medium`, `high`, `xhigh`, `max`, omitted effort and null effort without inventing a default.
- `messages-continuation` covers tools, signed or hidden thinking blocks, beta headers, cache boundaries and per-message effort.
- `messages-auto-mode` preserves opaque safeguards/verdicts; it does not certify a complete Claude Code session.
- `messages-observability` correlates the response request ID with the native operation, provider attempt, usage and scoped-key cleanup.

## How to get to it (user POV)

- Sign in to `/admin`, choose `API Keys`, and create a temporary user key with `Selected models` and one model grant. Under `Capabilities`, choose `Limited` and select only `Inference`.
- Send synthetic JSON to `/v1/messages` with `x-api-key`, `anthropic-version: 2023-06-01`, and `Content-Type: application/json`.
- Choose `Request Logs`, filter by the response's `x-request-id`, and choose `Inspect`.

## Driving it with control-oceans-admin

Preconditions:

- Run `control-oceans-admin plan` first. Use an explicit base ref when the checkout has no `origin/main` or `main`.
- Read the [native contract](../../../../docs/reference/provider-api-compatibility.md#native-messages-thinking-controls) and [research matrix](../../../../docs/internal/anthropic-thinking-compatibility.md) when model behavior changes. Model capabilities and effort ceilings must match the selected scenario.
- Run only the isolated stack owned by this run; `control-oceans-admin doctor` must pass before browser work and after any failed drive.
- The [live LLM policy](./live-llm-requests.md) authorizes one bounded canary for request-path verification when credentials are available. Do not run the effort matrix against paid providers. Tool loops and additional native scenarios have their own explicit call budgets.

1. **Run the offline gate from the final source.** This runs the core, provider and gateway Rust library suites, including separate API thinking contracts, native request matrices, local provider HTTP captures, tool/metadata roundtrips, handler accounting and stream reconstruction. It fails if any named critical regression disappears or is ignored, even when Cargo exits successfully:

   ```bash
   mise exec -- node .agents/skills/verify-oceans-admin/scripts/check-messages.mjs \
     --output "/tmp/oceans-admin-verification/$OCEANS_VERIFY_RUN_ID/evidence/messages-regressions.json"
   ```

   Require `verdict: pass` and all checks passing. Run after the last source edit; do not reuse an artifact from another checkout or source hash. The helper requires no stack or credentials and sends no upstream calls. Its local provider HTTP fixtures do not pass through the public gateway listener.

2. **Review the regression matrix.** The gate must cover the following contracts. Extend the Rust tests when changing any of them; do not weaken a gate check to make a refactor pass.

   | Contract | Required assertions |
   |---|---|
   | Adaptive controls | Shared Anthropic: both stream modes, all five efforts and `summarized`/`omitted`/null display; Vertex HTTP: all five efforts with `summarized`/`omitted`; Bedrock Runtime/Mantle: representative high/xhigh display parity in both mapping modes |
   | Default preservation | Neither thinking nor effort, adaptive thinking without effort, empty output config and `effort: null` stay unchanged; native effort alone does not enable thinking |
   | API separation | The HTTP handler selects a trusted protocol that request JSON cannot override; native Messages preserves omission while Anthropic-compatible/Vertex Chat Completions retains adaptive/high defaults and Bedrock Chat retains effort-triggered thinking |
   | Thinking modes | Manual `enabled` budgets remain valid on older models; current model-specific disabled/between-tools restrictions remain intact; OpenAI aliases retain their separate translation |
   | Negative controls | Invalid display identifies display; real adaptive budgets identify budget; conflicting budget/effort aliases fail; invalid effort values are not silently converted |
   | Native continuity | Empty signed thinking, signatures, tool-use IDs/results, cache boundaries, beta extensions, in-sequence system effort messages and safeguard results survive mapping |
   | Beta transport | Repeated caller beta headers merge with configured/body opt-ins without duplicates; direct Anthropic, Vertex and Mantle use a header, Runtime uses `anthropic_beta`; Messages and token counting preserve opt-ins without forwarding caller credentials; oversized beta lists and names fail locally before dispatch, while allowed unknown names retain their order |
   | Streaming | Assistant role appears first and once, metadata does not precede it, usage is retained, tool/content blocks close in order and `message_stop` terminates the response; pending metadata survives DONE/EOF even without a finish reason |
   | Gateway behavior | Authentication/model grants, response/error shape, accounting, exhausted-budget rejection before provider dispatch and request logging |

3. **Choose one live scenario and record its scope.** Use this decision table; never label the OpenRouter result a native-validator or Vertex proof.

   | Change | Route and smallest live request |
   |---|---|
   | Public Messages/streaming or omitted-effort behavior | `claude-haiku-5.5` → OpenRouter, omit both `thinking` and `output_config`, `max_tokens: 1024`; add `stream: true` for streaming work |
   | Native shared thinking/display validator | Existing `claude-opus-4-7` → `vertex-claude` when Vertex credentials work; adaptive + summarized display + `output_config.effort: low`, `max_tokens: 1024` |
   | Bedrock Messages adapter | Existing `claude-sonnet-4.6` → native Bedrock Runtime; use an allowed owner such as Ben/Diego and `stream: false`; this route does not support streaming |

   Confirm the current provider/upstream model and capability from the production model catalog before calling. A configured route is not a credential check. No seeded Opus 5.5 route exists: the local HTTP fixtures cover its wire contract. Do not change shared configuration to pretend another model is Opus 5.5. If the required native credential or model access is unavailable, record that native-live check as blocked; a successful translated smoke can still be reported separately.

4. **Create and constrain the key.** Follow [API keys](./api-keys.md), using a unique run-specific name and an owner eligible for the selected model. Capture the form before creation. Read the one-time key only into memory and choose `Dismiss` before screenshots. Confirm the saved grant is exactly one model and the saved scope is `inference`; `/v1/models` must expose only the selected grant.

5. **Exercise the public edge without provider spend.** A request with no key must return HTTP 401 in the Anthropic error shape. With the temporary key, malformed JSON must return HTTP 400. The current Axum JSON extractor returns plain text for malformed JSON before the Messages handler runs; record that envelope compatibility gap separately, not as a passing Anthropic error check. Repeat those free checks on `/messages` when HTTP routing changes. Check that these rejected requests do not produce a successful provider attempt. Do not send another valid request merely to exercise the alias.

6. **Send the single canary.** Use the selected scenario, a synthetic prompt such as `Reply OK`, a bounded timeout and `max_tokens <= 4096`. Record the response request ID. For JSON require HTTP 200, `type: message`, assistant content, a completed stop reason and input/output usage. For SSE require HTTP 200, `text/event-stream`, first `message_start`, content events, final usage/stop reason in `message_delta`, terminal `message_stop`, and no error event. Do not treat a truncated output as completion. Do not require visible reasoning when display is omitted, or assume every adaptive response contains thinking.

7. **Verify the log in the browser and API.** Open `/admin/observability/request-logs?request_id=<id>`, wait for `Showing 1 of 1`, capture the list, and choose `Inspect`. Require the selected model/provider, stream flag, HTTP status and usage. Compare with `GET /api/v1/admin/observability/request-logs?request_id=…` and the detail API. Compare the UI operation with `log.metadata.operation`: the current public Messages path records `chat_completions` (`Chat Completions`), so record the missing native operation label with `--optional native-operation-label=fail` until corrected. The successful attempt must name the intended provider and upstream model. Mask `request-log-payloads` in screenshots and omit it from ARIA/text snapshots; retain only route, metrics, facts and attempts. Do not save complete admin responses.

8. **Clean up even after failure.** Revoke the temporary key in a `finally` path and verify `/v1/models` returns 401. If key creation succeeded but ID extraction failed, recover its ID by the unique name before revoking. Do not retry a timed-out paid request automatically; inspect its request log first.

9. **Record layered evidence.** `evidence --require-current` validates the attached gate verdict and a fingerprint that includes tests and suite dependencies; a failed, missing or stale gate cannot satisfy the proof. Record `anthropic-messages` with separate checks for the offline gate, free HTTP/error controls, the selected live scenario, stream/JSON response, request-log parity, and revoked key. Attach `messages-regressions.json` plus the sanitized create/list/detail captures. Record the same single canary under `live-llm-requests`; do not send a second canary merely because both features were planned. Name the transport and any unverified native path explicitly.

   Keep the offline gate, HTTP status/authentication checks, selected live response, log parity and cleanup as required `--check` results. When the two known product gaps above are observed, add these flags to the `record anthropic-messages` command:

   ```bash
   --optional 'malformed-json-envelope=fail:Axum returns plain-text HTTP 400'
   --optional 'native-operation-label=fail:Messages logs chat_completions'
   ```

   These optional failures remain visible in the report without failing the required proof. Reserve `blocked` for an unmet prerequisite, such as missing native-provider credentials. If the current task fixes either product gap, make that check required and require it to pass; do not use optional status to excuse a regression in the behavior under test. Then run:

   ```bash
   .agents/skills/verify-oceans-admin/scripts/control-oceans-admin evidence \
     anthropic-messages live-llm-requests request-logs --require-current
   .agents/skills/verify-oceans-admin/scripts/control-oceans-admin report --plan
   .agents/skills/verify-oceans-admin/scripts/control-oceans-admin cleanup
   ```

   Record before cleanup; confirm the evidence still passes afterwards. Use the request-log recipe when that separate feature is planned.

## Gotchas

- Auto mode is a tool-permission workflow, not an effort enum. Omitted effort is valid; neither `auto` nor `none` is a native effort value. Omission does not mean thinking is disabled.
- Native Messages defaults belong to the upstream model. Do not inject `high` into Messages to make a test deterministic; Chat Completions has a separate translation contract and must retain its documented defaults.
- `display: omitted` is not a workaround for a validator rejecting the display key itself. It still sends the key. Beta `updates` and binding fields need upstream beta validation.
- OpenRouter translates Messages to Chat Completions. Its canary proves the public surface, while the offline gate tests native Anthropic/Vertex/Bedrock mapping. Neither establishes a complete Claude Code auto-mode session.
- Unknown SSE events and arbitrary mid-conversation system messages have documented forwarding limitations. Keep those gaps visible; do not assert universal protocol transparency.
- Malformed JSON currently receives a plain-text HTTP 400 from the shared framework extractor. Authentication errors use the Anthropic JSON envelope. Record this known product gap with `--optional malformed-json-envelope=fail` until parsing errors are normalized, then require it to pass.
- The public Messages handler currently uses a Chat Completions request-log context. A matching request ID, payload protocol or SSE response proves the caller used Messages; the operation label alone does not. Keep the native-operation-label gap visible until it is fixed.
- Tool loops and token counting have separate recipes in [live LLM requests](./live-llm-requests.md). Count requests do not create Request Logs rows. Keep signed assistant blocks unchanged in a continuation.
- A recipe is a maintained regression gate, not a guarantee against all future regressions. A missing named test requires an equivalent replacement and review of the gate mapping, never a silent skip.
