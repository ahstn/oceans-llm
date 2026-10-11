# Agent sessions

Agent Sessions lets the configured platform administrator review seeded agent runs, filter them by ownership and execution data, and inspect outcome, cost, activity, tool use, and available analysis or data-quality details.

## Sub-features

- `sessions-list` shows seeded sessions, a total count, and per-session metrics.
- `sessions-filter` narrows the list by harness, model, state, owner, tags, coverage, or start date (`Started after` / `Started before`).
- `sessions-open` opens a session detail sheet from a table row.
- `sessions-page` changes row count (25, 50, or 100) and moves between result pages.
- `sessions-request-link` opens a request from the detail event stream (`Open request <id> in request logs`, platform admins only).

## How to get to it (user POV)

- Sign in and choose `Agent Sessions` under `Observability`.
- Open `/admin/observability/agent-sessions` directly.
- Select any session table row to open its details. This sets `?session_id=`, which can be deep-linked.
- Choose `Clear` in `Filters` to drop every filter.

## Driving it with control-oceans-admin

Preconditions:

- `control-oceans-admin doctor` passes.
- `mise run dev-stack` completed its local demo seed.
- Sign in as the seeded `admin@local` platform administrator. The page needs the `agent_sessions` page in `gateway.yaml` and agent-analysis access. `dev-stack` grants that access by exporting `AGENT_ANALYSIS_SHADOW_DIAGNOSTICS_ENABLED=true`. Without it, the nav item disappears and the API returns 403.

- **Open list.** Choose the `Agent Sessions` link. The `Agent sessions` heading, `Session explorer`, and a session count badge are visible.
- **Filter.** Expand `Filters`. Fill the `Harness`, `Model`, or another labeled field, or choose a value under `Session state`, `Outcome`, `Score maturity`, or `Score confidence`. The URL search changes and the table settles with `aria-busy=false`.
- **Open detail.** Select a session row by clicking its visible harness or model cell. A sheet headed `Agent session details` opens and shows the selected session ID.
- **Inspect proof.** Expand the `Session identity` button, which starts collapsed. Confirm its `Model` and `Harness` rows match the list row. Locally, expect `Score not shown` and the `Calibration data` alert, because `calibrated_score_enabled` is false. `Event stream` is open by default. `Tool exposure`, `Score components`, `Token and cache use`, `Tools and changes`, and `Prompt context` are collapsible.
- **API parity.** In the same session, fetch `GET /api/v1/admin/observability/agent-sessions?page=1&page_size=50&<filters>` and `GET /api/v1/admin/observability/agent-sessions/{session_id}`. `total` must match the badge and pager label. `session.requested_model_key` and `session.harness_label` must match the sheet.
- **Pagination.** Use `Rows per page`, `Previous`, `Next`, or `Go to page N`. Confirm the `first - last of total` label changes.
- **Proof.** Capture the unfiltered list, a filtered result, and the matching detail sheet. Record the filter query and selected session ID.

## Gotchas

- `Filters` is a native `<details>` disclosure (ARIA role `group`), not a button. Click its `summary` text to expand it.
- Scores can be hidden until calibration is complete. `Score not shown` is a valid configured state.
- The demo worker can update report state after startup. Wait for the expected row or detail state, not a fixed delay.
- Table rows are keyboard accessible but are not links. Use the visible row text or press Enter on the focused row.
- The four select filters have no accessible name. Find their triggers by the visible label text, not by role name.
- Empty results show `No agent sessions match these filters.` A detail load failure shows `Session details are not available` with `Retry`.
- Page access depends on the permission set in `gateway.yaml`, agent-analysis access, and the signed-in role.
- If the page is absent, inspect the signed-in session's permissions before you diagnose routing or seeded-data faults.
