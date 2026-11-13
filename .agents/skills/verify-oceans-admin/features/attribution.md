# Attribution

Attribution is a read-only report that joins Git AI authorship notes with gateway traffic over a 7-, 30-, or 90-day window. It shows how many lines of each commit AI wrote, which merged pull requests those commits reached, and the agent requests and priced cost behind them. Platform administrators see every user and can narrow the report to one user or repository; everyone else sees only the commits they own or authored. The data arrives from `oceans hook` on developer machines, so the page also explains the hook install when it has nothing to show.

## Sub-features

- `attribution-kpis` shows the `Commits`, `Merged PRs`, `AI lines`, `Requests`, `Attributed cost`, and `Pricing coverage` cards.
- `attribution-window` switches the report between the last 7, 30, and 90 days through `?days=`.
- `attribution-repository-filter` narrows the report to one repository through the `Repository filter` combobox and `?repository=`.
- `attribution-user-filter` narrows a platform-admin report to one user: the `Filter the report to <name>` button in a `User activity` row sets `?user_id=`, and the `Clear user filter: <name>` button drops it.
- `attribution-models` lists the top 10 gateway models by requests in `Models`, with each model's share, priced cost, and gap badges.
- `attribution-pull-requests` lists merged pull requests in the `Merged pull requests` table, linked to the forge.
- `attribution-changes` lists commits in the `Recent changes` table, with a joined or unjoined session badge and `Show more` past 50 rows.
- `attribution-empty` shows the hook install steps when the window has no data, and `Clear filters` when only the filters exclude everything.

## How to get to it (user POV)

- Sign in and choose `Attribution` under `Observability`.
- Open `/admin/observability/attribution` directly. `?days=7|30|90`, `?repository=<host/owner/repo>`, and `?user_id=<uuid>` deep-link a filtered report.

## Driving it with control-oceans-admin

Preconditions:

- `control-oceans-admin doctor` passes.
- `mise run dev-stack` completed its local demo seed. Each run rewrites four commits at fresh relative times, in `github.com/oceans-demo/admin-console` and `github.com/oceans-demo/release-runbooks`:
  - `Gina Research Engineer` (`gina@research.local`) has an `admin-console` commit (about 55 minutes old) joined to her seeded Codex session, and a `release-runbooks` commit (about 150 seconds old) joined to another session that made 3 more requests after it.
  - `Alice Platform Lead` (`alice@platform.local`) has an `admin-console` commit (2 days old) whose Claude session is not joined, and owns a human-only commit (4 days old) whose author is not an Oceans user.
  - Two merged pull requests in `admin-console`: `#42` `Rename the observability route to sessions` by Gina, and `#41` `Summarise release blockers on the dashboard` by Alice. The `release-runbooks` commit has no pull request.
- You are signed in as the seeded `admin@local` platform administrator. The page shares Usage Costs' `usage_costs` page, which comes from the `users` page set that platform administrators inherit.

- **Open.** Choose the `Attribution` link. The `Attribution` heading appears under the `Observability` section label. With the seed, `Commits` reads `4` with `1 with an unmatched author`, `Merged PRs` reads `2`, and the `Requests` hint includes `3 uncommitted`.
- **Window.** In the `Report window` group, press the `Last 7 days` toggle (visible text `7d`). The URL gains `days=7`, `Refresh` reads `Refreshing...`, and the controls are disabled until the report loads. Once it settles, `7d` is pressed and the seeded totals are unchanged, because every seeded commit is under 7 days old. Press `Last 30 days` to return.
- **Repository filter.** Open the `Repository filter` combobox and choose `github.com/oceans-demo/release-runbooks`. The URL gains `repository=`, `Recent changes` shows only Gina's commit, and `Merged pull requests` reads `No merged pull requests in this window.` Choose `All repositories` to reset.
- **User filter.** In the `User activity` row for `Gina Research Engineer`, press the `Filter the report to Gina Research Engineer` button (visible text is her name). The URL gains `user_id=`, the button gets `aria-pressed="true"` and the row `aria-current="true"`, a `Clear user filter: Gina Research Engineer` button appears next to the window toggle, and `Commits` reads `2`. Press that button to drop the filter.
- **Tables.** In `Merged pull requests`, `github.com/oceans-demo/admin-console#42` is a link to the pull request. In `Recent changes`, each row shows a 7-character SHA, with the agent tool underneath when the note names one. Gina's `admin-console` commit shows a success `1 joined` badge and links `#42`. Alice's `admin-console` commit shows a warning `1 unjoined` badge. Alice's human-only commit reads `Unmatched author` with `Key owner: Alice Platform Lead` underneath.
- **Unknown cost.** Alice's unjoined commit has no priced request, so its `Cost` reads `—`, never `$0.00`. Rows with unpriced or usage-missing requests also show a warning `N gaps` badge. The `Attributed cost` card reads `—` when no request allocated to the window's commits was priced.
- **Empty states.** Open `/admin/observability/attribution?repository=github.com/example/none`. The page shows `No attribution matches these filters` and a `Clear filters` button, which drops both filters. A window with no data at all shows `No attributed commits yet`, the `oceans hook install --git-ai --claude` and `oceans hook install --git-ai --omp` commands, and a `Set up Git AI attribution` link to `https://oceans-llm.com/harnesses/git-ai-attribution/`.
- **API parity.** With the same session, fetch `GET /api/v1/admin/attribution/report?days=30`. `data.totals.commit_count`, `pull_request_count`, `ai_lines`, and `request_count` must match the `Commits`, `Merged PRs`, `AI lines`, and `Requests` cards. `data.totals.priced_cost_usd_10000 / 10000` must match `Attributed cost`, or the card reads `—` when `data.totals.priced_request_count` is 0. `data.totals.uncommitted_request_count` must match the `Requests` card's `N uncommitted` hint. `data.users` must have one entry per `User activity` row, and the first `data.changes[].commit_sha` must start with the first `Recent changes` SHA.
- **Proof.** Capture the default 30-day view, the Gina-filtered or `release-runbooks` view, and the API totals.

## Gotchas

- Unlike Usage Costs, the window and filters live in the URL, so a reload keeps them. The gateway defaults to 7 days, but the page always sends `days` and defaults to 30.
- Non-platform-admin sessions get a self-only report. Their `User activity` rows have no filter button, and the gateway ignores `user_id` for them. An empty self-only report shows the hook install steps, not `Clear filters`.
- The seed uses times relative to its run, so a stack seeded days ago can push Alice's 4-day-old commit out of the 7-day window. Rerun the seed rather than adjusting the expected counts.
- Outside the seed, notes reach Oceans from `oceans hook` on a developer machine: `oceans login`, `oceans hook install --git-ai --claude` (or `--omp`), then an agent-made commit. The CLI queues entries and sends them in the background; `oceans hook send --foreground` flushes the queue, and `oceans hook queue` shows pending and quarantined entries.
- Merged pull requests come only from `oceans hook prs`, which uses the local `gh`. There is no server-side GitHub token. A commit's `#N` links only when that PR is in the window's `Merged pull requests`.
- An unjoined session is one whose ID matched no gateway traffic. Its lines still count, but it adds no requests or cost.
- Cost sums priced requests only. `Pricing coverage` turns warning-styled under 95%, which is expected when the window holds unpriced or usage-missing requests.
- The report reads at most 1,000 commits, 1,000 pull requests and 1,000 hook-reported sessions. When it hits a read limit, it shows `This report is incomplete`; choose a shorter window or a filter.
