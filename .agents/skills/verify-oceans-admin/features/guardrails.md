# Guardrails

Guardrails is a read-only, platform-admin-only view of the effective guardrail policy and the privacy-safe history of guardrail decisions. Policy cards show the global default plus any model-route or MCP-server overrides, each with its enabled state, mode, packs, managed checks, and stream buffer size. The decision table filters by request ID, evaluator, phase, and action. It never shows raw prompts, commands, arguments, or results, and nothing on the page changes policy.

## Sub-features

- `guardrails-policies` shows `Effective policies` cards: `Global default` first, then `Model: <route>` and `MCP: <server>` overrides.
- `guardrails-decisions` lists decision events with UTC time, phase, action badge, evaluator, pack/rule, reason, latency, and decision ID.
- `guardrails-filter` narrows decisions by `Request ID`, `Evaluator`, phase, and action through URL search.
- `guardrails-page` pages through decisions when the total exceeds the page size (default 50).

## How to get to it (user POV)

- Sign in as a platform administrator and choose `Guardrails` under `Observability`.
- Open `/admin/observability/guardrails` directly. Filters are URL search params, for example `?action=audit&phase=harness_pre_tool`.

## Driving it with control-oceans-admin

Preconditions:

- `control-oceans-admin doctor` passes.
- You are signed in as the seeded `admin@local`. The sidebar entry needs both the `mcp_invocations` page and the `platform_admin` role, and both admin endpoints reject non-platform-admins.
- The demo seed creates no decisions. To get a row without any upstream call, create a temporary user API key (see [API keys](./api-keys.md)), then call `POST /api/v1/guardrails/evaluate` with `{"tool_name":"bash","command":"rm -rf /tmp/oceans-verify"}`. The command is evaluated, never executed. Revoke the key afterwards.

- **Open.** Choose the `Guardrails` link. The `Guardrails` heading, `Effective policies` card, and `Decision events` card are visible.
- **Policy.** With the default `gateway.yaml`, a single `Global default` card shows `Enabled`, `audit`, the configured packs, `Managed checks: None`, and `Stream buffer: 4,194,304 bytes`.
- **Filter.** Fill `Request ID` or `Evaluator`, or choose options such as `Harness pre-tool` and `Audit` in the phase and action selects, then choose `Apply filters`. The URL search updates and `Showing N of TOTAL decisions.` changes.
- **Clear.** Choose `Clear`. The URL search empties and every field resets.
- **Decision row.** After the evaluate call, a row appears with phase `harness_pre_tool`, action `audit`, rule `core.filesystem / recursive-force-remove`, and reason `filesystem.recursive_force_remove`.
- **API parity.** With the same session, fetch `GET /api/v1/admin/guardrails/policies` and `GET /api/v1/admin/guardrails/decisions` with the same filter query. `data.default` must match the `Global default` card. `data.total` and the `decision_id` values must match the table and the `Showing` line.
- **Proof.** Capture the policy card, the filtered table, and both API responses. Record the decision ID and filter query.

## Gotchas

- After `Apply filters` the URL changes before the table re-renders. Wait for the `Showing` line to change before you compare it with the API, or you will read the unfiltered total.
- The phase and action selects have no programmatic label; their accessible name is the displayed value (`All phases`, `All actions`). Find them by that text inside `Decision events`.
- Filters apply only through `Apply filters`. Editing a field on its own changes neither the URL nor the table.
- `No guardrail decisions` is the correct fresh-seed state, not a routing or permission fault.
- The seed does not clear decisions, so a reused `gateway.db` carries totals from earlier runs. Filter by request ID or decision ID for exact proof.
- Direct `/api/v1/guardrails/evaluate` decisions have no request ID. A request-linked `generated_tool_call` decision needs the paid [Backend gateway](./backend-gateway.md) recipe.
- Team admins and users hold `mcp_invocations`, but the sidebar link is still hidden from them.
