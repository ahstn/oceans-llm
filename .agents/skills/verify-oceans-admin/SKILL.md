---
name: verify-oceans-admin
description: Verify the Oceans LLM browser admin control plane against the real local gateway and seeded demo data. Use for user-path checks of sign-in, models, API keys, agent sessions, and request logs.
---

# Verify Oceans Admin

Use this skill to drive the embedded TanStack Start admin UI through the gateway. The primary surface is the browser UI at `/admin`. The gateway API is a secondary surface used only for health checks and read-only confirmation.

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

`launch` requires `lsof` for listener and checkout ownership checks. It runs the existing `mise run dev-stack` task with alternate ports. The task derives a temporary runtime config from `gateway.yaml` with the selected bind port, refreshes this checkout's gitignored `gateway.db` with the local demo seed, and records process IDs under `/tmp/oceans-admin-verification/$OCEANS_VERIFY_RUN_ID/`. It refuses to start if either selected port is in use or another gateway process has this repository as its working directory.

The instance is ready when `launch` prints `ready` and both `/readyz` and `/api/v1/health` answer. The sign-in URL is `http://127.0.0.1:$OCEANS_VERIFY_GATEWAY_PORT/admin/login`. Protected feature routes remain under `/admin/*`.

This checkout cannot run two verification stacks safely because both would write `gateway.db`. A stack in another checkout has a separate database and is safe when ports differ. Do not drive a pre-existing instance.

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

The Models driver opens the protected `/admin/api-keys` route, captures its redirect to sign-in, signs in as the seeded `admin@local` user, follows the `Models` sidebar link, checks the displayed count against rendered rows and the total count against the read-only admin models response, checks every platform-admin `Model info` section, enables `Context window` and `Capabilities`, and opens `Client config` for `gpt-5.6-sol`.

The current `gateway.yaml` grants the `agent_sessions` page to platform administrators. Verify the list, filtering, pagination, and a matching detail sheet against the seeded demo data.

For other features, follow the exact stable handles in the feature map. Extend the driver with a named command before you report a new path as automated.

## Evidence

Evidence is written to `/tmp/oceans-admin-verification/$OCEANS_VERIFY_RUN_ID/evidence/`. Keep the run ID in the verification report. The Models proof produces:

- `01-login.png` and `01-login.aria.txt` for the user entry state.
- `02-models.png` and `02-models.aria.txt` for the resulting model list.
- `03-model-info.png` and `03-model-info.aria.txt` for the selected model Access detail.
- `04-model-columns.png` and `04-model-columns.aria.txt` for the optional desktop columns.
- `05-model-client-config.png` and `05-model-client-config.aria.txt` for generated client configuration.
- `models-proof.json` with the visited URLs, displayed, rendered, total, and API counts, model ID, gateway version, and action log.
- `stack.log` beside the evidence directory for launch and runtime diagnostics. Cleanup redacts seeded passwords and raw demo API-key secrets from this log.

A valid proof exercises the real browser path. It captures the action and resulting state, not only a final screenshot. It also confirms the model list through the production admin API used by the UI. Do not use internal state setters or test-only endpoints. The local demo seed is the production seed boundary for this development command; no provider call is required to list models.

Mocks are valid only when the production boundary already isolates an external system. This Models proof uses no mock. Do not interpret a rendered configured provider as proof that its credentials or live upstream service work.

## Cleanup

Always run cleanup after success and after each failed attempt:

```bash
.agents/skills/verify-oceans-admin/scripts/control-oceans-admin cleanup
```

Cleanup sends termination only to the process IDs recorded by this run and checks port ownership before it signals a remaining listener. It removes the run's control files and checkout lock. It does not remove `evidence/` or `stack.log`, and it does not delete `gateway.db`.

Confirm that proof survived teardown:

```bash
.agents/skills/verify-oceans-admin/scripts/control-oceans-admin evidence
```

## Helpers

The executable helper is [scripts/control-oceans-admin](./scripts/control-oceans-admin). Its supported commands are:

```text
control-oceans-admin launch
control-oceans-admin doctor
control-oceans-admin drive models
control-oceans-admin evidence
control-oceans-admin cleanup
```

The browser implementation is [scripts/drive-models.mjs](./scripts/drive-models.mjs). Call it through `control-oceans-admin` so it receives the recorded URL, evidence path, credentials, and gateway version.
