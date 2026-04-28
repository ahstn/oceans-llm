# ADR: Cloudflare Pages Hosting for VitePress Docs

- Date: 2026-04-27
- Status: Accepted

## Context

The repository already contains a VitePress documentation site under `docs/`. The docs package builds static assets with `vitepress build .` into `docs/.vitepress/dist`, and the site does not require server-side code.

The project also owns the `oceans-llm.com` domain. The docs site needs a low-maintenance public hosting path that works locally, can be driven from CI later, and does not add application runtime infrastructure.

## Decision

Host the docs site as a Cloudflare Pages project named `oceans-llm-docs`, built from `docs` and deployed from the static output directory `.vitepress/dist`.

Use `docs.oceans-llm.com` as the production custom domain. This keeps `oceans-llm.com` available for the product apex domain while making the documentation address explicit and stable.

Use Wrangler direct upload for the repo-managed task surface:

- `mise run cf-pages-create` creates the Pages project.
- `mise run cf-pages-deploy` builds and uploads `.vitepress/dist`.
- `mise run cf-pages-domain-add` associates `docs.oceans-llm.com` with the Pages project.
- `mise run cf-register-ci` stores the Cloudflare credentials and Pages parameters in GitHub Actions.

## Why

Cloudflare Pages is the most direct fit for a static VitePress site. It provides global static hosting, preview deployments, rollbacks, and custom domains without introducing a Worker, container, bucket, or origin server.

Direct upload via Wrangler keeps deployment compatible with existing `mise` tasks and future CI. It avoids coupling the Cloudflare project to one Git integration path while still using Cloudflare's documented Pages deployment flow for prebuilt static assets.

## Trade-offs

Direct Upload projects cannot later be converted to Cloudflare Pages Git integration. If maintainers decide that Cloudflare-managed Git builds are preferable, they should create a replacement Pages project with Git integration.

The `docs.oceans-llm.com` custom domain must be associated with the Pages project before DNS alone will work. For a Cloudflare-managed `oceans-llm.com` zone, Cloudflare can create the required CNAME during custom domain setup.

## Follow-ups

- Add a GitHub Actions workflow that runs `mise run docs-check`, `mise run docs-build`, then deploys `.vitepress/dist` with Wrangler.
- Decide whether preview deployments should run for pull requests, pushes to non-main branches, or both.
