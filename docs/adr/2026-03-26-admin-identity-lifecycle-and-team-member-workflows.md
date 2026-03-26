# ADR: Admin Identity Lifecycle and Team Member Workflows

- Date: 2026-03-26
- Status: Accepted

## Implemented By

- Canonical docs:
  - [../identity-and-access.md](../identity-and-access.md)
  - [../admin-control-plane.md](../admin-control-plane.md)
  - [../e2e-contract-tests.md](../e2e-contract-tests.md)

## Context

The initial identity foundation gave the gateway and admin UI the ability to create users, invite members, and manage teams. That was enough to bootstrap access, but not enough to safely operate the control plane over time.

Operators still needed explicit workflows for:

- editing user lifecycle fields without recreating accounts,
- deactivating and reactivating users,
- resetting onboarding for password and OIDC users,
- removing a member from a team,
- transferring a member to another team with an explicit destination role,
- preventing owner memberships from being casually removed or reassigned,
- protecting the last active platform admin,
- keeping bootstrap admin behavior out of normal user-management flows.

Those actions are security-sensitive because they affect access, onboarding, and future ownership boundaries. They also need to remain consistent across the gateway, the admin UI, and the store-backed transaction layer.

## Decision

### 1. Model user lifecycle as an explicit state machine

We treat user state as `invited`, `active`, or `disabled` and centralize transition rules in the backend lifecycle layer.

Why:

- lifecycle transitions are authorization-sensitive and should not be spread across ad hoc handlers,
- the same rules apply across password and OIDC onboarding,
- a typed lifecycle model makes the state transitions easier to test and reason about.

### 2. Allow auth-mode changes only while a user is still invited

The admin UI can switch onboarding mode only before the user has activated.

Why:

- switching auth mode after activation is easy to get wrong and can orphan credentials,
- the invited state is the only point where onboarding is still in progress,
- the reset-onboarding action provides an explicit recovery path for already-activated users.

### 3. Add explicit lifecycle endpoints instead of overloading user creation

We use dedicated actions for:

- update user metadata and membership fields,
- deactivate user,
- reactivate user,
- reset onboarding,
- remove team member,
- transfer team member.

Why:

- destructive actions deserve named endpoints and named UI actions,
- keeping the operations separate reduces accidental coupling,
- it makes the route invalidation model straightforward in the UI.

### 4. Block owner removal and transfer in this slice

Owner memberships remain visible but are not removable or transferable from the admin UI in this slice.

Why:

- owner is a stronger boundary than ordinary admin/member membership,
- the product does not yet have a broader ownership migration story,
- blocking the action now avoids inventing incomplete semantics for historical ownership and spend.

### 5. Protect the last active platform admin and bootstrap admin

The backend lifecycle layer refuses operations that would remove the last active platform admin, and bootstrap admin remains out of band from normal user management.

Why:

- the control plane must not be able to lock itself out,
- bootstrap admin is a setup concern, not a normal lifecycle target,
- the backend is the correct place to enforce these invariants.

### 6. Treat transfer as future-membership only

Transferring a member only changes future membership-derived access.

Why:

- request logs, spend, budgets, and API-key ownership are historical or independently owned data,
- implicit historical migration would be harder to explain and more error-prone,
- keeping transfer narrow preserves data integrity while still enabling day-to-day reorganization.

## Consequences

Positive:

- operators can now deactivate, reactivate, and re-onboard users without recreating them,
- team membership changes are explicit and auditable,
- the UI can surface real owner-blocking and auth-mode limits instead of relying on documentation alone,
- the gateway and store can enforce the same invariants consistently.

Tradeoffs:

- owner workflows remain deferred,
- some lifecycle actions are intentionally conservative and require explicit confirmation,
- transfer does not simplify any historical accounting or ownership data.

## Follow-Up Work

- Decide whether owner membership gets its own dedicated lifecycle once the broader ownership model is ready.
- Expand contract coverage around invalid transitions and last-admin protection.
- Consider whether the lifecycle state machine should be surfaced as a shared SDK type for other admin surfaces.
