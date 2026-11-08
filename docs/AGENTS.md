This folder is the Nimbus (Astro) docs site published at https://oceans-llm.com. [AGENT.md](./AGENT.md) covers how the site works: file layout, components, the sidebar, checks and deploys. This file covers what to write and for whom.

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
- The published site, `src/content/docs/`, is for admins, users, callers, and clients operating Oceans LLM.
- Maintainer-only workflows, release process, contract generation, schema/data-model notes, migrations, E2E harnesses, implementation plans, interviews, rough notes, and research go in `contributing/`, `internal/`, `plans/`, or `interviews/`. These folders stay in the repo and aren't published. Keep them outside `src/` and `public/`.
- Keep `docs/internal/` for rough notes and research that should not be treated as live contract.
- Do not link site pages to maintainer-only documents unless the user workflow genuinely depends on that context. When you do, use a GitHub URL (`https://github.com/ahstn/oceans-llm/blob/main/docs/...`). Maintainer documents may link back to site pages with `https://oceans-llm.com/<path>/` URLs.
- When adding or moving pages, update these together: the sidebar (top-level groups in `astro.config.ts`, `sidebar.*` frontmatter), the page icon in `src/lib/sidebar-icons.ts`, a `public/_redirects` rule for the old URL, and `See also` links, so labels match destination titles.
- State validation commands before handoff: from the repo root use `mise run //docs:check`, which builds the site and runs `astro check` and `nimbus-docs check`; from `docs/`, use `mise run check`.
