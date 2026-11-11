# Native harness integration tests

This package runs the real Pi and OpenCode command-line entrypoints against Oceans LLM Gateway. Model requests go to an Oceans model backed by OpenRouter; MCP requests go to the Oceans aggregate `/mcp` endpoint and reach Context7 through the gateway.

The suite is credential-sensitive and makes paid, live model calls. Do not run it with production administrator credentials or against an untrusted gateway.

## Managed local gateway

Managed mode is the default. The global setup builds and starts a temporary local `gateway` process with an isolated LibSQL database, registers Context7, grants its discovered tools to the harness service account, and removes the runtime directory after the suite.

Required environment variable:

- `OPENROUTER_API_KEY`: upstream OpenRouter credential used only by the temporary gateway process.

Optional environment variable:

- `OPENROUTER_TEST_MODEL`: upstream OpenRouter model ID. Defaults to `deepseek/deepseek-v4.1-flash`.

The harness processes receive an Oceans API key, local gateway URL, and isolated HOME/XDG directories. They do not receive the OpenRouter key, gateway administrator credentials, or unrelated parent-process environment variables. Shell access and filesystem access outside each temporary workspace are denied.

## OpenCode

The suite pins the native OpenCode V2 CLI and Promise client SDK to `2.2.6`. The shared Pi/OpenCode contract does not hand-write a provider config. It asks Oceans for the OpenCode config that the admin UI shows for the test model (`POST /api/v1/admin/models/client-configs`) and runs OpenCode with that config. The suite proves that the generated provider, package, `env` key variable, base URL, and model entry work in the real client. The adapter adds only harness settings: the `x-oceans-tags` request tag on each provider, the Oceans MCP server, a step limit, permissions, and the guardrail plugin.

In managed mode, the gateway starts with `GATEWAY_CLIENT_CONFIG_BASE_URL` set to its local URL, so the generated config points at it. In external mode, the generated config uses the deployment's own client-config base URL.

Each run starts a private `opencode serve` process with the isolated environment, in the same way as `--standalone`, instead of using a shared background service. The adapter waits until `opencode mcp list --server <url>` reports Oceans as connected, and then sends the prompt with `opencode run --server <url>`. V2 connects MCP servers in the background, so without this wait the first model request can have no Oceans tools. The Oceans MCP server sets `codemode: false`, so aggregate MCP calls show as direct tool calls in the event stream.

The guardrail plugin is a V2 plugin. It checks each `shell` tool call with Oceans in `tool` `execute.before`, and it confines the command in `shell` `create.before`, after OpenCode checks permissions. It also rejects `session_move`. That tool has no permission check, and it would move the workspace boundary that the `external_directory` deny rule enforces.

### Oceans discovery plugin

`src/opencode-plugin.test.ts` packs `packages/opencode-oceans` with Bun and loads that archive in a real private OpenCode server. The discovery config supplies only the gateway URL and the name of the Oceans key environment variable. A provider overlay adds request tags and output limits. It does not supply static models, API overrides, or capability overrides.

The suite checks authorized model discovery, hidden-model exclusion, native SDK packages, context and output limits, per-million-token prices, reasoning and service-tier variants, and a route that disables images, PDF files, and tools. Its Anthropic routes also cap reasoning at `high` and `medium`, which checks that discovery does not offer unsupported effort levels.

Six live requests use the `low`, `high` and `fast` variants for both `openai/gpt-6-luna` and `anthropic/claude-haiku-5.5`. Each request includes a small image and a valid PDF. The answer must identify content from both files. A passive HTTP hook checks the native Responses or Messages request without changing the request or saving authorization headers. It checks the selected effort, or for `fast`, `service_tier: "priority"` on Responses and `speed: "fast"` on Messages. This is needed because Oceans normalizes Messages request logs to its Chat representation. The suite compares OpenCode's token and cost totals with the correlated Oceans request log, and checks that the log records the `priority` tier for `fast`. OpenCode prices every variant at the standard rate. When the provider serves `priority`, the Oceans cost must be higher than OpenCode's. Each request has a 2,048-token output limit; the temporary service account has a hard daily budget of $5. These tests make paid calls on each normal suite run.

Managed-mode model overrides:

- `OPENROUTER_PLUGIN_OPENAI_MODEL`: defaults to `openai/gpt-6-luna`.
- `OPENROUTER_PLUGIN_ANTHROPIC_MODEL`: defaults to `anthropic/claude-haiku-5.5`.

Overrides must support the tested protocol, tools, images, PDF input, and both reasoning levels. They change the upstream route, not the discovered Oceans model keys.

## External gateway

Set `GATEWAY_BASE_URL` to use an existing Oceans deployment instead of starting a local gateway. Remote URLs must use HTTPS; HTTP is accepted only for loopback hosts.

Required external-mode variables:

- `GATEWAY_ADMIN_EMAIL`
- `GATEWAY_ADMIN_PASSWORD`
- `OCEANS_API_KEY`

Optional external-mode variables:

- `OCEANS_TEST_MODEL`: Oceans model key used by the shared Pi/OpenCode contracts. Defaults to `harness-openrouter`.
- `OCEANS_PLUGIN_OPENAI_MODEL` and `OCEANS_PLUGIN_ANTHROPIC_MODEL`: discovered plugin model keys. Defaults to `gpt-6-luna` and `claude-haiku-5.5`.
- `OCEANS_PLUGIN_TEXT_ONLY_MODEL`: defaults to `harness-plugin-text-only`. This Anthropic route must disable vision and tools and cap reasoning at `medium`.
- `OCEANS_PLUGIN_HIDDEN_MODEL`: defaults to `harness-plugin-hidden`. This model must exist but must not be granted to `OCEANS_API_KEY`.
- `OCEANS_ALLOWLISTED_USER_API_KEY` and `OCEANS_ALLOWLISTED_TEST_MODEL`: human-user key and allowlisted Oceans model used by the Pi user-allowlist contract. Configure both or neither.
- `OCEANS_CACHE_CANARY_MODEL`: enables the Responses prompt-cache canary for this Oceans model key. Use a cache-capable model on one pinned provider route. Request payload capture must use `redacted_payloads` with `prompt_cache_key` retained. The canary requires a positive first-request cache-write counter, a positive second-request cache-read counter, and the same provider, route, and upstream model for both requests.

The external gateway must already expose the selected model and an aggregate MCP endpoint with callable Context7 tools. The administrator account must be able to read request logs so the suite can correlate each harness invocation with its Oceans request-log entry. It must also be able to generate model client configs, because the OpenCode adapter runs the generated config.

For the plugin suite, enable `redacted_payloads` request logging. Both selected models must accept images and PDF files. Cap the Anthropic model at `high`. The key must also grant the text-only negative control, but not the hidden model. Match the managed gateway's route configuration in `src/global-setup.ts`; missing models fail the suite rather than skip coverage.

## Commands

From the repository root:

```sh
mise install
mise run //harness:typecheck
mise run //harness:test
# Only the packed discovery plugin suite (six paid inference calls):
mise run //harness:plugin-test
```

The install task is cached from `package.json` and `package-lock.json`; lint and typechecking do not reinstall the dependency tree when the lockfile and installed output are unchanged. It runs OpenCode's own postinstall with Bun so the native CLI is present even when npm blocks dependency install scripts.

If the OpenRouter key is held in fnox, inject it after mise loads the local environment. This avoids a stale shell or `mise.local.toml` value. This command resolves only the required key and does not print it:

```sh
mise exec -- sh -eu -c '
  OPENROUTER_API_KEY="$(fnox --non-interactive --no-daemon get OPENROUTER_API_KEY)"
  export OPENROUTER_API_KEY
  exec npm test --prefix tests/harness-integration -- src/opencode-plugin.test.ts
'
```

For CI, provide `OPENROUTER_API_KEY` for managed mode, or the external-mode variables above from the CI secret store. Do not expose those secrets to untrusted pull requests. The live test requires outbound access to OpenRouter and Context7.

The cache canary is separate from the default OpenRouter contract because it makes two large paid requests and requires a provider that reports both cache-write and cache-read counters. Run it only against an external test deployment. Its route check proves Oceans route affinity. The deployment configuration must pin the same endpoint, region, project, and account because these values are not all present in the request-log API.

## Adding another harness

1. Add a `HarnessAdapter` implementation under `src/adapters/`.
2. Give every run isolated HOME/XDG directories with `createIsolatedPaths` and `createHarnessEnvironment`.
3. Configure the client to use `${gateway.baseUrl}/v1`, `gateway.apiKey`, and `gateway.model`; never point it directly at OpenRouter.
4. Send a unique `harness_run` request tag and return it as `HarnessRun.requestTag`.
5. Parse the final assistant response and structured tool-call inputs from the harness event stream. Do not assert against raw output that can echo the prompt.
6. Add the adapter to the shared contract list in `src/harness-contract.test.ts` and add focused parser tests for any new event format.
7. Restrict shell access, parent-environment inheritance, and filesystem access outside the temporary workspace.

Claude Code is intentionally not part of this package. Add it in a follow-up only when its native model-provider and HTTP MCP configuration can satisfy the same routing, isolation, tool-evidence, and request-log contracts without a harness-specific shortcut.
