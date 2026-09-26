# Spend controls

Spend Controls lets a platform administrator compare current-window spend against budgets for users, service accounts, and per-user model scopes. From here they can configure or remove each budget and read the latest budget alerts.

## Sub-features

- `spend-summary` shows the `Spend against budgets`, `Budgeted owners`, `Over budget`, and `Near limit` tiles.
- `spend-users` lists user budgets, with search, filters, sort, and 15 rows per page.
- `spend-service-accounts` lists service-account budgets and whether their alert recipients are ready.
- `spend-model-budgets` lists and adds per-user budgets scoped to a `Model id` or `Upstream model`.
- `spend-configure` edits a single budget in the `Configure budget` dialog.
- `spend-remove` deactivates a budget, with no confirmation step.
- `spend-alerts` shows the latest 10 threshold alerts under `Alert history`.

## How to get to it (user POV)

- Sign in as a platform administrator and choose `Spend Controls` under `Budget & Spending`.
- Open `/admin/spend-controls` directly.

## Driving it with control-oceans-admin

Preconditions:

- `control-oceans-admin doctor` passes.
- `mise run dev-stack` completed its seed and config sync. Budgets come only from `gateway.yaml`: Diego $50 daily, `local-ci-runner` $25 daily, `local-eval-worker` $15 daily.
- You are signed in as the seeded platform admin `admin@local`.

- **Open.** Choose `Spend Controls`. The `Spend controls` heading, the four summary tiles, the `Budgets` card, and the `Alert history` card are visible.
- **Users tab.** The `Users (N)` tab is selected, with columns `User`, `Budget`, `Usage`, `Status`, `Alerts`, and `Actions`. The pager reads `Showing a–b of N users`.
- **Filter.** Open `Filters`, uncheck `Hide quiet users`, and choose `With budget` in the `Budget state` group. `Diego Research Analyst` shows `$50.0000`, `Daily`, `Hard`, and `Config (user)`. `Clear filters` puts the defaults back.
- **Sort.** In the `Sort users` combobox, choose `Name, A to Z`.
- **Service accounts tab.** Choose `Service accounts (2)`. `Local CI Runner` has a $25 daily budget and `Local Eval Worker` a $15 daily budget, and both carry the `Config (service account)` badge.
- **Model budgets tab.** Choose `Model budgets (0)`. It reads `No user model budgets are configured`.
- **Safe mutation.** Under `Add model budget`, choose a `User`, select `Model id` under `Scope`, choose a `Model`, set `Amount (USD)` to `1000`, and choose `Add model budget`. The toast reads `User model budget created` and a `model:<key>` row appears. Reload and confirm the row is still there. Clean up with `Remove`: the toast reads `User model budget removed` and the tab count goes back to `(0)`.
- **API parity.** In the same session, fetch `GET /api/v1/admin/spend/budgets`. For every visible row, compare the name, `budget.amount_usd`, `cadence`, `hard_limit`, `budget_source.kind`, and scope count with `users`, `service_accounts`, and `user_model_budgets`. Then fetch `GET /api/v1/admin/spend/budget-alerts?page=1&page_size=10&owner_kind=all&status=all&channel=all` and compare it with `Alert history`.
- **Proof.** Capture the default list, the filtered list, the saved model budget on a second view, and the state after cleanup.

## Gotchas

- Never save or remove a config-sourced budget (Diego or either service account). Saving turns it into `Manual`. Removing leaves a `manual/deactivated` row that config sync will not overwrite. Rerunning `dev-stack` restores neither change.
- User-budget mutations are safe only on a user whose budget reads `Not set`. Use an amount of `1000` or more so the save does not raise an alert, then clean up with `Remove`.
- `Remove` acts immediately, with no confirmation.
- `Remove` on `Local CI Runner` fails while it has active service-account API keys.
- `Hide quiet users` is on by default and hides budgeted users below 20% usage, so Diego may not appear until you clear it.
- The seed creates no model budgets and no alerts, so `No budget alerts have been recorded yet.` is a valid result.
- Seeded ledger rows are timestamped relative to now, so usage percentages shift with the time of day.
