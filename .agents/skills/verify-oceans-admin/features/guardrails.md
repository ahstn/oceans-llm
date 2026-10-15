# Guardrails

Guardrails is a platform-admin-only view of the privacy-safe history of guardrail decisions, with a separate `Guardrail Settings` page for the effective policy. The decision view summarizes decisions by phase and action, filters them by request ID, evaluator, phase, and action, and opens one decision in a detail sheet. It never shows raw prompts, commands, arguments, or results. Settings only redrafts a `gateway.yaml` preview; nothing on either page changes policy.

## Sub-features

- `guardrails-summary` shows four tiles and `Action mix by phase`. Only `Decisions` counts every decision that matches the filters. `Denied`, `Transformed`, `p95 latency`, and the phase mix cover only the decisions on the current page, so they differ from filter-wide totals once more than one page matches.
- `guardrails-decisions` lists `Decision events` with UTC time, action badge, phase, reason and pack/rule, evaluator, and latency.
- `guardrails-filter` narrows decisions by `Request ID`, `Evaluator`, phase, and action through URL search (`request_id`, `evaluator`, `phase`, `action`).
- `guardrails-detail` opens a decision sheet titled with its phase and action badge, showing `Reason`, `Rule`, `Evaluator`, `Scope`, `Latency`, `Transformed`, `Decision`, and `Request` when the decision is request-linked.
- `guardrails-page` pages through decisions when the total exceeds the page size (default 50).
- `guardrails-settings` is the `Guardrail Settings` page at `/admin/guardrails`. Its `Destructive actions`, `Secret redaction`, and `Integrations` cards each open a dialog through `Manage <area>`, and edits only redraft the `gateway.yaml` preview below the cards (`Copy YAML`). Nothing is saved, so prove that the preview changes and that `/api/v1/admin/guardrails/policies` does not.

## How to get to it (user POV)

- Sign in as a platform administrator and choose `Guardrails` under `Observability`.
- Open `/admin/observability/guardrails` directly. Filters are URL search params, for example `?action=audit&phase=harness_pre_tool`.
- Choose `Configure` on the Guardrails page, or open `/admin/guardrails`, for `Guardrail Settings`. Its `Activity` button returns to the decision view.

## Driving it with control-oceans-admin

Preconditions:

- `control-oceans-admin doctor` passes.
- You are signed in as the seeded `admin@local`. The sidebar entry needs both the `mcp_invocations` page and the `platform_admin` role, and both admin endpoints reject non-platform-admins.
- The demo seed creates no decisions. To get a row without any upstream call, create a temporary user API key (see [API keys](./api-keys.md)), then call `POST /api/v1/guardrails/evaluate` with `{"tool_name":"bash","command":"rm -rf /tmp/oceans-verify"}`. The command is evaluated, never executed. Revoke the key afterwards.

- **Open.** Choose the `Guardrails` link. The `Guardrails` heading, the `Decisions` tile, `Action mix by phase`, and `Decision events` are visible.
- **Decision row.** After the evaluate call, a row shows action `Audit`, phase `Harness pre-tool`, reason `filesystem.recursive_force_remove`, rule `core.filesystem / recursive-force-remove`, and evaluator `built_in`.
- **Filter.** Type `built_in` in `Evaluator` and press Enter, then choose `Audit` in the `Action` group. Choose `Harness pre-tool` under `Action mix by phase` to filter by phase; the button shows as pressed. Each step updates the URL search (`evaluator=built_in&action=audit&phase=harness_pre_tool`) and reloads the decisions. The `Decisions` tile and the pager's `a–b of N` line show the filtered server total, which can stay the same, for example `1` and `1–1 of 1` when the one seeded decision matches every filter.
- **Detail.** Select the row. The sheet title reads `Harness pre-tool` with an `Audit` badge. `Reason`, `Rule`, and `Evaluator` match the row, and `Decision` shows the decision ID, which the grid does not.
- **Clear.** Choose `Clear` in the toolbar. It appears only while a filter is set. The URL search empties, `Action` returns to `All`, and the phase button is no longer pressed. The `Clear evaluator` and `Clear request id` buttons inside the text fields remove only that one filter.
- **Policy.** Choose `Configure`. `Guardrail Settings` shows the `gateway.yaml` preview. With the default config it reads `enabled: true`, `mode: audit`, the configured packs, `managed_checks: []`, and `stream_buffer_bytes: 4194304`.
- **API parity.** With the same session, fetch `GET /api/v1/admin/guardrails/decisions` with the same filter query and `GET /api/v1/admin/guardrails/policies`. `data.total` must match the `Decisions` tile and the pager. The grid does not show decision IDs, so match each row to an API item by time, action, phase, rule, and evaluator, and check that the open detail sheet's `Decision` equals that item's `decision_id`. In `data.default`, compare `enabled`, `mode`, `packs`, `managed_checks`, `stream_buffer_bytes`, `stream_buffer_timeout_ms`, and `secret_redaction` with the preview's default block. `scope` and `overridden_fields` exist only in the API, and the top-level `managed_checks` definitions are not part of the preview.
- **Proof.** Capture the unfiltered view, the filtered table, the detail sheet, and the settings preview. Record the decision ID and filter query.

## Gotchas

- There is no apply button. `Request ID` and `Evaluator` apply on Enter. The `Action` group and the phase buttons apply on click. The URL changes before the table re-renders. The text fields, the `Action` group, and `Clear` are disabled until the new page loads, so wait for the URL and for those controls to be enabled again before you compare with the API. Do not wait for the `Decisions` tile or pager to change: on a fresh run with one decision, every filter in this recipe leaves them at `1` and `1–1 of 1`.
- `Evaluator` matches the evaluator name (`built_in` for the packaged rules), not a pack such as `core.filesystem`. A pack name returns `No guardrail decisions`. Clicking an evaluator cell also filters by it.
- The `Action` group is a single-select toggle group (`All`, `Allow`, `Audit`, `Transformed`, `Deny`), so each option has role `radio`. This page has no phase select. Phase filtering uses the `Action mix by phase` buttons, whose counts cover only the current page.
- Field-level `Clear evaluator` and `Clear request id` buttons also start with `Clear`. Select the toolbar button by its exact name.
- `No guardrail decisions` is the correct fresh-seed state, not a routing or permission fault.
- The seed does not clear decisions, so a reused `gateway.db` carries totals from earlier runs. There is no decision-ID filter, so find the proof's recorded `decision_id` in the returned rows. When you need request-ID filtering, use a request-linked decision from the [Backend gateway](./backend-gateway.md) recipe.
- Direct `/api/v1/guardrails/evaluate` decisions have no request ID. A request-linked `generated_tool_call` decision needs the paid [Backend gateway](./backend-gateway.md) recipe.
- Team admins and users hold `mcp_invocations`, but the sidebar link is still hidden from them.
