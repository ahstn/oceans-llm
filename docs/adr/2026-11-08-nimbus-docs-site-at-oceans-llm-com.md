# ADR Nimbus docs site at oceans-llm.com

- Date: 2026-11-08
- Status: Accepted
- Supersedes: the VitePress and `docs.oceans-llm.com` parts of [Cloudflare Pages Hosting for VitePress Docs](2026-04-27-cloudflare-pages-docs-hosting.md)

## Context

The VitePress site in `docs/` had been rebuilt as a Nimbus (Astro) site in `docs-nimbus/`, with a landing page at `/`, a new sidebar and new page paths. Two docs folders could not both stay: links, CI and agent instructions had to name one.

The old site was already served at the apex `oceans-llm.com` by the Cloudflare Pages project `oceans-llm-docs`. `docs.oceans-llm.com`, the domain the earlier ADR chose, has no DNS record. The Pages project uses direct upload, with no Git integration.

`docs/` also held documents that the new site doesn't publish: ADRs, contributor workflows, plans, interviews, internal notes and design proposals. The old site published the contributor pages under `/contributing/`.

## Decision

- The Nimbus site replaces VitePress in `docs/`. There is one docs folder.
- The site's canonical URL is `https://oceans-llm.com`, served by the same Pages project, `oceans-llm-docs`.
- Deploys stay manual with `mise run //docs:deploy`, which uploads `dist/` with `wrangler pages deploy`. CI builds and checks the site but doesn't deploy it.
- Maintainer documents keep their paths in `docs/` and aren't published. Contributor pages are read on GitHub.
- Every old URL keeps working. `public/_redirects` sends each old page, in both its `/page` and `/page.html` forms, to its new page, and the old `/contributing/` pages to GitHub. `public/images/` and `public/r/` keep their paths, because the README, the Helm chart icon and shadcn theme installs use them.

## Implementation

- [`docs/wrangler.jsonc`](../wrangler.jsonc) is a Pages config: the project name and `pages_build_output_dir`. [`docs/mise.toml`](../mise.toml) holds the `install`, `dev`, `build`, `check`, `preview` and `deploy` tasks. `deploy` reads `CLOUDFLARE_ACCOUNT_ID` and `CLOUDFLARE_API_TOKEN` from the gitignored `docs/mise.local.toml`, and takes `CLOUDFLARE_PAGES_BRANCH` for a preview deploy.
- [`.github/workflows/docs-ci.yml`](../../.github/workflows/docs-ci.yml) runs `mise run //docs:check`: the build, `astro check` and `nimbus-docs check`.
- Astro builds only `src/` and copies `public/`, so `adr/`, `contributing/`, `internal/`, `plans/`, `interviews/` and `design-proposals/` stay out of the site without config.
- Links in the repo to old pages now use `https://oceans-llm.com/<path>/` URLs. Site pages link to maintainer documents with GitHub URLs.
- `scripts/build-shadcn-theme.mjs` checks the Nimbus `[data-mode="dark"]` tokens in `docs/src/styles/globals.css` against the admin UI tokens, in place of the VitePress theme file.

## Why

- **Apex domain:** search results, the admin UI's harness link and the Helm chart icon already point at `oceans-llm.com`. Moving to `docs.oceans-llm.com` would need new DNS and break those links for no gain.
- **Same Pages project:** it keeps the domain, the deploy history and rollbacks. Nimbus's static build writes `_headers`, and Pages reads `_headers` and `_redirects` from the output folder.
- **Manual deploys:** the site changes in batches with the docs, and a manual deploy keeps Cloudflare credentials out of GitHub.
- **Unpublished maintainer docs:** the site is for admins, users and callers. Contributor pages render well on GitHub and move with the code.

## Trade-offs

- A merged docs change isn't live until someone runs `mise run //docs:deploy`.
- Contributor pages lose site search and the site's sidebar.
- `_redirects` must gain a rule whenever a page moves.

## Follow-ups

- Revisit a CI deploy on pushes to `main` if manual deploys lag behind merged docs changes.
