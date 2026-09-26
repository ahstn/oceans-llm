# Account onboarding

Account onboarding lets a platform administrator create a password user and share a one-time invite URL. The invitee sets a password on a public page, sees `Account ready`, and then signs in. A signed-in password user can also rotate their own password from the sidebar identity menu.

## Sub-features

- `onboard-create-user` creates a password user from `Add user`, then shows `Password invite ready` with a `Generated URL`.
- `onboard-invite-accept` sets a password at `/admin/invite/<token>` and moves the invitation to `consumed`.
- `onboard-invite-invalid` shows `Invitation <state>` when the token is invalid, consumed, revoked, or expired.
- `onboard-account-ready` confirms onboarding and links `Open control plane` to `/admin`.
- `onboard-change-password` lets a signed-in password user rotate their password at `/admin/change-password`.

## How to get to it (user POV)

- Choose `Users` → `Add user`. For an invited or disabled user, use `Manage` → `Auth & onboarding` → `Reset onboarding`.
- Open the shared `/admin/invite/<token>` URL while signed out. On success it lands at `/admin/account-ready?mode=password&email=<email>`.
- Open the sidebar identity menu and choose `Change password`.

## Driving it with control-oceans-admin

Preconditions:

- `control-oceans-admin doctor` passes, and you are signed in as `admin@local`.
- Use the email `onboard-<run-id>@verify.local`. Generate a password of at least 8 characters that includes the run ID, and keep it in memory only.

- **Create.** Choose `Users` → `Add user`. Fill `Name` and `Email`, keep `Auth method` as Password and `Team` as `No team`, then choose `Create user`. The toast reads `Password invite created`, and the `generated-url` field holds `/admin/invite/<token>`.
- **Second view.** The new user's row shows Status `invited`, and `/api/v1/admin/identity/users` agrees.
- **Invite state.** In a new browser context, fetch `/api/v1/auth/invitations/<token>`. The state is `valid`.
- **Accept.** In the same context, open the URL. The heading reads `Finish your account setup` and `Invitation is valid` is shown. Fill `Password` and `Confirm password`, then choose `Set password`. The toast reads `Password set`.
- **Ready.** The URL is `/admin/account-ready?mode=password&email=...` and the heading reads `Account ready`. Choose `Open control plane`; the sign-in page appears.
- **API parity.** Fetch the invitation again. Its state is `consumed`, and reopening the URL shows `Invitation consumed`. The user's Status reads `active`.
- **Sign in.** Sign in as the new user. The browser lands on `/admin/observability/usage-costs`.
- **Invalid token.** Open `/admin/invite/not-a-token`. The page shows `Invitation invalid` and no `Set password` button.
- **Change password.** In a new browser context, sign in as `ben@platform.local` with `localdemo123`. Choose `Change password`, fill `Current password`, `New password`, and `Confirm new password`, then choose `Update password`. The page reloads to the session's default page (`/admin/observability/usage-costs` for Ben), and the next sign-in with the new password succeeds.
- **Cleanup.** Change Ben's password back to `localdemo123`. As admin, open the created user's `Manage` → `Auth & onboarding` and choose `Deactivate`. The toast reads `User deactivated` and Status reads `disabled`.
- **Proof.** Capture the invite result with the URL masked, the accept form, `Account ready`, the `consumed` state, and the deactivated row. Redact tokens and passwords.

## Gotchas

- Never rotate the `admin@local` password. Bootstrap does not reset it on a reused `gateway.db`.
- The invite token is a credential. Redact the `Generated URL` from screenshots and logs.
- `Set password` does not sign the user in. There is no user delete, so `Deactivate` is the cleanup.
- `Change password` always shows `Rotation required` and prefills `Current password` with `admin`. Clear the field before filling it.
- `mode=oidc` and `mode=oauth` need SSO providers, and `gateway.yaml` does not configure any.
- `Update password` fires `Password updated` and then does a full-page navigation, so the toast is rarely visible. Assert the navigation away from `/admin/change-password` and a sign-in with the new password instead.
- If a change-password run fails midway, restore Ben with `POST /api/v1/auth/password/change` before the next proof; other recipes sign in with `localdemo123`.
- Forced rotation (`require_password_change: true`) is covered in password-sign-in.
