# Password sign-in

Password sign-in lets a local user enter Oceans credentials, return to the requested protected page, confirm the authenticated identity, and end the session.

## Sub-features

- `auth-redirect` sends an unauthenticated protected route to sign-in with a redirect target.
- `auth-password` signs in with email and password.
- `auth-password-backoff` rejects excess failed sign-ins with `429` and permits sign-in after the backoff.
- `auth-admin-body-bounds` rejects oversized admin UI bodies with `413` and incomplete bodies with `408`.
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
- **Bare entry.** In a fresh context, open `/`, then `/admin`. Each redirects to `/admin/` and on to `/admin/login?redirect=%2F`, where the `Sign in` heading is visible instead of the Vite base-URL notice.
- **Credentials.** Fill the `Email` field with `admin@local`, fill the exact `Password` field with `admin`, and choose the `Sign in` button.
- **Redirect result.** The browser returns to `/admin/models?page=1&page_size=30`. The `Oceans Gateway` sidebar identity and `Models` heading are visible.
- **Default page.** In a fresh context, sign in from `/admin/login` with no `redirect`. The browser lands on `/admin/profile` and the `Welcome back, <first name>` heading appears. Report `auth-no-access` as skipped; it needs `permissions.users.pages: []`.
- **Session confirmation.** Fetch `/api/v1/auth/session` from the browser context. The response reports `admin@local`, `platform_admin`, and `must_change_password: false`.
- **Sign out.** Open the button whose accessible name contains `admin@local`, then choose the `Sign out` menu item. The browser returns to `/admin/login`, and `/api/v1/auth/session` returns `data: null`.
- **Failure path (optional).** A wrong password shows an error toast and stays on `/admin/login`.
- **Proof.** Capture sign-in before submission, the requested page after submission, and the sign-out page. Record the session response without its cookie.

Bounded security checks, after the normal sign-in proof:

- **Isolation.** Use only this run's loopback gateway, with no other callers or previous failed sign-ins. Do not run these checks against shared or production accounts. If the initial request is already throttled, stop and report the precondition rather than retrying. Keep requests sequential; do not saturate the Argon2 pool or try to fill the histories.
- **Failed sign-ins and backoff.** In a fresh signed-out context, send exactly 5 requests to `POST /api/v1/auth/login/password` with JSON fields `email: "admin@local"` and an intentionally wrong synthetic `password`. Each returns `401`, and `/api/v1/auth/session` still reports `data: null`. Immediately submit the correct credentials once; expect `429` and no session. Wait at least 1 second from the fifth failure, then submit the correct credentials once; expect `200` and the `admin@local` session. Sign out. Record only status codes, elapsed time and the sanitised session identity, never passwords or cookies. Stop on an unexpected result; do not add retries.
- **Body limit.** On the default local stack with Skills disabled or its default archive limit, the `/admin/*` body cap is 10,551,296 bytes (10 MiB + 64 KiB). Using a loopback HTTP client, send one `POST /admin/login` with `Content-Type: text/plain` and `Content-Length: 10551297`, without sending the body. Expect `413` before a body is needed. Then send one chunked `POST /admin/login` with `Content-Type: application/json`, no `Content-Length`, and at most 10,551,297 synthetic bytes; stop sending as soon as `413` arrives. Neither request should reach the UI handler. Keep only status codes and byte counts as evidence, and close both connections.
- **Body deadline.** Send one loopback `POST /admin/login` declaring a 1-byte body, but leave it incomplete. Expect `408` after `ADMIN_UI_REQUEST_TIMEOUT_MS` (10 seconds by default). Set the client's deadline to that value plus 2 seconds and close the connection even on failure. Do not open concurrent slow requests.
- **Deterministic coverage.** Use the offline regression tests, not live traffic, to prove concurrent account/source reservations, cancellation accounting, pinned bounded histories, the 20-failure source threshold, and the shared 4-job Argon2 pool with its 5-second slot wait. A sequential browser proof does not establish those invariants.
- **Cleanup.** Sign out any session created by these checks, close their browser context and HTTP clients, remove any temporary synthetic payload, then run `control-oceans-admin cleanup` even after a failed check. Keep the sanitised proof artifacts; do not delete or edit `gateway.db` to clear throttling.

## Gotchas

- The login form defaults to `admin@local` and `admin`, but a driver must still fill both fields so the action is explicit.
- Bootstrap only seeds `admin@local` when no platform admin exists, so a reused `gateway.db` keeps any changed password or rotation flag. `mise run dev-stack` refreshes demo data but does not delete `gateway.db`.
- A signed-in visit to `/admin/login` redirects to the default page (`/admin/profile` for `admin@local`). Start each sign-in proof from a fresh browser context.
- The admin UI proxy must not follow upstream redirects. If a protected route answers `200` with the sign-in page instead of `307`, the proxy is serving the redirect target at the original URL, and React logs `Hydration failed because the server rendered HTML didn't match the client`.
- The gateway redirects `/` and the bare `/admin` to `/admin/`, because the Vite dev server answers `/admin` with a base-URL notice (404). Signed out, `/admin/` redirects to `/admin/login?redirect=%2F`, so prove the default page through sign-in instead of the index route.
- OIDC/OAuth buttons appear under `or continue with` only when providers are configured, and `?sso_error=` shows the alert `SSO sign in failed`. Neither is part of password sign-in.
- `/admin/no-access` is allowed only when a session has zero pages. Page lists merge across groups, so every seeded user is redirected away from it. `/admin/account-ready` and voluntary password change belong to [account onboarding](./account-onboarding.md).
- Do not record the session cookie or any raw API key in evidence.
- A visible sidebar alone does not prove the expected identity. Confirm the session response.
- Password failure histories are process-local: 5 failures per account, 20 per source, exponential backoff from 1 to 60 seconds, and reset after 15 idle minutes. Successful sign-in clears account failures, not source history. Closing a browser or cancelling an admitted request does not clear its accounting.
- The `/admin/*` body cap applies to every content type, not just multipart uploads. It is separate from the JSON limit on `/api/v1/auth/login/password`; send body-bound checks through `/admin/login`.
