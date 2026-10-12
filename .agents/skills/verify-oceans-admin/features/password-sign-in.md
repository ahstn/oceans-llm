# Password sign-in

Password sign-in lets a local user enter Oceans credentials, return to the requested protected page, confirm the authenticated identity, and end the session.

## Sub-features

- `auth-redirect` sends an unauthenticated protected route to sign-in with a redirect target.
- `auth-password` signs in with email and password.
- `auth-session` shows the signed-in identity in the application sidebar.
- `auth-sign-out` clears the session and returns to sign-in.
- `auth-forced-rotation` sends a user with `must_change_password` to `/admin/change-password` (heading `Change password`, alert `Rotation required`, fields `Current password`, `New password`, `Confirm new password`, button `Update password`); every protected route redirects there until it is done.
- `auth-default-page` sends a sign-in without a `redirect` target to `/admin/profile` for any session with page permissions; a session with none falls back to its default page or `/admin/no-access`.
- `auth-no-access` shows `No console pages available` at `/admin/no-access`, but only when the session has no pages. The default `gateway.yaml` cannot reach it.

## How to get to it (user POV)

- Open `/admin` or any protected `/admin/*` route while signed out.
- Open `/admin/login` directly.
- Open the identity menu in the sidebar and choose `Sign out` to end a session. The same menu offers `Change password`.

## Driving it with control-oceans-admin

Preconditions:

- `control-oceans-admin doctor` passes.
- `gateway.yaml` `auth.bootstrap_admin` creates the platform admin `admin@local` / `admin` with `require_password_change: false`.
- Use a new browser context so an old session cookie cannot bypass sign-in.

- **Protected entry.** Open `/admin/models`. The gateway passes the server's redirects through (`/admin/models` → `/admin/models?page=1&page_size=30` → sign-in), so the URL becomes `/admin/login?redirect=%2Fmodels%3Fpage%3D1%26page_size%3D30` and the heading `Sign in` is visible. The page must hydrate without React hydration errors.
- **Credentials.** Fill the `Email` field with `admin@local`, fill the exact `Password` field with `admin`, and choose the `Sign in` button.
- **Redirect result.** The browser returns to `/admin/models?page=1&page_size=30`. The `Oceans Gateway` sidebar identity and `Models` heading are visible.
- **Default page.** In a fresh context, sign in from `/admin/login` with no `redirect`. The browser lands on `/admin/profile` and the `Welcome back, <first name>` heading appears. Report `auth-no-access` as skipped; it needs `permissions.users.pages: []`.
- **Session confirmation.** Fetch `/api/v1/auth/session` from the browser context. The response reports `admin@local`, `platform_admin`, and `must_change_password: false`.
- **Sign out.** Open the button whose accessible name contains `admin@local`, then choose the `Sign out` menu item. The browser returns to `/admin/login`, and `/api/v1/auth/session` returns `data: null`.
- **Failure path (optional).** A wrong password shows an error toast and stays on `/admin/login`.
- **Proof.** Capture sign-in before submission, the requested page after submission, and the sign-out page. Record the session response without its cookie.

## Gotchas

- The login form defaults to `admin@local` and `admin`, but a driver must still fill both fields so the action is explicit.
- Bootstrap only seeds `admin@local` when no platform admin exists, so a reused `gateway.db` keeps any changed password or rotation flag. `mise run dev-stack` refreshes demo data but does not delete `gateway.db`.
- A signed-in visit to `/admin/login` redirects to the default page (`/admin/profile` for `admin@local`). Start each sign-in proof from a fresh browser context.
- The admin UI proxy must not follow upstream redirects. If a protected route answers `200` with the sign-in page instead of `307`, the proxy is serving the redirect target at the original URL, and React logs `Hydration failed because the server rendered HTML didn't match the client`.
- On the verification stack, `/admin` shows a Vite base-URL notice (404) and `/admin/` redirects to `/admin/login?redirect=%2F`, so prove the default page through sign-in instead of the index route.
- OIDC/OAuth buttons appear under `or continue with` only when providers are configured, and `?sso_error=` shows the alert `SSO sign in failed`. Neither is part of password sign-in.
- `/admin/no-access` is allowed only when a session has zero pages. Page lists merge across groups, so every seeded user is redirected away from it. `/admin/account-ready` and voluntary password change belong to [account onboarding](./account-onboarding.md).
- Do not record the session cookie or any raw API key in evidence.
- A visible sidebar alone does not prove the expected identity. Confirm the session response.
