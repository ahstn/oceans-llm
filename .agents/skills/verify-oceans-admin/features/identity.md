# Identity

Identity lets a platform administrator review and manage users, teams, and service accounts from the `Identity` sidebar group. Other signed-in users get read-only directories of the same records. Service accounts are always read-only and link to their team and API key.

## Sub-features

- `identity-users-list` lists users with `Global role`, `Team`, and `Status`, and opens the `Manage user` sections.
- `identity-user-manage` edits role, team, tags, and lifecycle (`Deactivate`, `Reactivate`, `Reset onboarding`) for non-bootstrap users.
- `identity-teams-list` lists teams with admins, member avatars, tags, and status, filters them by search, tag, missing admins, and status, and expands a row to its member table.
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
- **Teams list.** Choose `Teams`. The `Teams` table lists `Platform`, `Research`, `Applied AI`, and `Operations`, and `Showing N of N teams` matches the length of `data.teams` from `/api/v1/admin/identity/teams`. Choose `Expand Platform`: the `Members of Platform` table has one row per member, and its count after `Members ·` matches Platform's `member_count`.
- **Filters.** Open `Filter` and check `Without admins`: only teams with no admins stay (none in the seed, so the list shows `No teams match these filters.`), and an `Admins is none` chip appears. Choose `Clear`. If a seeded team has tags, choose one of its `Filter by <key>:<value>` chips and confirm only teams with that tag stay.
- **Transfer.** Expand Platform. In the `Members of Platform` table, open `Actions for Ben` and choose `Transfer`. Set `Destination team` to Research and `Destination role` to Member, then choose `Transfer member`. The toast reads `Member transferred`.
- **Second view.** Reload Users. Ben's `Team` reads Research, and the API agrees.
- **Cleanup.** Expand Research, open `Actions for Ben` and choose `Transfer`, and move him back to Platform as a Member. Confirm Team reads Platform in Users and in the API.
- **Service accounts.** Choose `Service Accounts`. `Local CI Runner` shows team Platform and `Open API key Local CI Runner Key`. `Local Eval Worker` shows `No credential attached`. Compare both with `/api/v1/admin/identity/service-accounts`.
- **Read-only role.** In a new browser context, sign in as `ben@platform.local`. `Users` shows `Only administrators can make changes.` and has no `Add user` button. Compare it with `/api/v1/identity/directory/users`.
- **Proof.** Capture both list views, the transfer dialog, the moved and restored team, and the read-only view.

## Gotchas

- `admin@local` is the bootstrap admin, so it cannot be edited, deactivated, or reset. Never change its password.
- You cannot deactivate yourself. There is no user delete and no team delete, so do not create teams during verification.
- Owner memberships have `Transfer` and `Remove` disabled in their `Actions for <name>` menu, which explains why. The seed has no owners.
- `Add members` lists only teamless users. `Reset onboarding` is enabled only for invited or disabled users.
- Users renders `Manage` in both its table and its mobile cards, so scope each click to the visible row. On Teams, `Edit team`, `Add members`, and `Copy team key` sit in the `Actions for <team>` menu in the pinned right column. The card and table share expanded state, so an expanded team renders its member table twice; scope to the visible `Teams` table.
- Team admins such as `cara@research.local` also get read-only views, because identity mutations call `require_platform_admin`.
- The expanded team shows a `Members of <team>` table with one row per member. `Transfer` and `Remove` are items in that member's `Actions for <name>` menu, not buttons on the row. Clicking anywhere on a team row toggles it, except its tag chips, admin link, and actions menu.
- The page heading is `Service accounts`, but the sidebar label is `Service Accounts`.
