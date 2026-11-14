# Harness integration

Harness integration verification runs the real OpenCode (with the packed `@ahstn/opencode-oceans` plugin), Pi, Oh-My-Pi and Claude Code command-line clients against a gateway built from this checkout. It proves that the inference endpoints, model discovery and generated client configs work in the clients that people use, not only in gateway unit tests. The suite and its fixtures are in [`tests/harness-integration`](../../../../tests/harness-integration/README.md). The published matrix is [Harness Verification](../../../../docs/src/content/docs/harnesses/verification.mdx); keep it in step with `tests/harness-integration/src/matrix.ts`.

## Sub-features

- `matrix` runs harness x model x provider cells from `tests/harness-integration/src/matrix.ts`. Harnesses: OpenCode plugin, Pi (generated `models.json`), OMP (`litellm` discovery) and Claude Code (generated `settings.json`). Models: `gpt-6-luna`, `claude-haiku-5.5`, `gemini-4-hydrogen`. Providers: OpenRouter (always; 10 cells), Vertex AI `global` (Gemini only for OpenCode, Pi and OMP; runs when `OCEANS_HARNESS_VERTEX_PROJECT` and Google credentials resolve, else skipped as optional `blocked`), and Bedrock (every cell blocked today; opt-in flag only). Each cell runs a conversation, a file read/write and a `date` shell call. Each run checks its request logs for status 200, model, provider, `metadata.operation` (`responses`, `messages` or `chat_completions`) and `agent_harness_key` (`opencode`, `pi`, `oh_my_pi`, and `claude_cli` for Claude Code, whose print mode sends a `claude-cli/` user agent). Cells that don't run are data with a reason: `blocked` (could work; the test account lacks access or quota, or Bedrock `runtime_anthropic_invoke` can't stream for Claude Code) or `not-applicable` (Claude Code with non-Claude models; no OpenAI on Vertex; no Gemini on Bedrock). The suite prints them as `Matrix cell:` lines.
- `mcp` calls Context7 through the aggregate `/mcp` endpoint once per harness.
- `opencode-snippet` runs one conversation with the `opencode.json` that Oceans generates in `Client config`.
- `user-allowlist` proves that a human user whose normalized email is on a model allowlist can call the model from Pi.
- `plugin-discovery` loads the packed plugin in a private OpenCode server and checks the discovered models (including Gemini on `openai-compatible`), hidden-model exclusion, native SDK packages, limits, prices, capability restrictions, and the `low`, `high` and `fast` variants.
- `plugin-live` sends six requests with an image and a PDF: `low`, `high` and `fast` for `gpt-6-luna` (Responses) and `claude-haiku-5.5` (Messages). It checks the native wire fields, the requested and served tier in the request log, and token and cost parity with OpenCode.
- `offline` runs the guardrail hook, event parser, process and setup tests.

## How to get to it (user POV)

- A user installs OpenCode, Pi, OMP or Claude Code. They add `@ahstn/opencode-oceans`, copy the Pi, OpenCode or Claude Code config from `Models` → `Client config`, or add Oceans to OMP's `models.yml` with `discovery.type: litellm`.
- The user picks a model, such as `oceans/gpt-6-luna#fast`, and sends a prompt.
- An admin sees each request in `Request Logs`, tagged with the harness run.

## Driving it with control-oceans-admin

Preconditions:

- No launched stack is needed. The suite builds `target/debug/gateway` from this checkout and starts a temporary gateway with its own LibSQL database. Set `OCEANS_VERIFY_RUN_ID` so the proof has a run.
- `OPENROUTER_API_KEY` is in the environment, or fnox can resolve it without a prompt. The driver passes it only to the suite, which passes it only to the temporary gateway. The driver never prints or saves it.
- The machine can reach OpenRouter and Context7.
- Bun 1.4.4 or later on `PATH` (OMP and the plugin need it), and `bwrap` on Linux for the shell sandbox.
- The operator accepts short paid requests: six plugin requests, 30 matrix runs (10 cells x 3 tests; tool runs take more than one call), four MCP runs, and the snippet and allowlist runs. Gemini 4 Hydrogen costs about 15 times more per token than the other two models. The temporary service account has a hard daily budget of $10, and each plugin request has a 2,048-token output limit.

- **Automated proof.** Run `control-oceans-admin drive harness-integration`. The driver installs the suite's dependencies with `mise run //harness:install`, snapshots the routed source, and runs `npm test` in `tests/harness-integration` with the verbose and JSON reporters.
- **Checks.** The proof has one check per test, named from the test file and title. A skipped test, such as the prompt-cache canary that needs an external gateway, is an optional `blocked` check. `plugin-live-usage` passes only when the plugin printed a live-request line for every live case. `harness-suite-exit` fails when the suite exits non-zero or is killed, even if every reported test passed. The driver deletes the previous JSON report first, so a crashed run cannot reuse it.
- **Details.** `details.live` lists each plugin request: family, variant, endpoint, request-log ID, requested and served tier, tokens, and the OpenCode and Oceans cost.
- **Artifacts.** `harness-integration-results.json` is the vitest JSON report. The full suite log, with the temporary gateway's logs, is `harness-integration.log` beside the evidence directory.
- **Proof.** Run `control-oceans-admin evidence harness-integration --require-current`.

## Gotchas

- The suite rebuilds `target/debug/gateway`. A launched verification stack in this checkout keeps running its old process, but stop it first if you want the two to use the same build.
- On a BYOK OpenRouter account, OpenRouter returns `usage.cost: 0` for OpenAI models and puts the charge in `cost_details.upstream_inference_cost`. OpenCode uses `usage.cost`, so its cost for `gpt-6-luna` is `$0`. Oceans bills the upstream cost. The parity check allows $0.0001, so it still passes at canary size.
- OpenRouter serves `claude-haiku-5.5` at standard speed, so the Anthropic `fast` case is requested at `priority` and served at `default`. The suite accepts any served tier and checks billing against it.
- OpenAI can serve a priority request at `default` when it is short of capacity. The `fast` case then checks cost parity instead of a higher Oceans cost.
- Changes to `tests/harness-integration/**` are routed to the "tests only" skip rule. Run this proof by hand when you change the suite.
