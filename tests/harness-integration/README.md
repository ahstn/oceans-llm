# Native harness integration tests

This package runs the real OpenCode, Pi, OMP (Oh My Pi) and Claude Code command-line entrypoints against Oceans LLM Gateway. Model requests go to Oceans models backed by OpenRouter. MCP requests go to the Oceans aggregate `/mcp` endpoint and reach Context7 through the gateway.

The suite is credential-sensitive and makes paid, live model calls. Do not run it with production administrator credentials or against an untrusted gateway.

## Harness matrix

`src/matrix.ts` defines the harness x model x provider matrix, and `src/matrix.test.ts` generates its tests from it. Each harness uses the setup that the Oceans docs give to users. A cell has one of four statuses:

- `run`: the suite runs the cell when its provider is configured. If the provider is not configured, the test skips and gives the reason.
- `blocked`: the cell applies but cannot run yet. The reason is kept in `src/matrix.ts`. A blocked cell with an opt-in variable runs when that variable is set, so that access can be checked again. Otherwise the test skips and gives the reason.
- `not-applicable`: the harness, model or provider does not support the cell. The reason is kept in `src/matrix.ts`.

The suite generates tests only for `run` cells and opt-in `blocked` cells, and prints one `Matrix cell:` line for every other cell. On OpenRouter, each runnable cell is the gateway API that the harness must use for that model, as the request log records it in `metadata.operation`:

| Harness | Setup | `gpt-6-luna` | `claude-haiku-5.5` | `gemini-4-hydrogen` |
| --- | --- | --- | --- | --- |
| OpenCode 2.2.6 | Packed `@ahstn/opencode-oceans` plugin with `litellm` discovery | `responses` | `messages` | `chat_completions` |
| Pi 0.87.1 | Generated `models.json` | `chat_completions` | `messages` | `chat_completions` |
| OMP 18.20.0 | `models.yml` with `litellm` discovery | `responses` | `messages` | `chat_completions` |
| Claude Code 2.1.323 | Generated `settings.json` | Not applicable | `messages` | Not applicable |

Claude Code supports only Claude models through a gateway, so its other cells are not applicable.

On Vertex AI (location `global`), `gemini-4-hydrogen` runs for OpenCode, Pi and OMP on `chat_completions`, with the Oceans model key `gemini-4-hydrogen-vertex`. Claude Haiku on Vertex AI is blocked by quota, and Vertex AI has no OpenAI models. On Bedrock, the OpenAI and Claude cells of OpenCode, Pi and OMP are blocked by account access, and `OCEANS_HARNESS_BEDROCK=1` runs them again. Bedrock has no Gemini models. Claude Code stays blocked on Bedrock whatever the access, because its `runtime_anthropic_invoke` routes do not stream. `src/matrix.ts` has the full reasons.

Every applicable cell runs three tests:

- **chat**: the reply must be an exact token.
- **files**: the harness reads `source.txt` and writes `destination.txt` with its own file tools. The test checks the file content and a completed read call and write call. OpenCode gives GPT models a `patch` tool in place of `write`, so OpenCode accepts either.
- **bash**: the shell tool runs `date -u +%F`. The test checks a completed shell call whose command contains `date`, and that the answer contains the UTC date from before or after the run.

On OpenRouter, that is 30 runs (10 cells x 3 tests). With Vertex AI configured, there are 9 more runs, and the Bedrock opt-in adds 18. Every run sends a new `harness_run` tag, and the test then checks every request log with that tag: status 200, the cell's Oceans model, provider `openrouter`, the cell's operation, the cell's provider (`openrouter`, `vertex` or `bedrock`, the managed provider ids), and the harness key that the gateway takes from the `User-Agent`. The keys are `opencode`, `pi`, `oh_my_pi` and `claude_cli`. The gateway classifies Claude Code model requests (`claude-cli/<version>`) as `claude_cli`. When the key does not match, the failure message shows the raw `User-Agent` from the request-log detail. Each run prints one `Harness run:` line with its operations, harness keys, request count and completed tools.

The three models of one harness and test run at the same time (`maxConcurrency: 3`). Each run has a new, empty workspace with its own home, config, cache and data folders, and its own request tag. Models use their default reasoning level.

`src/harness-contract.test.ts` holds the runs outside the matrix:

- **Context7 through aggregate MCP**: one run for each harness. OpenCode, Pi and OMP use `gpt-6-luna`, and Claude Code uses `claude-haiku-5.5`. The model must call `search_tools` and then `call_tool` for Context7, and both calls must complete. Runs without MCP do not connect to it, so a Context7 outage cannot fail the matrix.
- **OpenCode generated snippet**: one chat run on `gpt-6-luna` with the OpenCode config that Oceans generates. The expected operation comes from the generated provider package.
- **Pi user allowlist**: a human user whose normalised email is in a model allowlist runs Pi with the generated `models.json`.

## Harness setup

The harness processes receive an Oceans API key, the gateway URL, and isolated HOME/XDG folders. They do not receive the OpenRouter key, gateway administrator credentials, or unrelated parent-process environment variables.

Every shell command goes through an Oceans guardrail check (`POST /api/v1/guardrails/evaluate`) and then runs in `bwrap` on Linux or `sandbox-exec` on macOS. The sandbox sees only the workspace, mounted at `/workspace`. A failed or invalid guardrail answer blocks the command. File tools must stay in the workspace: the hooks reject `~` and `@` prefixes, URLs and internal schemes such as `xd://`, `local://`, `agent://` and `proc://`, OMP hashline headers that point outside, and paths that leave the workspace. The hooks do not resolve symbolic links.

### OpenCode

The suite pins the native OpenCode V2 CLI and Promise client SDK to `2.2.6`. Matrix runs load the packed Oceans plugin, which finds the gateway in `gateways.yml` with `litellm` discovery. The generated-snippet run asks Oceans for the OpenCode config that the admin UI shows (`POST /api/v1/admin/models/client-configs`) and runs OpenCode with that config. Both add only harness settings: the `x-oceans-tags` request tag, a step limit, permissions, the guardrail plugin, and the Oceans MCP server for MCP runs.

In managed mode, the gateway starts with `GATEWAY_CLIENT_CONFIG_BASE_URL` set to its local URL, so generated configs point at it. In external mode, generated configs use the deployment's own client-config base URL.

Each run starts a private `opencode serve` process with the isolated environment, in the same way as `--standalone`, instead of using a shared background service. It sends the prompt with `opencode run --server <url> --title <title>`. A session without a title asks the model for one, and stopping the server cancels that request. For MCP runs, the adapter first waits until `opencode mcp list --server <url>` reports Oceans as connected. V2 connects MCP servers in the background, so without this wait the first model request can have no Oceans tools. The Oceans MCP server sets `codemode: false`, so aggregate MCP calls show as direct tool calls in the event stream.

The guardrail plugin is a V2 plugin. It checks each `shell` tool call with Oceans in `tool` `execute.before`, and it confines the command in `shell` `create.before`, after OpenCode checks permissions. It also rejects `session_move`. That tool has no permission check, and it would move the workspace boundary that the `external_directory` deny rule enforces.

### Pi

Pi runs with the `models.json` that Oceans generates for the model. The adapter adds only the request tag header. The generated `apiKey` names an environment variable, and the adapter supplies the key in that variable. A Pi extension checks and confines `bash`, and keeps `read`, `write` and `edit` in the workspace. MCP runs also load `pi-mcp-adapter` with a workspace `.mcp.json`.

### OMP

OMP reads `models.yml` from `PI_CODING_AGENT_DIR`, in the isolated config folder, so the workspace never holds an `.omp` folder. The provider uses `litellm` discovery with `api: openai-completions` and `auth: apiKey`, and sends `claude-haiku-5.5` over `anthropic-messages`. Without `auth: apiKey`, OMP gives custom `anthropic-messages` models Claude Code request shaping. OMP sends models that the gateway lists with the `openai` provider family over Responses. The run uses `--tools read,write,bash` (plus the Oceans MCP tools for MCP runs) and turns off skills, rules, context files, sessions, titles, LSP and extension discovery. An OMP extension returns the confined `bash` input, because OMP ignores changes to `event.input`. It also checks the `bash` working folder and blocks every other tool. MCP runs add an HTTP server to `mcp.json` and set `OMP_MCP_REQUIRE_READY=1`.

### Claude Code

Claude Code runs with the `settings.json` that Oceans generates for the model, in an isolated `CLAUDE_CONFIG_DIR`. The adapter removes the `ANTHROPIC_AUTH_TOKEN` placeholder and supplies the key in the process environment. The tag goes in `ANTHROPIC_CUSTOM_HEADERS`. The run loads only user settings, uses `--strict-mcp-config`, allows only `Read`, `Write`, `Edit` and `Bash` (and the Oceans MCP tools for MCP runs), and turns off non-essential traffic, telemetry, auto-updates, memory and `CLAUDE.md` files. It never uses `--bare`, which skips hooks.

A `PreToolUse` command hook with `onFailure: "block"` sees every tool call. It returns `updatedInput` with the confined `Bash` command, keeps file tools in the workspace, allows the Oceans MCP tools and denies all other tools. MCP runs pass the server with `--mcp-config` and `alwaysLoad: true`. The key goes in `OCEANS_API_KEY`, because Claude Code reads its own credential variables as empty in MCP headers.

## Oceans discovery plugin

`src/opencode-plugin.test.ts` packs `packages/opencode-oceans` with Bun and loads that archive in a real private OpenCode server. The discovery config supplies only the gateway URL and the name of the Oceans key environment variable. A provider overlay adds request tags and output limits. It does not supply static models, API overrides, or capability overrides.

The suite checks authorized model discovery, hidden-model exclusion, the SDK package of each model, context and output limits, per-million-token prices, reasoning and service-tier variants, and a route that disables images, PDF files, and tools. `gpt-6-luna` uses the Responses package, `claude-haiku-5.5` the Anthropic package, and `gemini-4-hydrogen` the Chat Completions package. Gemini input modalities must match the gateway listing. Its Anthropic routes also cap reasoning at `high` and `medium`, which checks that discovery does not offer unsupported effort levels.

Six live requests use the `low`, `high` and `fast` variants for both `openai/gpt-6-luna` and `anthropic/claude-haiku-5.5`. Each request includes a small image and a valid PDF. The answer must identify content from both files. A passive HTTP hook checks the native Responses or Messages request without changing the request or saving authorization headers. It checks the selected effort, or for `fast`, `service_tier: "priority"` on Responses and `speed: "fast"` on Messages. This is needed because Oceans normalizes Messages request logs to its Chat representation. The suite compares OpenCode's token and cost totals with the correlated Oceans request log, and checks that the log records the `priority` tier for `fast`. OpenCode prices every variant at the standard rate. When the provider serves `priority`, the Oceans cost must be higher than OpenCode's. Each request has a 2,048-token output limit. These tests make paid calls on each normal suite run, and each request prints one `Live plugin pass:` line.

## Managed local gateway

Managed mode is the default. The global setup builds and starts a temporary local `gateway` process with an isolated LibSQL database, registers Context7, grants its discovered tools to the harness service account, and removes the runtime directory after the suite. The service account has a hard budget of 10 USD.

Required environment variable:

- `OPENROUTER_API_KEY`: upstream OpenRouter credential used only by the temporary gateway process.

Optional Vertex AI provider. The global setup adds a `gcp_vertex` provider in location `global`, with Application Default Credentials, and the `gemini-4-hydrogen-vertex` model only when both variables are set. Without them, the Vertex AI cells skip. Never put project IDs or key paths in the repository:

- `OCEANS_HARNESS_VERTEX_PROJECT`: the Google Cloud project for Vertex AI.
- `GOOGLE_APPLICATION_CREDENTIALS`: the credentials file for Application Default Credentials.

Optional Bedrock opt-in. Use it only with a scoped IAM user, never with root credentials. With `OCEANS_HARNESS_BEDROCK=1`, the global setup adds an `aws_bedrock` provider (`bedrock_runtime`, default credential chain) and the `gpt-6-luna-bedrock` and `claude-haiku-5.5-bedrock` models. Only the temporary gateway gets the AWS credentials:

- `OCEANS_HARNESS_BEDROCK`: set to `1` to run the blocked Bedrock cells.
- `OCEANS_HARNESS_BEDROCK_REGION`, else `AWS_REGION`: the Bedrock region.

Optional upstream overrides:

- `OPENROUTER_PLUGIN_OPENAI_MODEL`: defaults to `openai/gpt-6-luna`. The user-allowlist model uses the same upstream.
- `OPENROUTER_PLUGIN_ANTHROPIC_MODEL`: defaults to `anthropic/claude-haiku-5.5`.
- `OPENROUTER_PLUGIN_GOOGLE_MODEL`: defaults to `google/gemini-4-hydrogen`.

Overrides must support the tested protocol, tools, images, PDF input, and both reasoning levels. They change the upstream route, not the Oceans model keys.

## External gateway

Set `GATEWAY_BASE_URL` to use an existing Oceans deployment instead of starting a local gateway. Remote URLs must use HTTPS; HTTP is accepted only for loopback hosts.

Required external-mode variables:

- `GATEWAY_ADMIN_EMAIL`
- `GATEWAY_ADMIN_PASSWORD`
- `OCEANS_API_KEY`

Optional external-mode variables:

- `OCEANS_PLUGIN_OPENAI_MODEL`, `OCEANS_PLUGIN_ANTHROPIC_MODEL` and `OCEANS_PLUGIN_GOOGLE_MODEL`: the matrix model keys. Defaults to `gpt-6-luna`, `claude-haiku-5.5` and `gemini-4-hydrogen`.
- `OCEANS_VERTEX_GOOGLE_MODEL`: the Oceans model key of `gemini-4-hydrogen` on a Vertex AI route. Without it, the Vertex AI cells skip.
- `OCEANS_PLUGIN_TEXT_ONLY_MODEL`: defaults to `harness-plugin-text-only`. This Anthropic route must disable vision and tools and cap reasoning at `medium`.
- `OCEANS_PLUGIN_HIDDEN_MODEL`: defaults to `harness-plugin-hidden`. This model must exist but must not be granted to `OCEANS_API_KEY`.
- `OCEANS_ALLOWLISTED_USER_API_KEY` and `OCEANS_ALLOWLISTED_TEST_MODEL`: human-user key and allowlisted Oceans model used by the Pi user-allowlist test. Configure both or neither.
- `OCEANS_CACHE_CANARY_MODEL`: enables the Responses prompt-cache canary for this Oceans model key. Use a cache-capable model on one pinned provider route. Request payload capture must use `redacted_payloads` with `prompt_cache_key` retained. The canary requires a positive first-request cache-write counter, a positive second-request cache-read counter, and the same provider, route, and upstream model for both requests.

The external gateway must already expose the matrix models on OpenRouter routes, with the same APIs as the managed gateway, and an aggregate MCP endpoint with callable Context7 tools. The administrator account must be able to read request logs so the suite can correlate each harness invocation with its Oceans request-log entries. It must also be able to generate model client configs, because the OpenCode snippet, Pi and Claude Code adapters run the generated configs.

For the plugin suite, enable `redacted_payloads` request logging. The OpenAI and Anthropic models must accept images and PDF files. Cap the Anthropic model at `high`. The key must also grant the text-only negative control, but not the hidden model. Match the managed gateway's route configuration in `src/global-setup.ts`; missing models fail the suite rather than skip coverage.

## Commands

From the repository root:

```sh
mise install
mise run //harness:typecheck
mise run //harness:test
# Only the harness x model x test matrix (30 paid runs):
mise run //harness:matrix-test
# Only the packed discovery plugin suite (six paid inference calls):
mise run //harness:plugin-test
```

The install task is cached from `package.json` and `package-lock.json`; lint and typechecking do not reinstall the dependency tree when the lockfile and installed output are unchanged. It runs the OpenCode and Claude Code install scripts directly, so the native CLIs are present even when npm blocks dependency install scripts. OMP needs Bun 1.4.4 or later, and the Linux shell sandbox needs `bwrap`.

If the OpenRouter key is held in fnox, inject it after mise loads the local environment. fnox resolves the key only where `fnox.local.toml` exists: a new worktree needs `mise run secrets-sync` first, or run fnox from the main checkout. The `verify-oceans-admin` driver also reads `OCEANS_HARNESS_VERTEX_PROJECT` and `OCEANS_HARNESS_VERTEX_SA_JSON` from fnox when the Vertex variables are not set, and writes the service account to a temporary 0600 file that it deletes after the run. This avoids a stale shell or `mise.local.toml` value. This command resolves only the required key and does not print it:

```sh
mise exec -- sh -eu -c '
  OPENROUTER_API_KEY="$(fnox --non-interactive --no-daemon get OPENROUTER_API_KEY)"
  export OPENROUTER_API_KEY
  exec npm test --prefix tests/harness-integration -- src/matrix.test.ts -t "chat"
'
```

Test names are `<harness label> <provider> <model family> <test>`, so `-t` can select one harness, provider, model or test, for example `-t "OMP with litellm discovery openrouter openai bash"`, `-t "vertex"` or `-t "files"`.

For CI, provide `OPENROUTER_API_KEY` for managed mode, or the external-mode variables above from the CI secret store. Do not expose those secrets to untrusted pull requests. The live test requires outbound access to OpenRouter and Context7.

The cache canary is separate from the default OpenRouter contract because it makes two large paid requests and requires a provider that reports both cache-write and cache-read counters. Run it only against an external test deployment. Its route check proves Oceans route affinity. The deployment configuration must pin the same endpoint, region, project, and account because these values are not all present in the request-log API.

## Adding another harness

1. Add a `HarnessAdapter` implementation under `src/adapters/`. Its `harness` is the request-log `agent_harness_key` that the gateway gives the harness's `User-Agent`, and `tools` names its native read, write and shell tools.
2. Give every run isolated HOME/XDG directories with `createIsolatedPaths` and `createHarnessEnvironment`.
3. Configure the client with the setup from the Oceans docs, pointed at `runtime.baseUrl` and `runtime.apiKey`, for the model that `run` receives. Never point it directly at OpenRouter.
4. Send a unique `harness_run` request tag on every model request and return it as `HarnessRun.requestTag`.
5. Parse the final assistant response and structured tool-call inputs from the harness event stream. Do not assert against raw output that can echo the prompt. Add focused parser tests in `src/adapters/events.test.ts` for any new event format.
6. Check every shell command with `guardShell`, confine it with `confineShell`, and check file-tool paths with `workspacePathError` from `src/adapters/sandbox.ts`. Add offline hook tests to `src/guardrails-hook.test.ts`. Restrict parent-environment inheritance.
7. Add a row to `MATRIX` in `src/matrix.ts` with a cell for each provider and model, and add the adapter to the adapter maps in `src/matrix.test.ts` and `src/harness-contract.test.ts`.
