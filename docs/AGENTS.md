Rather than operators, refer to people as distinct audience classes:

- `admins`: humans using the control plane, including platform admins and team admins.
- `users`: managed identities or end users whose access, budgets, and logs are governed by the gateway.
- `maintainers`: contributors changing repo code, migrations, releases, or docs.
- `callers` or `clients`: software sending data-plane requests through API keys.

For docs changes:

- Update the canonical owning page instead of copying policy across several pages.
- Capture behavior that spans files, workflows, or runtime phases; do not restate code that is obvious from one source file.
- Link to ADRs, GitHub issues, PRs, and source files when they explain why the behavior exists.
- Keep `docs/adr/` as historical decision records. Prefer appending a short supersession note over rewriting old decision context.
- Preserve the split between the primary user-facing docs surface and the `Contributing & Internal` surface.
- Keep the primary surface focused on admins, users, callers, and clients operating Oceans LLM.
- Put maintainer-only workflows, release process, contract generation, schema/data-model notes, migrations, E2E harnesses, implementation plans, interviews, rough notes, and research in the internal/contributor surface.
- Keep `docs/internal/` for rough notes and research that should not publish as contract pages. Use a published internal/contributor docs path only when maintainers need the page in the VitePress site.
- Do not link user-facing navigation to maintainer-only pages unless the user workflow genuinely depends on that context. Internal/contributor pages may link back to user-facing canonical pages.
- When adding or moving pages, update VitePress nav/sidebar ownership and `See also` links together so labels match destination titles.
- State validation commands before handoff: from the repo root use `mise run //docs:build`; from `docs/`, use `mise run build`.
