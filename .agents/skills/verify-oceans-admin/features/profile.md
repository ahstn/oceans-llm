# Profile

Profile is the landing page after sign-in for any session with page permissions. It greets the user, shows their budget meter, headline usage, a usage heatmap with per-day tooltips, and their personal API keys, with links into the API Keys create dialog.

## Sub-features

- `profile-landing` sends a sign-in without a `redirect` target to `/admin/profile`.
- `profile-budget` shows the budget meter with the spent and limit amounts from `/api/v1/me/profile`.
- `profile-usage` shows headline usage and a heatmap whose tooltip lists `Requests`, `Total tokens`, and `Cost`.
- `profile-keys` lists personal keys, expands with `Show N more`, and links `Manage keys` to API Keys.
- `profile-create-key` opens `Create API key` from `/admin/api-keys?create=true` with the viewer preselected as `Owner user`.
- `profile-empty-keys` shows `Create your first key` for a user with no personal keys.

## How to get to it (user POV)

- Sign in from `/admin/login` with no `redirect`.
- Choose `Profile` in the sidebar.

## Driving it with control-oceans-admin

Preconditions: a launched stack with the local demo seed. The default user is `alice@platform.local` / `localdemo123`; override with `OCEANS_VERIFY_PROFILE_EMAIL` and `OCEANS_VERIFY_PROFILE_PASSWORD`.

- **Automated proof.** Run `control-oceans-admin drive profile`. It writes `profile-proof.json` with one check per step below.
- **Landing.** Sign in as Alice. The URL is `/admin/profile` and the heading reads `Welcome back, Alice`.
- **API parity.** Fetch `/api/v1/me/profile`. The `budget-meter` test ID shows the same spent and limit amounts, and the seed has at least 100 active days.
- **Heatmap.** Hover the last active cell in `usage-heatmap`. The `heatmap-tooltip` lists `Requests`, `Total tokens`, and `Cost`.
- **Create link.** Open `/admin/api-keys?create=true`. The `Create API key` dialog shows `Alice Platform Lead` as `Owner user`.
- **Empty keys.** Sign in as `admin@local`. `Create your first key` is visible and opens the `Create API key` dialog.

## Gotchas

- Charts load lazily and animate in; wait for network idle and the first `.recharts-surface` before reading the heatmap.
- `admin@local` has no personal keys and is not a selectable key owner, so only the dialog opening is checked for it.
