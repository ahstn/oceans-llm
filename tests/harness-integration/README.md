# Native harness integration tests

This package runs the real Pi and OpenCode command-line entrypoints against Oceans LLM Gateway. Model requests go to an Oceans model backed by OpenRouter; MCP requests go to the Oceans aggregate `/mcp` endpoint and reach Context7 through the gateway.

The suite is credential-sensitive and makes paid, live model calls. Do not run it with production administrator credentials or against an untrusted gateway.

## Managed local gateway

Managed mode is the default. The global setup builds and starts a temporary local `gateway` process with an isolated LibSQL database, registers Context7, grants its discovered tools to the harness service account, and removes the runtime directory after the suite.

Required environment variable:

- `OPENROUTER_API_KEY`: upstream OpenRouter credential used only by the temporary gateway process.

Optional environment variable:

- `OPENROUTER_TEST_MODEL`: upstream OpenRouter model ID. Defaults to `deepseek/deepseek-v4-flash`.

The harness processes receive an Oceans API key, local gateway URL, and isolated HOME/XDG directories. They do not receive the OpenRouter key, gateway administrator credentials, or unrelated parent-process environment variables. Shell access and filesystem access outside each temporary workspace are denied.

## External gateway

Set `GATEWAY_BASE_URL` to use an existing Oceans deployment instead of starting a local gateway. Remote URLs must use HTTPS; HTTP is accepted only for loopback hosts.

Required external-mode variables:

- `GATEWAY_ADMIN_EMAIL`
- `GATEWAY_ADMIN_PASSWORD`
- `OCEANS_API_KEY`

Optional external-mode variables:

- `OCEANS_TEST_MODEL`: Oceans model key used by the shared Pi/OpenCode contracts. Defaults to `harness-openrouter`.
- `OCEANS_ALLOWLISTED_USER_API_KEY` and `OCEANS_ALLOWLISTED_TEST_MODEL`: human-user key and allowlisted Oceans model used by the Pi user-allowlist contract. Configure both or neither.
- `OCEANS_CACHE_CANARY_MODEL`: enables the Responses prompt-cache canary for this Oceans model key. Use a cache-capable model on one pinned provider route. Request payload capture must use `redacted_payloads` with `prompt_cache_key` retained. The canary requires a positive first-request cache-write counter, a positive second-request cache-read counter, and the same provider, route, and upstream model for both requests.

The external gateway must already expose the selected model and an aggregate MCP endpoint with callable Context7 tools. The administrator account must be able to read request logs so the suite can correlate each harness invocation with its Oceans request-log entry.

## Commands

From the repository root:

```sh
mise install
mise run harness-integration-typecheck
mise run harness-integration-test
```

The install task is cached from `package.json` and `package-lock.json`; lint and typechecking do not reinstall the dependency tree when the lockfile and installed output are unchanged.

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
