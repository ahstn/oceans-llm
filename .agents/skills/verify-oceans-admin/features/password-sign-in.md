# Password sign-in

Password sign-in lets a local user enter Oceans credentials, return to the requested protected page, confirm the authenticated identity, and end the session.

## Sub-features

- `auth-redirect` sends an unauthenticated protected route to sign-in with a redirect target.
- `auth-password` signs in with email and password.
- `auth-session` shows the signed-in identity in the application sidebar.
- `auth-sign-out` clears the session and returns to sign-in.

## How to get to it (user POV)

- Open `/admin` or any protected `/admin/*` route while signed out.
- Open `/admin/login` directly.
- Open the identity menu in the sidebar and choose `Sign out` to end a session.

## Driving it with control-oceans-admin

Preconditions:

- `control-oceans-admin doctor` passes.
- The demo seed has the platform admin `admin@local` with password `admin`.
- Use a new browser context so an old session cookie cannot bypass sign-in.

- **Protected entry.** Open `/admin/models`. The URL becomes `/admin/login?redirect=%2Fmodels` and the heading `Sign in` is visible.
- **Credentials.** Fill the `Email` field with `admin@local`, fill the exact `Password` field with `admin`, and choose the `Sign in` button.
- **Redirect result.** The browser returns to `/admin/models`. The `Oceans Gateway` sidebar identity and `Models` heading are visible.
- **Session confirmation.** Fetch `/api/v1/auth/session` from the browser context. The response reports `admin@local`, `platform_admin`, and `must_change_password: false`.
- **Sign out.** Open the button whose accessible name contains `admin@local`, then choose the `Sign out` menu item. The browser returns to `/admin/login`, and `/api/v1/auth/session` no longer reports an authenticated user.
- **Proof.** Capture sign-in before submission, the requested page after submission, and the sign-out page. Record the session response without its cookie.

## Gotchas

- The login form defaults to `admin@local` and `admin`, but a driver must still fill both fields so the action is explicit.
- A changed local database can retain a different password. `mise run dev-stack` refreshes demo data but does not delete `gateway.db`.
- Do not record the session cookie or any raw API key in evidence.
- A visible sidebar alone does not prove the expected identity. Confirm the session response.
