# Admin UI: bring lagging pages in line with DESIGN.md

## Outcome
The high-priority findings from the 27 Nov design walkthrough are fixed:

- MCP invocations and Batch requests filter with the toolbar and chip pattern that Agent Sessions uses. Neither page shows a native date input, `n/a` or a bare `1689ms`.
- MCP invocations' table fits its card or pins its action column, and shows times and owners rather than raw ID dumps.
- Request costs in the request log table read at four decimals.
- Attribution's spend axis shows real dollar ticks, and its User activity table never clips its last column.
- Audit log shows cards below `md`.

## Acceptance checks
- MCP invocations filters through a `Filters` menu with removable chips; status, policy and day fields pick from lists, and no native date input or `n/a` remains — e2e: mcp-invocations-filters `#mcp-filters`
- MCP invocations latency reads `formatDuration` units, right-aligned and tabular, and the table keeps its Inspect action visible at 1440 px — manual `#mcp-table`
- Batch requests filters through the same toolbar; days pick from recent days or a typed `YYYY-MM-DD`, and the page sits under Observability — e2e: batch-filters `#batch-filters`
- A request log row's cost reads four decimals (`$0.0010`, `<$0.0001`), with the exact amount in its tooltip and the detail panel — unit: format.test.ts `#request-cost`
- The Attribution "Spend against output" axis shows distinct dollar ticks for sub-dollar spend — unit: format.test.ts `#usd-axis`
- Attribution's User activity table shows its Last activity column in full at 1440 px — manual `#user-activity-fit`
- Audit log at 375 px shows one card per entry instead of a scrolling table — manual `#audit-mobile`
- The changed pages look right at 1440 px and 375 px (screenshots before and after) — manual `#screens`

## Approach
- **Shared filter toolbar.** Lift `SessionFilterToolbar` (`components/observability/agent-sessions/session-filter-toolbar.tsx`) into a generic `components/data/filter-toolbar.tsx`. It is parameterised by field groups, the params each field owns (one, or a key/value pair), a picker-options hook and a test-id prefix. The `dayOptions`, `customDay` and `dayPicker` helpers move with it. Agent Sessions keeps its behaviour and test ids by passing its own groups, its harness/model option loader and the `agent-session` prefix.
- **MCP invocations.** Use the toolbar with these groups:
  - Invocation: Request ID, Server, Target, Method (list of known MCP methods, typing allowed), Status and Policy (closed lists).
  - Owner: API key ID, User ID, Team ID.
  - Date: Occurred after, Occurred before (day picker).
  - The page applies each change to the URL straight away, with no Apply button.
  - The table gains a Time column (`formatTimestamp` with seconds). Owner shows the owner kind and one shortened mono ID, with the full IDs in its title. Latency uses `formatDuration`, right-aligned and tabular. Missing values read `—`.
  - The whole row opens the detail, so the Inspect button column goes. The table drops its `min-w-[84rem]` so it fits at 1440 px.
  - Detail headings and labels move to sentence case, and "Occurred at" is formatted.
- **Batch requests.** Use the toolbar with Status (closed list), User and Service account (lists from the loaded users and accounts, shown only when the viewer can filter across users) and Created after/before (day picker). Days are UTC days, as everywhere else in the console. The `PageHeader` section becomes "Observability".
- **Request cost.** Add `formatUsdNanosPrecise` (four decimals; `<$0.0001` or `-<$0.0001` for a non-zero amount below that). The request-log table and mobile card use it, with `formatUsdNanos` as the cell's title. The detail summary keeps the exact nanos.
- **Attribution axis.** Add `formatUsdAxis` (two to three decimals below $1,000, compact above) and use it on the spend axis.
- **User activity table.** Swap `overflow-hidden` with `min-w-[72rem]` for `overflow-x-auto` with a minimum width that fits the 1094 px card at 1440 px.
- **Audit log mobile.** Below `md`, `AuditLogTable` renders a list of cards (time, change badge, target, actor), each with a full-width `View details` button that opens the existing entry sheet. The grid stays `hidden md:block`.

## Decisions
- Request-log costs show four decimals in the list and keep exact nanos in the tooltip and detail. DESIGN.md's per-request precision rule wins for the scanning view, and the 22 Nov "keep sub-cent charges" work stays visible where a single request is read.
- Generalise the Agent Sessions toolbar rather than copy it. Three pages share the pattern, and a 470-line copy would drift.
- Batch day filters move from local days to UTC days, matching `formatTimestamp` and the Agent Sessions day picker.
- Medium and low findings (title casing, empty-value wording, badge casing, window toggles, Guardrails truncation and so on) are out of scope here and stay for a follow-up.

## Tasks
### Plan
- [x] Walk every admin page and list findings `#walkthrough`
- [x] Read the code behind each high-priority finding `#read-code`
### Build
- [x] Shared filter toolbar `#filter-toolbar`
  - [x] Move the toolbar and day helpers into components/data, generic over fields `#toolbar-generic`
  - [x] Point Agent Sessions at the shared toolbar with unchanged test ids `#toolbar-sessions`
- [x] MCP invocations page `#mcp-page`
  - [x] Replace the form filters with the shared toolbar `#mcp-toolbar`
  - [x] Table: time column, owner, latency, em dashes, row opens detail, fits 1440 px `#mcp-table-build`
  - [x] Detail panel: formatted time, sentence case, em dashes `#mcp-detail`
- [x] Batch requests page `#batch-page`
  - [x] Replace the batch filter bar with the shared toolbar `#batch-toolbar`
  - [x] Section label Observability `#batch-section`
- [x] Request cost at four decimals with exact tooltip `#cost-format`
- [x] Attribution spend axis formatter `#axis-format`
- [x] User activity table fits at 1440 px `#user-activity-build`
- [x] Audit log mobile cards `#audit-cards`
- [x] Update route tests, e2e specs and features recipes for changed filters `#tests-update`
- [x] Commit on fix/admin-ui-design-consistency `#commit`
### Test
- [x] Run mise run lint and the admin-ui unit tests `#lint-unit`
- [x] Run the affected Playwright specs `#e2e-run`
- [x] Verify changed pages in the browser at 1440 and 375 px with screenshots `#browser-verify`
- [x] Run the verify-oceans-admin plan and evidence `#verify-skill`
### Review
- [ ] Person reviews the outcome `#outcome-review`
### Merge review
- [ ] Person approves the merge `#merge-review`
