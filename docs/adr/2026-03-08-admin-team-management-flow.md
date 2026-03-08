# ADR: Admin Team Management Flow and Deferred Team Admin Assignment

- Date: 2026-03-08
- Status: Accepted

## Context

The admin UI already had an identity foundation for users, invitations, team membership storage, and one-team-per-user enforcement, but the `Teams` page was still mock data. Admins could not:

- create teams from the UI,
- assign team admins during creation,
- come back later and assign admins once users existed,
- add existing users to a team,
- invite a new user directly into a team.

This gap mattered because the underlying data model and adjacent product requirements already assume teams are first-class ownership boundaries for API keys, budgets, and future spending controls. The supporting docs also make the relationship explicit:

- teams are durable containers for members and team-owned resources,
- users may exist without a team,
- many teams will be created before their eventual users exist,
- one user should belong to at most one team in this slice.

We needed a team-management slice that:

- keeps Rust as the source of truth for team and membership state,
- fits the same same-origin admin architecture used by the users flow,
- preserves the existing one-team-per-user rule,
- allows empty teams at creation time,
- allows deferred assignment of team admins,
- reuses the user onboarding flow instead of inventing a second invite system.

## Decision

### 1. Build team management as a full vertical slice through store, gateway, and admin UI

We implemented team creation and membership management across:

- `crates/gateway-store`
- `crates/gateway`
- `crates/admin-ui/web`

Why:
- the database already held the source-of-truth identity schema,
- the gateway already owned admin identity APIs for users,
- the admin UI should continue consuming explicit gateway endpoints rather than direct database access.

### 2. Treat team creation and team membership changes as admin-only gateway operations

We added admin endpoints for:

- listing teams plus assignable users,
- creating a team,
- updating a team name and admin set,
- adding existing users as team members.

Why:
- all team membership changes affect authorization and ownership boundaries,
- admin authorization already exists at the gateway layer,
- keeping these operations in one backend boundary avoids Bun-side data drift or duplicated validation logic.

### 3. Keep `team_key` server-generated and non-editable in v1

New teams receive a generated `team_key` derived from the requested name, with collision handling in the gateway. The UI does not expose `team_key` editing.

Why:
- team keys are stable system identifiers, not presentation fields,
- server-side generation avoids inconsistent slug behavior between clients,
- non-editable keys reduce churn for future team-owned resources that may reference them.

### 4. Allow teams to be created with zero admins

The create-team flow accepts an empty admin list.

Why:
- team creation often precedes user creation,
- blocking team creation on existing users would fight the documented relationship model,
- deferred admin assignment is operationally simpler than forcing placeholder users.

### 5. Model team admin editing as full-set synchronization of the `admin` role only

The edit-team dialog manages only the `admin` subset for the target team:

- selected users are promoted or added as `admin`,
- deselected current admins are demoted to `member`,
- users are not removed from the team through this flow,
- the `owner` role remains hidden in this slice.

Why:
- this keeps the UI focused on the concrete admin-management need,
- it avoids introducing partial member-removal semantics in the same slice,
- `owner` is a stronger role that should remain out of the casual admin UI until its lifecycle is clearer.

### 6. Preserve the one-team-per-user rule and reject cross-team reassignment in v1

When adding existing users to a team:

- teamless users can be added,
- users already on the same team are treated as no-ops,
- users belonging to another team are rejected and surfaced as unavailable in the UI.

Why:
- the schema already enforces one team membership per user,
- silent reassignment would be risky for budgets, API keys, and future team-owned resources,
- explicit rejection is safer until a dedicated transfer flow exists.

### 7. Reuse the existing user onboarding flow for new team members

The team page does not create a separate invitation system. Instead, the `Add members` dialog reuses `POST /api/v1/admin/identity/users` with:

- `team_id` set to the current team,
- `team_role` fixed to `member`,
- `global_role` defaulted to `user`.

Why:
- it keeps password-invite and OIDC onboarding behavior consistent across the product,
- it avoids duplicating invitation logic and token handling,
- it ensures newly invited users appear immediately with their intended team assignment.

### 8. Use a local shadcn-style multi-select composition instead of adding a second design system path

The admin picker and existing-user picker are composed from local primitives using:

- `Popover`
- `Command`
- `Badge`

Why:
- the repo already uses local source-controlled shadcn-style components,
- we only needed a small set of additional primitives,
- this kept the UI accessible and consistent without introducing a separate external form library.

## Consequences

Positive:
- the `Teams` page now reflects real backend state instead of mock data,
- admins can create teams before users exist,
- team admins can be assigned later without manual SQL or backend-only steps,
- existing users and newly invited users can be attached to a team from one place,
- cross-team conflicts fail safely instead of silently moving users.

Tradeoffs:
- team transfer and member removal remain out of scope,
- `member_count` is derived from team memberships and is intentionally simpler than a richer breakdown by role,
- `owner` remains a backend concept that is not yet manageable from the UI,
- direct local dev against `:3001` still depends on restarting the gateway process when new routes are added.

## Follow-up Work

- Add explicit team member removal and team-to-team transfer flows.
- Decide when and how `owner` should be surfaced in admin tooling.
- Connect team pages to team-owned budgets, API keys, and model access controls as those slices become editable.
- Add richer gateway integration coverage for the new team endpoints.

## Attribution

This ADR was prepared through collaborative human + AI implementation/design work.
