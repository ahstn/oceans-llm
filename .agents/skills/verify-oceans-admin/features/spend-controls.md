# Spend controls

Spend Controls lets a platform administrator compare current-window spend against budgets for users, service accounts, and per-user model scopes. From here they can configure or remove each budget, temporarily increase a user budget that is within 15% of its limit, and read the latest budget alerts. Team owners and admins get a read-only view of their own team, where they can also increase their members' budgets.

## Sub-features

- `spend-summary` shows the `Spend against budgets`, `Budgeted owners`, `Over budget`, and `Near limit` tiles.
- `spend-users` lists user budgets, with search, filters, sort, and 15 rows per page.
- `spend-service-accounts` lists service-account budgets and whether their alert recipients are ready.
- `spend-model-budgets` lists and adds per-user budgets scoped to a `Model id` or `Upstream model`.
- `spend-configure` edits a single budget in the `Configure budget` dialog.
- `spend-remove` deactivates a budget, with no confirmation step.
- `spend-alerts` shows the latest 10 threshold alerts under `Alert history`.
- `spend-increase` adds a temporary top-up in the `Increase budget` dialog. Every user row has the button. It is enabled only when spend is at least 85% of the effective limit and the viewer is a platform admin or the user's team owner/admin. The dialog has a `Period` toggle (`End of month` is the default, or `7 days`), four prefill buttons at 50/100/150/200% of the budget, and an `Increase amount (USD)` input capped at 3× the budget. An active increase shows as a `+$X until <date>` badge in the `Budget` column and raises the `of $Y` limit under `Usage`.
- `spend-team-admin` is the team-admin view: only the admin's team's users and service accounts, no `Configure`/`Remove`, no `Add model budget`, and no `Alert history`.

## How to get to it (user POV)

- Sign in as a platform administrator and choose `Spend Controls` under `Budget & Spending`.
- Sign in as a team admin (for example `cara@research.local`, password `localdemo123`). `Spend Controls` appears because `gateway.yaml` grants `team_admins` the `spend_controls` page.
- Open `/admin/spend-controls` directly.

## Driving it with control-oceans-admin

Preconditions:

- `control-oceans-admin doctor` passes.
- `mise run dev-stack` completed its seed and config sync. Budgets come only from `gateway.yaml`: Alice $12 weekly (soft), Diego $50 daily, Erin $150 monthly, Gina $200 monthly, Grace $150 monthly (soft), Heidi $40 weekly, Judy $100 monthly, `local-ci-runner` $25 daily, `local-eval-worker` $15 daily.
- The demo seed tops up current-window spend so that Gina is at 104% (`Over budget`), Erin 92%, Grace 88%, and Heidi 86% (`Near limit`, increase enabled), and Judy 62% (increase disabled).
- You are signed in as the seeded platform admin `admin@local`.

- **Open.** Choose `Spend Controls`. The `Spend controls` heading, the four summary tiles, the `Budgets` card, and the `Alert history` card are visible.
- **Users tab.** The `Users (N)` tab is selected, with columns `User`, `Budget`, `Usage`, `Status`, `Alerts`, and `Actions`. The pager reads `Showing a–b of N users`.
- **Filter.** Open `Filters`, uncheck `Hide quiet users`, and choose `With budget` in the `Budget state` group. `Diego Research Analyst` shows `$50.0000`, `Daily`, `Hard`, and `Config (user)`. `Clear filters` puts the defaults back.
- **Sort.** In the `Sort users` combobox, choose `Name, A to Z`.
- **Service accounts tab.** Choose `Service accounts (2)`. `Local CI Runner` has a $25 daily budget and `Local Eval Worker` a $15 daily budget, and both carry the `Config (service account)` badge.
- **Model budgets tab.** Choose `Model budgets (0)`. It reads `No user model budgets are configured`.
- **Safe mutation.** Under `Add model budget`, choose a `User`, select `Model id` under `Scope`, choose a `Model`, set `Amount (USD)` to `1000`, and choose `Add model budget`. The toast reads `User model budget created` and a `model:<key>` row appears. Reload and confirm the row is still there. Clean up with `Remove`: the toast reads `User model budget removed` and the tab count goes back to `(0)`.
- **Increase budget.** On `Erin Research Analyst`, `Increase budget` is enabled. On `Judy Operations Analyst` it is disabled, and hovering it reads `Available once spend is within 15% of the limit.` Open Erin's dialog. `End of month` is pressed, the prefill buttons read `$75.00`, `$150.00`, `$225.00`, and `$300.00`, and the help text reads `Up to $450.00 (3× the budget).` Typing `451` shows `The increase cannot exceed $450.00 (3× the budget).` Choose `$75.00`, then `7 days`, then `Increase budget`. The toast reads `Budget increased for Erin Research Analyst`. The row now shows `+$75.00 until <date>`, and `Usage` reads `of $225.00`. Erin drops to about 61%, so her button is disabled again.
- **Team admin view.** Sign in as `cara@research.local`. Only Research users appear. `Erin` and `Gina` have `Increase budget` enabled, no row has `Configure` or `Remove`, and `Alert history` is absent. As `frank@applied.local`, Grace's button is enabled and no Research users are listed.
- **API parity.** In the same session, fetch `GET /api/v1/admin/spend/budgets`. For every visible row, compare the name, `budget.amount_usd`, `cadence`, `hard_limit`, `budget_source.kind`, and scope count with `users`, `service_accounts`, and `user_model_budgets`. Then fetch `GET /api/v1/admin/spend/budget-alerts?page=1&page_size=10&owner_kind=all&status=all&channel=all` and compare it with `Alert history`. Each user's `active_increase` (or `null`) matches the badge in the `Budget` column. `POST /api/v1/admin/spend/budgets/increases` returns 403 for a team admin targeting another team's user and 400 when spend is below 85% or the amount exceeds 3× the budget.
- **Proof.** Capture the default list, the filtered list, the saved model budget on a second view, and the state after cleanup.

## Gotchas

- Never save or remove a config-sourced budget (Diego or either service account). Saving turns it into `Manual`. Removing leaves a `manual/deactivated` row that config sync will not overwrite. Rerunning `dev-stack` restores neither change.
- User-budget mutations are safe only on a user whose budget reads `Not set`. Use an amount of `1000` or more so the save does not raise an alert, then clean up with `Remove`.
- `Remove` acts immediately, with no confirmation.
- There is no endpoint to revoke an increase. It lasts until its expiry, survives reseeds and `Configure`, and a newer increase on the same budget replaces it. To get back to the seeded state, use `7 days` on a seeded user, or reset the dev database.
- `Remove` on `Local CI Runner` fails while it has active service-account API keys.
- `Hide quiet users` is on by default and hides budgeted users below 20% usage, so Diego may not appear until you clear it.
- The seed creates no model budgets and no alerts, so `No budget alerts have been recorded yet.` is a valid result.
- Seeded ledger rows are timestamped relative to now, so usage percentages shift with the time of day.
