# Live LLM requests

Live LLM requests prove that a changed request path reaches a real upstream provider and returns through the gateway. They are paid integration checks, not part of the read-only admin baseline.

## Sub-features

- `live-openrouter-generic` sends one short Chat Completions request through the preferred generic OpenRouter route.
- `live-bedrock-mantle` sends one short Responses request through Bedrock Mantle for Bedrock-specific changes.
- `live-decisions` sends one OpenAI-format request to `/v1/decisions` and one System One request to `/v1/systemone` through the `jev` alpha route for Decisions changes.
- `live-request-log` confirms the selected provider, route, upstream model, status, and usage in the admin request log.
- `live-key-cleanup` revokes the temporary gateway key used by the canary.

## How to get to it (user POV)

- Sign in, choose `API Keys`, and create a unique temporary key with access only to the selected canary model.
- For a generic request-path change, call `/v1/chat/completions` with gateway model `deepseek-v4-flash-0731`.
- For a Bedrock-specific change, call `/v1/responses` with gateway model `gpt-oss-120b-bedrock`.
- For a Decisions change, call `/v1/decisions` (OpenAI format, `input`) and `/v1/systemone` (System One format, `state`) with gateway model `jev`.
- Choose `Request Logs` and open the matching request after the call completes.

## Driving it with control-oceans-admin

Preconditions:

- `control-oceans-admin doctor` passes.
- The task affects behavior that needs real upstream proof. UI-only, documentation-only, seed-only, and unrelated configuration changes do not qualify.
- `OPENROUTER_API_KEY` is available for the generic canary, or the AWS default credential chain can call Bedrock Mantle for a Bedrock-specific canary.
- The operator accepts one short paid request. Use synthetic input and do not include repository content, user data, or secrets.
- Create a unique temporary gateway API key through the API Keys recipe. Export it only in the current shell as `OCEANS_VERIFY_API_KEY`. Do not write it to evidence or command history.

- **Select one provider.** Prefer OpenRouter unless the change is Bedrock-specific. Do not call both providers only to increase test count.
- **OpenRouter request.** Send one non-streaming request with `model: deepseek-v4-flash-0731`, a short synthetic prompt, and `max_tokens` no greater than 128. Expect a successful Chat Completions response.
- **Bedrock request.** Send one non-streaming request with `model: gpt-oss-120b-bedrock`, a short synthetic input, `max_output_tokens` no greater than 64, and `store: false`. Expect a successful Responses response.
- **Decisions requests.** For Decisions changes only, send one OpenAI-format body to `/v1/decisions`. Use `model: jev`, a short synthetic `input`, and one named `predicate` question. Expect `answers[0].type == "predicate"` and integer `usage.input_tokens_details.cached_tokens`. Then send one System One body to `/v1/systemone` with a synthetic `state` and one `noul` question. Expect a map of `answers` and `usage.input_tokens`. Also send one body that contains both `input` and `state` to `/v1/decisions`, and confirm it returns 400 without a request log or provider attempt. This last call is free.
- **Changed behavior.** Add only the smallest option needed to exercise the changed path, such as `stream: true` for streaming work or one deterministic function tool for tool translation. Do not broaden the prompt.
- **Request-log proof.** In `Request Logs`, locate the canary by time and gateway model. Confirm success, the expected provider and configured upstream model, and non-zero usage when the provider supplies it. Capture the list and detail states with bounded, sanitized payload evidence and credentials redacted. Record it with `control-oceans-admin record live-llm-requests --file openrouter-canary-proof.json` (or `bedrock-canary-proof.json`). Use one `--check` each for the response status, provider and upstream model, usage, and request-log match, with the gateway model and request-log ID in the notes, plus the captured list and detail screenshots as `--artifact`. Do not include prompts, responses, or credentials.
- **Cleanup.** Revoke the temporary gateway API key and confirm it can no longer call `/v1/models`. Then run the normal stack cleanup and `control-oceans-admin evidence live-llm`.

## Gotchas

- A configured model, present credential, healthy gateway, or generated client configuration does not prove a live provider call.
- OpenRouter is the preferred low-cost generic path. Its configured `deepseek/deepseek-v4-flash-0731` rate is $0.03 input and $0.10 output per million tokens, but OpenRouter can route among upstream hosts.
- Bedrock Mantle uses model ID `openai.gpt-oss-120b` and the `/v1/responses` path. The configured standard rate in `us-east-1` is $0.15 input and $0.60 output per million tokens. The Bedrock Runtime route (`gpt-oss-120b`, `runtime_openai_chat`) is a separate path that does not accept `/v1/responses`. Do not use it for Mantle proof.
- `/v1/decisions` and `/v1/systemone` (model `jev`, upstream `typesafe/jev-1.13` on the alpha route) are a separate paid path. The route speaks System One, so an OpenAI-format call proves translation in both directions. Request logs show operation `Decisions` and store the caller-format response. Decisions skips inference guardrails. Chat and Responses canaries do not prove it.
- No local route speaks the OpenAI Decisions format upstream (`compatibility.openai_compat.decisions_api: openai`). The provider round-trip tests cover that path. Do not add an OpenAI route only for a canary.
- Bedrock stored responses default to retention. Keep `store: false` unless storage behavior is the test target.
- A provider outage, missing model access, expired credential, quota, or network block is an integration failure with a specific cause. It does not invalidate separate local UI proof.
- Never record raw API keys, authorization headers, full environment dumps, or provider response content that can contain submitted data.
