# Documentation Source Notes

This folder holds the docs site published at [oceans-llm.com](https://oceans-llm.com/), and the maintainer documents that stay in the repo without being published.

- Site guide (layout, sidebar, checks, deploys):
  - [AGENT.md](AGENT.md)
- Writing guide:
  - [AGENTS.md](AGENTS.md)
- Site config:
  - [astro.config.ts](astro.config.ts)
- Site pages:
  - [src/content/docs/](src/content/docs)
- Canonical provider compatibility reference:
  - [Provider API compatibility](https://oceans-llm.com/reference/provider-api-compatibility/)
- Migration authoring checklist:
  - [contributing/reference/migration-authoring.md](contributing/reference/migration-authoring.md)
- ADRs:
  - [adr/](adr)
- Internal notes:
  - [internal/](internal)
- Development workflows:
  - [contributing/development/](contributing/development)

What lives where:

- `src/content/docs/` holds the published pages. `src/landing/` holds the landing page at `/`.
- `public/` is copied to the site as is: images, the shadcn theme registry in `r/`, and `_redirects` for old URLs.
- `contributing/` holds maintainer and contributor workflows. It isn't published.
- `adr/` explains why decisions were made.
- `internal/`, `plans/`, `interviews/` and `design-proposals/` hold research, plans and rough notes that should not be treated as live contract.

When adding a new canonical page:

- create it in the right folder under `src/content/docs/`
- add its sidebar icon to `src/lib/sidebar-icons.ts`, and a new top-level folder to `astro.config.ts`
- add a `public/_redirects` rule when a page moves
- run `mise run //docs:check` before handing docs work off
- deploy with `mise run //docs:deploy` (see [AGENT.md](AGENT.md))
