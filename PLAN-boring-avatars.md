# Plan: Boring Avatars for team and user avatars

## Context

We need evaluate and plan integration of [Boring Avatars](https://boringavatars.com/) in `crates/admin-ui/web` for deterministic team and user avatars.

Findings:
- The admin UI currently has a Radix-based local avatar primitive in `crates/admin-ui/web/src/components/ui/avatar.tsx`.
- The sidebar currently renders the signed-in user as initials via `AvatarFallback` in `crates/admin-ui/web/src/components/app-sidebar.tsx`.
- User/team management pages currently render text-only identity rows/cards in `src/routes/identity/users.tsx` and `src/routes/identity/teams.tsx`.
- Boring Avatars package/API:
  - npm package: `boring-avatars@2.0.4`, MIT, ESM, ~28 KB unpacked, peer deps `react >=18` and `react-dom >=18` (compatible with current React 19).
  - Default export is a React SVG component with props `name`, `variant`, `size`, `colors`, `square`, `title`, plus SVG props.
  - Default/no explicit `variant` uses `marble`; `beam` is supported for users.
  - README notes the old hosted API service was paused; use the local React package, not the remote avatar service.
- Requested style direction:
  - Teams: default/no variant, e.g. `<Avatar name="Maria Mitchell" />`.
  - Users: `beam`, e.g. `<Avatar name="Alice Paul" variant="beam" />`.
  - Palette should incorporate logo-complementary blue, purple, and green.
- Logo sampling from `docs/public/images/oceans-logo-rounded-square.png` shows dominant light aqua plus deep ocean blue/teal. Existing theme vars also use teal/green OKLCH chart/primary colors.

## Approach

Add Boring Avatars as a local dependency and create a project-specific wrapper around its React SVG component rather than replacing the existing Radix avatar primitive globally. Use it anywhere we need generated avatars, while preserving current `Avatar`, `AvatarFallback`, `AvatarGroup`, and size styling APIs for uploaded/fallback avatars.

Recommended wrapper shape:
- `GeneratedAvatar` (or similarly named) accepts `kind: 'team' | 'user'`, `name`, optional `size`, `className`, and SVG props.
- It uses a shared `OCEANS_AVATAR_COLORS` palette with logo-derived blue/teal plus complementary purple/green, e.g. `['#B0E0E0', '#106090', '#2080B0', '#7C3AED', '#22C55E']` (final values can be tuned visually).
- It passes no `variant` for `kind="team"` and `variant="beam"` for `kind="user"`.
- It derives an accessible label/title from the entity kind/name, and keeps the SVG decorative only where adjacent visible text already labels the entity.
- Keep generated avatars rounded by default; use `square` only if a specific surface intentionally needs rounded-square styling.

## Files to modify

- `crates/admin-ui/web/package.json`
- `crates/admin-ui/web/bun.lock`
- New file such as `crates/admin-ui/web/src/components/ui/generated-avatar.tsx`
- `crates/admin-ui/web/src/components/app-sidebar.tsx`
- `crates/admin-ui/web/src/routes/identity/users.tsx`
- `crates/admin-ui/web/src/routes/identity/teams.tsx`
- Tests under `crates/admin-ui/web/src/test/routes/users-route.test.tsx`, `crates/admin-ui/web/src/test/routes/teams-route.test.tsx`, and optionally a new component test for the wrapper

## Reuse

- Reuse existing Radix avatar sizing/classes from `crates/admin-ui/web/src/components/ui/avatar.tsx`.
- Reuse current sidebar user name/email source from `crates/admin-ui/web/src/components/app-sidebar.tsx`.
- Reuse existing `user.name`, `user.email`, `team.name`, and `team.key` route data in `users.tsx` and `teams.tsx`; no API changes are needed.
- Reuse current table/card layout patterns and place avatars beside existing visible names instead of changing information architecture.

## Steps

- [ ] Add `boring-avatars` to `crates/admin-ui/web/package.json` and update `bun.lock`.
- [ ] Create `src/components/ui/generated-avatar.tsx` that imports Boring Avatars under an unambiguous alias (to avoid colliding with the existing Radix `Avatar`).
- [ ] Define and export/reuse one shared Oceans palette in the wrapper; include logo-derived aqua/blue plus complementary purple and green.
- [ ] Implement `kind="team"` with no explicit `variant`; implement `kind="user"` with `variant="beam"`.
- [ ] Replace the sidebar initials fallback with a generated user avatar using `session.user.name` (or `session.user.email` fallback if ever needed).
- [ ] Add user avatars next to names in mobile cards and desktop table rows in `identity/users.tsx`.
- [ ] Add team avatars next to team names in mobile cards and desktop table rows in `identity/teams.tsx`.
- [ ] Add user avatars in team member roster and user multiselect rows where space allows; keep badges text-only unless the UI becomes noisy.
- [ ] Add/adjust tests to assert avatars render with accessible labels or stable test IDs, without brittle SVG internals.

## Verification

- [ ] Run `bun run lint` in `crates/admin-ui/web`.
- [ ] Run `bun run test` in `crates/admin-ui/web`.
- [ ] Manually verify users and teams render deterministic avatars, use correct variants, and remain readable in light/dark themes.
