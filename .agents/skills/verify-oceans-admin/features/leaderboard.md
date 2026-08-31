# Leaderboard

The Leaderboard ranks users by spend for a selected observability window and shows each user's most-used model, most-used harness, request volume, and average tool counts.

## Sub-features

- `leaderboard.7d-api-parity` — The default 7-day desktop ranking matches the production leaderboard API.
- `leaderboard-model` — Each user shows the single most-used model key.
- `leaderboard-harness` — Each user shows the label of the most-used normalized agent harness.
- `leaderboard.31d` — Selecting Last 31 days refreshes the chart and ranking from the 31-day API view.
- `leaderboard-responsive` — Mobile cards replace the desktop table below the `md` breakpoint.

## How to get to it (user POV)

- Open `/admin/observability/leaderboard` directly. An unauthenticated visit redirects to password sign-in and returns to the Leaderboard after authentication.
- From another authenticated admin page, open `Budget & Spending` in the sidebar and select `Leaderboard`.

## Driving it with control-oceans-admin

Preconditions: launch a dedicated verification stack, require `doctor` to pass, and use the seeded `admin@local` platform administrator.

1. Run `control-oceans-admin drive observability`; the driver opens the protected Leaderboard route and signs in through the visible `Sign in` form.
2. Wait for the `Leaderboard` heading and `leaderboard-table`. The desktop headers must include `Most used model` and `Most used harness`.
3. Read `/api/v1/admin/observability/leaderboard?range=7d` through the authenticated browser session. Every `leaderboard-table` row must match the API rank, user, spend, model key, harness label, and request count.
4. Require the number of rendered chart areas to match `chart_users`, then capture `02-leaderboard-7d.png` and `02-leaderboard-7d.aria.txt`.
5. Select the radio named `Last 31 days`. Wait until the rendered user and request-count rows match `/api/v1/admin/observability/leaderboard?range=31d`, then capture `03-leaderboard-31d.png` and `03-leaderboard-31d.aria.txt`.
6. Require `observability-proof.json` to retain the API leaders, rendered headers and rows, window boundaries, series counts, gateway version, and action log.

## Gotchas

- The route always loads 7 days first; do not infer a 31-day refresh from the selected radio alone. Wait for row values from the 31-day API response.
- `leaderboard-mobile-list` is present but hidden at the driver's desktop viewport. Use `leaderboard-table` for deterministic API comparison and verify responsive behavior in route tests.
- The chart is spend-only. Most-used model and harness values belong to ranking rows and must not be inferred from chart labels.
