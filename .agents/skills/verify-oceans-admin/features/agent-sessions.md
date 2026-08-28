# Agent sessions

Agent sessions lets an authorized user review seeded agent runs, filter them by ownership and execution data, and inspect outcome, cost, activity, tool use, and data-quality details.

## Sub-features

- `sessions-list` shows seeded sessions and summary metrics.
- `sessions-filter` narrows the list by harness, model, state, owner, tags, or date.
- `sessions-open` opens a session detail sheet from a table row.
- `sessions-page` changes row count and moves between result pages.

## How to get to it (user POV)

- Sign in and choose `Agent Sessions` under `Observability`.
- Open `/admin/observability/agent-sessions` directly.
- Select any session table row to open its details.

## Driving it with control-oceans-admin

Preconditions:

- `control-oceans-admin doctor` passes.
- `mise run dev-stack` completed its local demo seed.
- The signed-in session includes the `agent_sessions` page permission.
- The current `gateway.yaml` grants no permission group the `agent_sessions` page. Under the unchanged baseline, this feature is verified-unreachable: the sidebar omits `Agent Sessions`, and `/admin/observability/agent-sessions` returns the user to the configured default page.

- **Open list.** Choose the `Agent Sessions` link. The `Agent sessions` heading, `Session explorer`, and a session count badge are visible.
- **Filter.** Expand `Filters`. Fill the `Harness`, `Model`, or another labeled field, or choose a value under `Session state`, `Outcome`, `Score maturity`, or `Score confidence`. The URL search changes and the table settles with `aria-busy=false`.
- **Open detail.** Select a session row by clicking its visible harness or model cell. A sheet headed `Agent session details` opens and shows the selected session ID.
- **Inspect proof.** When the page permission is configured, confirm that the detail contains calibration or data-quality content and that the list row's model and harness agree with `Session identity` in the detail.
- **Pagination.** Use `Rows per page`, `Previous`, `Next`, or `Go to page N`. Confirm the `first - last of total` label changes.
- **Proof.** Capture the unfiltered list, a filtered result, and the matching detail sheet. Record the filter query and selected session ID.

## Gotchas

- Scores can be hidden until calibration is complete. `Score not shown` is a valid configured state.
- The demo worker can update report state after startup. Wait for the expected row or detail state, not a fixed delay.
- Table rows are keyboard accessible but are not links. Use the visible row text or press Enter on the focused row.
- Page access depends on the permission set in `gateway.yaml` and the signed-in role.
- Do not add `agent_sessions` to `gateway.yaml` during a maintenance run. Record the attempted route and missing page permission as the concrete prerequisite.
