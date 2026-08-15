# Documentation Source Notes

This file stays in the repo as a maintainer note for the VitePress source tree.

- Public entry point:
  - [Documentation Home](index.md)
- Site config:
  - [.vitepress/config.mts](.vitepress/config.mts)
- Canonical provider compatibility reference:
  - [reference/provider-api-compatibility.md](reference/provider-api-compatibility.md)
- Migration authoring checklist:
  - [contributing/reference/migration-authoring.md](contributing/reference/migration-authoring.md)
- ADRs:
  - [adr/](adr)
- Internal notes:
  - [internal/](internal)
- Development workflows:
  - [development/](development)

What lives where:

- `index.md` and the section folders drive the published docs site.
- `contributing/` holds published maintainer and contributor workflows.
- `adr/` explains why decisions were made.
- `internal/` holds research and rough notes that should not be treated as live contract.

When adding a new canonical page:

- place it in the right section folder
- add it to the matching VitePress sidebar group
- link it from `index.md` if it changes the audience map
- run `mise run //docs:build` before handing docs work off
- keep `adr/` and `internal/` out of the public nav
