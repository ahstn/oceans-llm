# Live LLM requests

Live LLM requests prove that a changed request path reaches a real upstream provider and returns through the gateway. They are paid integration checks, not part of the read-only admin baseline.

## Sub-features

- `live-openrouter-generic` sends one short Chat Completions request through the preferred generic OpenRouter route.
- `live-anthropic-messages` sends one short Anthropic Messages request to `/v1/messages` for Messages-format changes.
- `live-bedrock-mantle` sends one short Responses request through Bedrock Mantle for Bedrock-specific changes.
- `live-decisions` sends one OpenAI-format request to `/v1/decisions` and one System One request to `/v1/systemone` through the `jev` alpha route for Decisions changes.
- `live-count-tokens` counts a short Messages prompt through `/v1/messages/count_tokens` and confirms that a route without an upstream count API returns `501 not_implemented_error`.
- `live-request-log` confirms the selected provider, route, upstream model, status, and usage in the admin request log.
- `live-key-cleanup` revokes the temporary gateway key used by the canary.

## How to get to it (user POV)

- Sign in, choose `API Keys`, and create a unique temporary key with access only to the selected canary model.
- For a generic request-path change, call `/v1/chat/completions` with gateway model `deepseek-v4.1-flash`.
- For an Anthropic Messages change, call `/v1/messages` with `x-api-key`, `anthropic-version: 2023-06-01` and gateway model `claude-haiku-5.5`.
- For a Bedrock-specific change, call `/v1/responses` with gateway model `gpt-oss-120b-bedrock`.
- For a Decisions change, call `/v1/decisions` (OpenAI format, `input`) and `/v1/systemone` (System One format, `state`) with gateway model `jev`.
- For a token-counting change, call `/v1/messages/count_tokens` or `/messages/count_tokens` with `x-api-key` and gateway model `claude-sonnet-4.6` (Bedrock Runtime Anthropic invoke). Call it again with `deepseek-v4.1-flash` to see the 501 path.
- For every canary except a token count, choose `Request Logs` and open the matching request after the call completes. A token count creates no request log, so skip this step for it.

## Driving it with control-oceans-admin

Preconditions:

- `control-oceans-admin doctor` passes.
- The task affects behavior that needs real upstream proof. UI-only, documentation-only, seed-only, and unrelated configuration changes do not qualify.
- `OPENROUTER_API_KEY` is available for the generic and Messages canaries, or the AWS default credential chain can call Bedrock Mantle for a Bedrock-specific canary. A Decisions canary needs a working credential for the `jev` alpha route.
- The operator accepts one short paid request. A Decisions canary makes two paid calls, one per format, and the operator must accept both. Its mixed-format 400 check is free. Use synthetic input and do not include repository content, user data, or secrets.
- Create a unique temporary gateway API key through the API Keys recipe. Export it only in the current shell as `OCEANS_VERIFY_API_KEY`. Do not write it to evidence or command history.

- **Select one provider.** Prefer OpenRouter unless the change is Bedrock-specific. Do not call both providers only to increase test count.
- **Token cap.** Every canary request sets `max_tokens` (or `max_output_tokens`) to 4096 or less. Use the smallest value that lets the model finish: reasoning models spend part of the budget before any output, and a truncated answer or empty tool arguments prove nothing.
- **OpenRouter request.** Send one non-streaming request with `model: deepseek-v4.1-flash`, a short synthetic prompt, and `max_tokens` no greater than 4096. Expect a successful Chat Completions response.
- **Anthropic Messages request.** For Messages-format changes only, send one request to `/v1/messages` with `model: claude-haiku-5.5`, a short synthetic prompt, and `max_tokens` no greater than 4096. Expect a `message` response with `usage.input_tokens` and `usage.output_tokens`. Add `stream: true` for Messages streaming work; the stream should open with `message_start` and end with `message_stop`.
- **Bedrock request.** Send one non-streaming request with `model: gpt-oss-120b-bedrock`, a short synthetic input, `max_output_tokens` no greater than 4096, and `store: false`. Expect a successful Responses response.
- **Decisions requests.** For Decisions changes only, send one OpenAI-format body to `/v1/decisions`. Use `model: jev`, a short synthetic `input`, and one named `predicate` question. Expect `answers[0].type == "predicate"` and integer `usage.input_tokens_details.cached_tokens`. Then send one System One body to `/v1/systemone` with a synthetic `state` and one `noul` question. Expect a map of `answers` and `usage.input_tokens`. Also send one body that contains both `input` and `state` to `/v1/decisions`, and confirm it returns 400 without a request log or provider attempt. This last call is free.
- **Changed behavior.** Add only the smallest option needed to exercise the changed path, such as `stream: true` for streaming work or one deterministic function tool for tool translation. Do not broaden the prompt.
- **Token count request.** Send one Messages body with a short synthetic prompt to `/v1/messages/count_tokens` using model `claude-sonnet-4.6`. Expect HTTP 200 and `{"input_tokens": N}` with `N` greater than zero. Send the same body with model `deepseek-v4.1-flash`. Expect HTTP 501, `error.type: not_implemented_error`, and a message naming the route's provider key and upstream model. Neither call adds a Request Logs row. Record one `--check` each for the count, the 501 envelope, and the unchanged request-log list.
- **Request-log proof.** For canaries that create a request log, which excludes token counts, locate the canary in `Request Logs` by time and gateway model. Confirm success, the expected provider and configured upstream model, and non-zero usage when the provider supplies it. Capture the list and detail states with bounded, sanitized payload evidence and credentials redacted. Record it with `control-oceans-admin record live-llm-requests --file openrouter-canary-proof.json` (or `bedrock-canary-proof.json`). Use one `--check` each for the response status, provider and upstream model, usage, and request-log match, with the gateway model and request-log ID in the notes, plus the captured list and detail screenshots as `--artifact`. Do not include prompts, responses, or credentials.
- **Cleanup.** Revoke the temporary gateway API key and confirm it can no longer call `/v1/models`. Then run the normal stack cleanup and `control-oceans-admin evidence live-llm`.

## Gotchas

- A configured model, present credential, healthy gateway, or generated client configuration does not prove a live provider call.
- OpenRouter is the preferred low-cost generic path. `deepseek/deepseek-v4.1-flash` is priced from the pricing catalog (about $0.016 input and $0.89 output per million tokens when this was written), but OpenRouter can route among upstream hosts. The older `deepseek-v4-flash-0731` route stays configured; use it only to compare against an earlier run.
- `claude-haiku-5.5` reaches `anthropic/claude-haiku-5.5` through OpenRouter, priced from the catalog (about $0.10 input and $0.50 output per million tokens). The gateway translates the Messages request to Chat Completions for this route, so it proves the Messages surface and its stream adapter, not a native Anthropic upstream. Use `claude-sonnet-4.6` (Bedrock Runtime Anthropic invoke) when the native upstream path is under test.
- Bedrock Mantle uses model ID `openai.gpt-oss-120b` and the `/v1/responses` path. The configured standard rate in `us-east-1` is $0.15 input and $0.60 output per million tokens. The Bedrock Runtime route (`gpt-oss-120b`, `runtime_openai_chat`, upstream `openai.gpt-oss-120b-1:0`) is a separate Chat Completions path that does not accept `/v1/responses`. Do not use it for Mantle proof. Use it with `/v1/chat/completions` when the Bedrock Runtime OpenAI-chat path is under test. Bedrock Runtime rejects the short ID `gpt-oss-120b` with "The provided model identifier is invalid".
- `/v1/decisions` and `/v1/systemone` (model `jev`, upstream `typesafe/jev-1.13` on the alpha route) are a separate paid path. The route speaks System One, so an OpenAI-format call proves translation in both directions. Request logs show operation `Decisions` and store the caller-format response. Decisions skips inference guardrails. Chat and Responses canaries do not prove it.
- No local route speaks the OpenAI Decisions format upstream (`compatibility.openai_compat.decisions_api: openai`). The provider round-trip tests cover that path. Do not add an OpenAI route only for a canary.
- Token counting is never logged as a request. Prove it from the HTTP response and the `anthropic count_tokens request completed` gateway log event, not from Request Logs or usage totals.
- The seeded `claude-sonnet-4.6` route uses the cross-Region profile `us.anthropic.claude-sonnet-4-6`, so its count is sent to `anthropic.claude-sonnet-4-6`. A Bedrock `ValidationException` on a count is an upstream rejection, not a gateway failure. It has two common causes: the foundation model has no Runtime `CountTokens` support in the provider's Region, or the route uses an ID Bedrock does not publish. Check the model card's token-counting support for that Region before changing the ID; if `/v1/messages` succeeds on the same route, the ID is valid and the model is not countable there.
- Bedrock `CountTokens` accepts foundation model IDs only. The gateway strips a cross-Region profile prefix such as `us.`, including inside a system-defined `inference-profile/` ARN; an application inference profile ARN is rejected upstream.
- The unversioned aliases (`/chat/completions`, `/responses`, `/embeddings`, `/decisions`, `/models`) reuse the `/v1` handlers. A canary on an alias proves routing only, not provider behavior.
- Bedrock stored responses default to retention. Keep `store: false` unless storage behavior is the test target.
- A provider outage, missing model access, expired credential, quota, or network block is an integration failure with a specific cause. It does not invalidate separate local UI proof.
- Never record raw API keys, authorization headers, full environment dumps, or provider response content that can contain submitted data.
