# Identity

Identity lets a platform administrator review and manage users, teams, and service accounts from the `Identity` sidebar group. Other signed-in users get read-only directories of the same records. Service accounts are always read-only and link to their team and API key.

## Sub-features

- `identity-users-list` lists users with `Global role`, `Team`, and `Status`, and opens the `Manage user` sections.
- `identity-user-manage` edits role, team, tags, and lifecycle (`Deactivate`, `Reactivate`, `Reset onboarding`) for non-bootstrap users.
- `identity-teams-list` lists teams with admins, member counts, and an expandable member roster.
- `identity-team-membership` adds teamless users, removes members, and transfers members between teams.
- `identity-service-accounts` lists service accounts with links to their team and API key.
- `identity-read-only` shows directory views with `Only administrators can make changes.` to users who are not platform admins.

## How to get to it (user POV)

- Under `Identity`, choose `Teams`, `Users`, or `Service Accounts`.
- Open `/admin/identity/users?user_id=<id>&user_section=overview|configuration|auth|provider-configuration|usage` directly.
- From Teams, choose an admin link `Open <name>`. From Service Accounts, choose `Open <team> in Teams` or `Open API key <name>`.

## Driving it with control-oceans-admin

Preconditions:

- `control-oceans-admin doctor` passes, and you are signed in as `admin@local`.
- Seeded demo users use the password `localdemo123`. `ben@platform.local` is a member of Platform.

- **Users list.** Choose `Users`. The `Users` heading and the `User list` card show the `Name`, `Email`, `Global role`, `Team`, `Status`, and `Actions` columns.
- **API parity (users).** Fetch `/api/v1/admin/identity/users` and read `data.users`. Every returned email appears once in the list with the same status and team.
- **Manage user.** In the `ben@platform.local` row, choose `Manage`. The URL gains `user_id` and `user_section=overview`, and `Profile` shows `Team` as Platform. Close the sheet.
- **Teams list.** Choose `Teams`. The `Platform`, `Research`, `Applied AI`, and `Operations` rows each show a `Show N members` toggle, and N matches `member_count` from `/api/v1/admin/identity/teams` (`data.teams`).
- **Transfer.** Expand Platform. In Ben's roster row, choose `Transfer`. Set `Destination team` to Research and `Destination role` to Member, then choose `Transfer member`. The toast reads `Member transferred`.
- **Second view.** Reload Users. Ben's `Team` reads Research, and the API agrees.
- **Cleanup.** Expand Research, choose `Transfer` for Ben, and move him back to Platform as a Member. Confirm Team reads Platform in Users and in the API.
- **Service accounts.** Choose `Service Accounts`. `Local CI Runner` shows team Platform and `Open API key Local CI Runner Key`. `Local Eval Worker` shows `No credential attached`. Compare both with `/api/v1/admin/identity/service-accounts`.
- **Read-only role.** In a new browser context, sign in as `ben@platform.local`. `Users` shows `Only administrators can make changes.` and has no `Add user` button. Compare it with `/api/v1/identity/directory/users`.
- **Proof.** Capture both list views, the transfer dialog, the moved and restored team, and the read-only view.

## Gotchas

- `admin@local` is the bootstrap admin, so it cannot be edited, deactivated, or reset. Never change its password.
- You cannot deactivate yourself. There is no user delete and no team delete, so do not create teams during verification.
- Owner memberships have `Transfer` and `Remove` disabled. The seed has no owners.
- `Add members` lists only teamless users. `Reset onboarding` is enabled only for invited or disabled users.
- Tables and mobile cards both render `Manage`, `Edit team`, and `Add members`, so scope each click to the visible row.
- Team admins such as `cara@research.local` also get read-only views, because identity mutations call `require_platform_admin`.
- The expanded roster is one table cell with a flat list of members, not one row per member. Scope `Transfer` and `Remove` through the member's email text, for example its nearest ancestor that contains a `Transfer` button.
- The page heading is `Service accounts`, but the sidebar label is `Service Accounts`.
