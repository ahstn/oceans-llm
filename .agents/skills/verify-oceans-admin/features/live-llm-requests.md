# Live LLM requests

Live LLM requests prove that a changed request path reaches a real upstream provider and returns through the gateway. They are paid integration checks, not part of the read-only admin baseline.

## Sub-features

- `live-openrouter-generic` sends one short Chat Completions request through the preferred generic OpenRouter route.
- `live-bedrock-mantle` sends one short Responses request through Bedrock Mantle for Bedrock-specific changes.
- `live-request-log` confirms the selected provider, route, upstream model, status, and usage in the admin request log.
- `live-key-cleanup` revokes the temporary gateway key used by the canary.

## How to get to it (user POV)

- Sign in, choose `API Keys`, and create a unique temporary key with access only to the selected canary model.
- For a generic request-path change, call `/v1/chat/completions` with gateway model `deepseek-v4-flash-0731`.
- For a Bedrock-specific change, call `/v1/responses` with gateway model `gpt-oss-120b-bedrock`.
- Choose `Request Logs` and open the matching request after the call completes.

## Driving it with control-oceans-admin

Preconditions:

- `control-oceans-admin doctor` passes.
- The task affects behavior that needs real upstream proof. UI-only, documentation-only, seed-only, and unrelated configuration changes do not qualify.
- `OPENROUTER_API_KEY` is available for the generic canary, or the AWS default credential chain can call Bedrock Mantle for a Bedrock-specific canary.
- The operator accepts one short paid request. Use synthetic input and do not include repository content, user data, or secrets.
- Create a unique temporary gateway API key through the API Keys recipe. Export it only in the current shell as `OCEANS_VERIFY_API_KEY`. Do not write it to evidence or command history.

- **Select one provider.** Prefer OpenRouter unless the change is Bedrock-specific. Do not call both providers only to increase test count.
- **OpenRouter request.** Send one non-streaming request with `model: deepseek-v4-flash-0731`, a short synthetic prompt, and `max_tokens` no greater than 32. Expect a successful Chat Completions response.
- **Bedrock request.** Send one non-streaming request with `model: gpt-oss-120b-bedrock`, a short synthetic input, `max_output_tokens` no greater than 64, and `store: false`. Expect a successful Responses response.
- **Changed behavior.** Add only the smallest option needed to exercise the changed path, such as `stream: true` for streaming work or one deterministic function tool for tool translation. Do not broaden the prompt.
- **Request-log proof.** In `Request Logs`, locate the canary by time and gateway model. Confirm success, the expected provider and upstream model, and non-zero usage when the provider supplies it. Capture the list and detail states with raw payloads and credentials redacted. Write a sanitized `openrouter-canary-proof.json` or `bedrock-canary-proof.json` in the run evidence directory. Include the run ID, gateway model, provider, upstream model, status, usage, and request-log ID. Do not include prompts, responses, or credentials.
- **Cleanup.** Revoke the temporary gateway API key and confirm it can no longer call `/v1/models`. Then run the normal stack cleanup and `control-oceans-admin evidence live-llm`.

## Gotchas

- A configured model, present credential, healthy gateway, or generated client configuration does not prove a live provider call.
- OpenRouter is the preferred low-cost generic path. Its configured `deepseek/deepseek-v4-flash-0731` rate is $0.03 input and $0.10 output per million tokens, but OpenRouter can route among upstream hosts.
- Bedrock Mantle uses model ID `openai.gpt-oss-120b` and the `/v1/responses` path. The configured standard rate in `us-east-1` is $0.15 input and $0.60 output per million tokens. The Bedrock Runtime model ID is different: `openai.gpt-oss-120b-1:0`.
- Bedrock stored responses default to retention. Keep `store: false` unless storage behavior is the test target.
- A provider outage, missing model access, expired credential, quota, or network block is an integration failure with a specific cause. It does not invalidate separate local UI proof.
- Never record raw API keys, authorization headers, full environment dumps, or provider response content that can contain submitted data.
