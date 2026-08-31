# Backend gateway

Backend gateway verification proves that an authenticated OpenAI-compatible request resolves a configured gateway model, reaches its live provider route, applies deterministic guardrails to generated tool calls, and records request, provider-attempt, usage, and guardrail-decision evidence.

## Sub-features

- `gateway-openrouter-route` sends a bounded Chat Completions request through gateway model `deepseek-v4-flash-0731` to OpenRouter model `deepseek/deepseek-v4-flash-0731`.
- `gateway-deterministic-evaluate` confirms that the active default policy evaluates a destructive shell command with the expected built-in pack and rule.
- `gateway-generated-tool-guard` confirms that a live generated function call receives a request-linked `generated_tool_call` decision.
- `gateway-log-evidence` confirms the request log, provider attempt, tool cardinality, status, and usage for the live request.
- `gateway-key-cleanup` revokes the temporary model-scoped key and confirms that gateway authentication rejects it.

## How to get to it (user POV)

- Sign in as the seeded platform administrator and open `API Keys` to create a temporary key with explicit access to `deepseek-v4-flash-0731`.
- Call `/api/v1/guardrails/evaluate` with that key to inspect the active deterministic policy without executing the command.
- Call `/v1/chat/completions` with model `deepseek-v4-flash-0731` and one forced synthetic function tool.
- Open `Request Logs` and `Guardrails` under `Observability` to inspect the request and its generated-tool decision.

## Driving it with control-oceans-admin

Preconditions:

- `control-oceans-admin doctor` passes for the stack owned by this verification run.
- `OPENROUTER_API_KEY` is available through Mise. The harness never reads, prints, or saves it.
- The operator accepts one short paid request. Use only the synthetic command in the driver.
- Root `gateway.yaml` keeps the default guardrail policy in `audit` mode. Under another mode, update the expected action before driving.

- **Automated proof.** Run `control-oceans-admin drive backend-gateway`. The driver signs in through the real browser route and creates one user-owned key with explicit access to `deepseek-v4-flash-0731`; the raw key remains only in process memory.
- **Deterministic evaluation.** The driver submits `rm -rf /tmp/oceans-verify` to `/api/v1/guardrails/evaluate`. The response must be allowed with action `audit`, pack `core.filesystem`, rule `recursive-force-remove`, and reason `filesystem.recursive_force_remove`. The command is evaluated only and is never executed.
- **Live OpenRouter request.** The driver submits one non-streaming Chat Completions request with a forced `bash` function call, `max_tokens: 32`, and gateway model `deepseek-v4-flash-0731`. It requires HTTP 200 and an `x-request-id`.
- **Decision and log proof.** Through the authenticated browser session, the driver requires a request-linked `generated_tool_call` decision for `core.filesystem`, then loads the matching request-log detail. It confirms provider `openrouter`, configured upstream model `deepseek/deepseek-v4-flash-0731`, success, a tool call, and provider usage when supplied.
- **Cleanup.** The driver revokes the temporary key in a `finally` block and confirms that `/v1/models` rejects it. It writes only sanitized metadata to `backend-gateway-canary-proof.json`.
- **Proof.** Run `control-oceans-admin evidence backend-gateway` before and after stack cleanup.

## Gotchas

- A configured route, healthy Models page, or present environment reference is not live-provider proof. Startup requires referenced environment variables to exist but does not validate credential correctness or upstream reachability.
- OpenRouter can choose its serving host. The gateway request log proves the configured OpenRouter provider and upstream model ID, not the physical serving host.
- Root `gateway.yaml` uses audit mode, so a matched generated tool call is returned to the caller and recorded as `audit`. It is not executed by this driver.
- Direct `/api/v1/guardrails/evaluate` decisions have no inference request ID. Use the generated-tool decision for request-linked proof.
- Payload evidence is redacted and bounded by `gateway.yaml`; do not require raw prompts, responses, credentials, or authorization headers.
- Provider outages, model access changes, expired credentials, quota, and network failures are concrete integration failures. Run `doctor` again before retrying, then clean the failed key and request residue.
