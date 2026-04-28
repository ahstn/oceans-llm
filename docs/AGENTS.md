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
- Put rough notes, interviews, and research under `docs/internal/`; they are not public contract pages.
- State validation commands before handoff, usually `mise run docs-check` or `mise run docs-verify`.
