# Leaderboard

The Leaderboard ranks users by priced or legacy-estimated spend for a selected observability window and shows each user's most-used model, most-used harness, request volume, and average tool counts. Requests without priced usage still count toward request volume. The `User costs over time` chart plots the top 5 users in 12-hour UTC buckets; the `Top Users` ranking lists up to 30 users. With no data, both cards show `No leaderboard data yet`.

## Sub-features

- `leaderboard.7d-api-parity` — The default 7-day desktop ranking matches the production leaderboard API.
- `leaderboard.model` — Each user shows the single most-used model key.
- `leaderboard.harness` — Each user shows the label of the most-used normalized agent harness.
- `leaderboard.31d` — Selecting Last 31 days refreshes the chart and ranking from the 31-day API view.
- `leaderboard.responsive` — Mobile cards replace the desktop table below the `md` breakpoint.

## How to get to it (user POV)

- Open `/admin/observability/leaderboard` directly. An unauthenticated visit redirects to password sign-in and returns to the Leaderboard after authentication.
- From another authenticated admin page, open `Budget & Spending` in the sidebar and select `Leaderboard`.

## Driving it with control-oceans-admin

Preconditions: launch a dedicated verification stack, require `doctor` to pass, and use the seeded `admin@local` platform administrator. Every role has the `leaderboard` page.

1. Run `control-oceans-admin drive observability`; the driver opens the protected Leaderboard route and signs in through the visible `Sign in` form.
2. Wait for the `Leaderboard` heading and `leaderboard-table`. The desktop headers must include `Most used model` and `Most used harness`.
3. Read `/api/v1/admin/observability/leaderboard?range=7d` through the authenticated browser session. Every `leaderboard-table` row must match the API rank, user, spend, model key, harness label, request count, and all four average tool counts.
4. Require the number of rendered chart areas to match `chart_users`, then capture `02-leaderboard-7d.png` and `02-leaderboard-7d.aria.txt`.
5. Select the radio named `Last 31 days` (visible text `31d`). Wait until every rendered row, including average tool counts, matches `/api/v1/admin/observability/leaderboard?range=31d`, then capture `03-leaderboard-31d.png` and `03-leaderboard-31d.aria.txt`.
6. Resize to 600×1000. Require `leaderboard-mobile-list` visible and `leaderboard-table` hidden, capture `03b-leaderboard-mobile`, then restore 1440×1000.
7. Require `observability-proof.json` to retain the API leaders, rendered headers and rows, window boundaries, series counts, gateway version, and action log.

## Gotchas

- The route always loads 7 days first; do not infer a 31-day refresh from the selected radio alone. Wait for row values from the 31-day API response.
- `leaderboard-mobile-list` is in the DOM but hidden (`md:hidden`) at 1440px. Use `leaderboard-table` for API comparison. The responsive step checks visibility at 600px, not DOM presence.
- The chart is spend-only. Most-used model and harness values belong to ranking rows and must not be inferred from chart labels.
