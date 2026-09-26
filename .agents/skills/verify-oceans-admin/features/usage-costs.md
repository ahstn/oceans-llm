# Usage costs

Usage Costs is a read-only spend dashboard for the signed-in user over a 7- or 30-day UTC window. It shows priced spend, request, pricing-coverage, and peak-day KPIs, a daily spend trend, model mix, top models and owners by spend, and cache efficiency by owner. Platform administrators see every owner and can narrow the report to user owners or service accounts; everyone else sees only their own spend. The page also offers a FOCUS CSV billing export for the window or for a single UTC day.

## Sub-features

- `usage-kpis` shows the `Priced spend`, `Requests`, `Pricing coverage`, and `Peak day` cards.
- `usage-trend` plots priced spend per UTC day in `Spend trend`, with uncached input, cache read, and cache write token totals.
- `usage-window` switches the report between the last 7 and the last 30 days.
- `usage-owner-filter` limits a platform-admin report to all owners, user owners, or service accounts.
- `usage-breakdowns` shows `Model mix`, `Spend by model`, `Spend by owner`, and `Cache efficiency by owner` (top 10 each), with warning badges for unpriced or usage-missing gaps.
- `usage-export` downloads a FOCUS CSV for the window or for one UTC day.

## How to get to it (user POV)

- Sign in and choose `Usage Costs` under `Budget & Spending`.
- Open `/admin/observability/usage-costs` directly.
- Non-admin users land here after sign-in because `usage_costs` is their `default_page`.

## Driving it with control-oceans-admin

Preconditions:

- `control-oceans-admin doctor` passes.
- `mise run dev-stack` completed its local demo seed, which writes priced and usage-missing ledger rows 0-6 days back.
- You are signed in as the seeded `admin@local` platform administrator. `usage_costs` comes from the `users` page set, which platform administrators inherit.

- **Open.** Choose the `Usage Costs` link. The `Usage costs` heading appears under the `Budget & Spending` section label. The four KPI cards show values, and `Spend trend` shows a chart, not `No priced spend yet`.
- **Window.** In the `Report window` group, press the `Last 30 days` toggle (visible text `30d`). `Refresh` briefly reads `Refreshing...` and the controls are disabled. Once it settles, `30d` is pressed and `Priced spend` is at least the 7-day value.
- **Owner filter.** Open the `Owner filter` combobox and choose `Service accounts`. `Spend by owner` then lists only service accounts, such as `Local CI Runner`. Switch back to `All owners`.
- **Export menu.** Open `Export FOCUS CSV`. The menu shows the `Billing export` label, `Export last N days`, a `Single UTC day` date input, and an `Export` button. Unless a download was approved, close the menu with Escape.
- **API parity.** With the same session, fetch `GET /api/v1/admin/spend/report?days=7&owner_kind=all`. `data.totals.priced_cost_usd_10000 / 10000` must match `Priced spend`. The priced, unpriced, and usage-missing counts must sum to `Requests`. The first `data.models` entry ranked by `priced_cost_usd_10000` must match the top `Spend by model` row.
- **Proof.** Capture the 7-day default, the 30-day or service-account view, and the API totals.

## Gotchas

- The window and owner filter live in component state, not the URL. A reload resets them to `7d` and `All owners`.
- Non-platform-admin sessions don't get the `Owner filter` combobox. They see a self-only report and export through `/api/v1/me/spend/focus.csv`.
- The seed includes usage-missing requests, so `Pricing coverage` can fall below 100% and turn warning-styled under 95%. That is expected, not a fault.
- Days are UTC. Near UTC midnight, the seeded day-0 rows can land in a different chart bucket than local time suggests.
- The export is a browser navigation to the gateway origin, not the UI port. Prove it from the menu contents, or from a same-session fetch that returns HTTP 200, not from a saved file.
