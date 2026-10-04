---
name: verify-oceans-admin
description: Verify the Oceans LLM admin control plane and backend gateway against the real local stack, seeded demo data, and bounded live providers when required. Use for user-path checks of sign-in, models, API keys, observability, agent sessions, request logs, MCP registry and tool access, OpenRouter routing, guardrails, and Skills storage.
---

# Verify Oceans Admin

Use this skill to drive the embedded TanStack Start admin UI through the gateway. The primary surface is the browser UI at `/admin`. The gateway API is a secondary surface for health checks, saved-state confirmation, bounded MCP tool calls, and bounded live LLM requests when the change affects the request path.

Read [features/README.md](./features/README.md) before you choose a proof. Use the exact feature recipe for the path under test.

## Launch

Run all commands from the repository root. Activate the configured toolchain and select a unique run ID:

```bash
eval "$(/Users/ahstn/.local/bin/mise activate zsh)"
export OCEANS_VERIFY_RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)-$$"
export OCEANS_VERIFY_GATEWAY_PORT=38090
export OCEANS_VERIFY_UI_PORT=33010
.agents/skills/verify-oceans-admin/scripts/control-oceans-admin launch
```

`launch` requires `lsof` for listener and checkout ownership checks. It runs the existing `mise run dev-stack` task with alternate ports through `GATEWAY_PORT` and `UI_PORT`, refreshes this checkout's gitignored `gateway.db` with the local demo seed, and records process IDs under `/tmp/oceans-admin-verification/$OCEANS_VERIFY_RUN_ID/`. It refuses to start if either selected port is in use or another gateway process has this repository as its working directory.

The instance is ready when `launch` prints `ready` and both `/readyz` and `/api/v1/health` answer. The sign-in URL is `http://127.0.0.1:$OCEANS_VERIFY_GATEWAY_PORT/admin/login`. Protected feature routes remain under `/admin/*`.

For Skills storage, set `OCEANS_VERIFY_SKILLS=true` before launch. The harness starts native RustFS through the `rustfs` mise profile and creates a private run-local configuration, database, and synthetic service-account key. See [features/skills.md](./features/skills.md) for the full recipe. This option leaves `gateway.db` unchanged.

This checkout cannot run two verification stacks safely because ordinary runs would both write `gateway.db`. The checkout lock also applies to Skills verification. A stack in another checkout has a separate database and is safe when ports differ. Do not drive a pre-existing instance.

Teardown uses the recorded process IDs:

```bash
.agents/skills/verify-oceans-admin/scripts/control-oceans-admin cleanup
```

## Doctor

Run the read-only doctor check before browser work and whenever the UI looks stale:

```bash
.agents/skills/verify-oceans-admin/scripts/control-oceans-admin doctor
```

Doctor confirms that the selected gateway and UI ports are still owned by their recorded listener PIDs, checks `/readyz`, and records the gateway service version from `/api/v1/health`. The short-lived launcher PID can exit after mise has started both listeners. A failed ownership check means this instance is not safe to drive.

## Drive

The harness is `control-oceans-admin`. It uses the repository's installed Playwright package and Chromium. It drives ARIA roles, accessible labels, route paths, and existing test IDs from `crates/admin-ui/web`.

Prove the Models path:

```bash
.agents/skills/verify-oceans-admin/scripts/control-oceans-admin drive models
```

The Models driver runs the idempotent `mise run e2e-install` task to ensure Chromium is available. It then opens the protected `/admin/api-keys` route, captures its redirect to sign-in, signs in as the seeded `admin@local` user, follows the `Models` sidebar link, checks the displayed count against rendered rows and the total count against the read-only admin models response, checks every platform-admin `Model info` section, enables `Context window`, `Capabilities`, and `Intelligence`, and opens `Client config` for `gpt-6-astra`.

Prove the Leaderboard and Agent Harnesses paths together:

```bash
.agents/skills/verify-oceans-admin/scripts/control-oceans-admin drive observability
```

The Observability driver opens the protected Leaderboard route, signs in, and compares both the
Leaderboard and Agent Harnesses desktop tables with their production admin API responses for the
seeded 7-day window. It selects the 31-day range on both pages and repeats the comparison. The proof
also requires the new leaderboard columns, harness token columns, a Mastra row with its Lobe icon,
and an Oh My Pi row with its white `omp.sh` mark.

The current `gateway.yaml` grants the `agent_sessions` page to platform administrators. Verify the list, filtering, pagination, and a matching detail sheet against the seeded demo data.

For other features, follow the exact stable handles in the feature map. Extend the driver with a named command before you report a new path as automated.

Prove backend gateway routing and generated-tool guardrails through OpenRouter:

```bash
.agents/skills/verify-oceans-admin/scripts/control-oceans-admin drive backend-gateway
```

The Backend Gateway driver opens the real API Keys page, creates a temporary user key limited to `deepseek-v4-flash-0731`, evaluates one synthetic destructive command without executing it, and sends one bounded forced-tool Chat Completions request through OpenRouter. It confirms the request-linked guardrail decision and request-log attempt, writes sanitized evidence, revokes the key, and confirms that the revoked key is rejected. Use [features/backend-gateway.md](./features/backend-gateway.md) for the exact contract.

Prove the MCP registry, Tool Sets workbench, grants, and live tool routing:

```bash
export OCEANS_VERIFY_MCP_CANDIDATES_FILE=/absolute/path/to/mcp-candidates.json
.agents/skills/verify-oceans-admin/scripts/control-oceans-admin drive mcp
```

Read [features/mcp.md](./features/mcp.md) before launch. It defines the required gateway credential aliases and the candidate file. The MCP driver creates temporary servers, two tool sets, and one API key through the UI. It verifies saved membership, independent drafts, client configurations, grant enforcement, direct and aggregate tool calls, and invocation records. The API key UI requires one explicit model grant; the driver selects one model but sends no model request. This proof uses public access or static upstream credentials and does not test OAuth consent or token refresh.

Prove Skills storage and ownership with Oceans and native RustFS:

```bash
# Set OCEANS_VERIFY_SKILLS=true before the launch command.
.agents/skills/verify-oceans-admin/scripts/control-oceans-admin drive skills
.agents/skills/verify-oceans-admin/scripts/control-oceans-admin evidence skills
```

Read [features/skills.md](./features/skills.md) before launch. This proof drives regular-user namespace claims, ZIP uploads, file previews, version and default changes, and browser downloads. It compares saved API state and archive digests, verifies shared reads and owner-only updates, and checks service-account restrictions. It sends no LLM request.

## Live LLM requests

Read-only control-plane verification is the default and does not call an upstream provider. Add one short, paid live request when the change affects request routing, provider authentication, request or response translation, streaming, tool calls, usage accounting, request logging, provider error mapping, or another behavior that a configured model list cannot prove. Do not add a paid request for UI-only, documentation-only, seed-only, or unrelated configuration changes.

Use [features/live-llm-requests.md](./features/live-llm-requests.md) for the exact recipe. Prefer `deepseek-v4-flash-0731` through OpenRouter for generic request-path changes. For Bedrock-specific request-path testing, use gateway model `gpt-oss-120b-bedrock`, which routes to Bedrock Mantle model `openai.gpt-oss-120b`. Do not use `openai.gpt-5.6-luna`; it is not enabled for this AWS account. Run both providers only when provider parity is the behavior under test.

Keep each prompt synthetic and small. Limit the output, create a temporary gateway API key with access only to the selected model, and revoke it after the proof. Never print or save raw gateway or provider credentials. For Bedrock Responses requests, set `store: false` unless stored-response behavior is the subject of the test.

A request to verify a request-path or provider change permits one bounded canary when the required local credential is available. Credential presence alone does not justify paid calls for other work. If a canary is outside the stated task, report it as an available paid check instead of running it.

## Evidence

Evidence is written to `/tmp/oceans-admin-verification/$OCEANS_VERIFY_RUN_ID/evidence/`. Keep the run ID in the verification report. The Models proof produces:

- `01-login.png` and `01-login.aria.txt` for the user entry state.
- `02-models.png` and `02-models.aria.txt` for the resulting model list.
- `03-model-info.png` and `03-model-info.aria.txt` for the selected model Access detail.
- `04-model-columns.png` and `04-model-columns.aria.txt` for the optional desktop columns.
- `05-model-client-config.png` and `05-model-client-config.aria.txt` for generated client configuration.
- `models-proof.json` with the visited URLs, displayed, rendered, total, and API counts, model ID, gateway version, and action log.
- `stack.log` beside the evidence directory for launch and runtime diagnostics. Cleanup redacts seeded passwords and raw demo API-key secrets from this log.

The Observability proof produces:

- `01-observability-login.png` and `01-observability-login.aria.txt` for the protected entry state.
- `02-leaderboard-7d.*`, `03-leaderboard-31d.*`, and `03b-leaderboard-mobile.*` for API parity and responsive presentation.
- `04-agent-harnesses-7d.*` and `05-agent-harnesses-31d.*` for the two Agent Harnesses ranges.
- `observability-proof.json` with the production API leaders, rendered table values, ranges, chart
  series counts, gateway version, Mastra/Oh My Pi icon checks, and action log.

A valid proof exercises the real browser path. It captures the action and resulting state, not only a final screenshot. It also confirms rendered data through the production admin API used by the UI. Do not use internal state setters or test-only endpoints. The local demo seed is the production seed boundary for these development commands; no provider call is required for Models, Leaderboard, or Agent Harnesses.

A live canary is separate evidence. Name the gateway model, provider, endpoint family, and observed request-log record. A rendered configured provider or successful health check is not live-provider proof.

The Backend Gateway proof produces:

- `01-backend-api-keys.png` and `01-backend-api-keys.aria.txt` for the authenticated key-management entry state.
- `backend-gateway-canary-proof.json` with the gateway model, OpenRouter provider, configured upstream model, request ID, status, usage presence, tool count, guardrail rule, payload capture mode, gateway version, and action log.
- No prompt, response, gateway key, provider credential, or authorization header.

The MCP proof produces:

- Screenshots and ARIA snapshots for sign-in, discovery, saved membership, independent drafts, client configuration, mobile layout, grants, and filtered invocation lists.
- `mcp-proof.json` with candidate results, saved IDs, gateway version, grant checks, request and invocation IDs, transport type, authentication-error control, and cleanup results.
- No raw key, authorization header, tool arguments, or tool-result payload in the proof JSON. Invocation detail is inspected in memory and is not captured.

Require `mcp-proof.json` to report `passed: true` and inspect each candidate result. An optional candidate failure remains a reported gap. Check `control-oceans-admin evidence mcp` before and after stack cleanup.

The Skills proof produces:

- `01-skills-login.*`, `01b-skills-bundled.*`, `02-skills-before.*`, `03-skills-file-preview.*`, `04-skills-new-version.*`, `05-skills-owner-default.*`, `06-skills-other-owner.*`, and `07-skills-catalog-after.*` screenshots and ARIA snapshots.
- `skills-proof.json` with the run, storage scope, saved IDs, version digests, caller checks, and action log. It must report `passed: true`.
- `skills-storage-cleanup.json` after stack teardown confirms that the run's object prefix is empty and its database was removed. `evidence skills` checks this file after teardown.
- No gateway key, RustFS credential, password, or authorization header in proof artifacts.

Mocks are valid only when the production boundary already isolates an external system. This Models proof uses no mock. Do not interpret a rendered configured provider as proof that its credentials or live upstream service work.

## Cleanup

Always run cleanup after success and after each failed attempt:

```bash
.agents/skills/verify-oceans-admin/scripts/control-oceans-admin cleanup
```

Cleanup sends termination only to the process IDs recorded by this run and checks port ownership before it signals a remaining listener. It removes the run's control files and checkout lock. It does not remove `evidence/` or `stack.log`, and it does not delete `gateway.db`. For an opt-in Skills run, it also removes the run-specific RustFS object prefix and private database/config/token, then stops RustFS only if this run started the recorded process. RustFS credentials, its data directory, and other object prefixes remain intact.

Confirm that proof survived teardown:

```bash
.agents/skills/verify-oceans-admin/scripts/control-oceans-admin evidence backend-gateway
```

Pass the proof name so evidence validation requires its matching proof JSON.

## Helpers

The executable helper is [scripts/control-oceans-admin](./scripts/control-oceans-admin). Its supported commands are:

```text
control-oceans-admin launch
control-oceans-admin doctor
control-oceans-admin drive models
control-oceans-admin drive observability
control-oceans-admin drive backend-gateway
control-oceans-admin drive mcp
control-oceans-admin drive profile
control-oceans-admin drive skills
control-oceans-admin evidence [models|observability|live-llm|backend-gateway|mcp|profile|skills]
control-oceans-admin cleanup

```

The browser implementations are [scripts/drive-models.mjs](./scripts/drive-models.mjs), [scripts/drive-observability.mjs](./scripts/drive-observability.mjs), [scripts/drive-backend-gateway.mjs](./scripts/drive-backend-gateway.mjs), [scripts/drive-mcp.mjs](./scripts/drive-mcp.mjs), [scripts/drive-profile.mjs](./scripts/drive-profile.mjs), and [scripts/drive-skills.mjs](./scripts/drive-skills.mjs). The MCP driver uses [scripts/mcp-browser.mjs](./scripts/mcp-browser.mjs) and [scripts/mcp-canary.mjs](./scripts/mcp-canary.mjs). Call the drivers through `control-oceans-admin` so they receive the recorded URL, evidence path, credentials, and gateway version.
