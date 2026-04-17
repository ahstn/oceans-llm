# ADR: Cloudflare Pages Deployment For Public Docs

- Date: 2026-03-31
- Status: Accepted

## Current state

- [../reference/release-process.md](../reference/release-process.md)
- [../setup/deploy-and-operations.md](../setup/deploy-and-operations.md)

## Context

Implemented by:

- [../../.github/workflows/release.yml](../../.github/workflows/release.yml)
- [../../mise.toml](../../mise.toml)
- [../package.json](../package.json)
- [../.vitepress/config.mts](../.vitepress/config.mts)

The repository already ships a standalone VitePress docs source tree under `docs/`, but publishing it to a stable public domain was still a manual gap. The project needed a deployment path that:

- publishes the docs site at `https://oceans-llm.com`,
- keeps release-time docs publication aligned with the existing tag-driven release model,
- avoids adding another always-on application to operate,
- preserves the docs site as a static artifact instead of introducing SSR or custom backend code,
- leaves future maintainers with a small, explicit deployment surface.

We also needed to account for current platform reality:

- the docs site is already static VitePress output,
- the repo standardizes operational tooling through `mise`,
- Cloudflare Pages can host the built site directly,
- apex domain attachment and `www` redirect policy live in Cloudflare zone configuration, not purely in the repo.

## Decision

### 1. Publish the docs site as a static Cloudflare Pages project

The public docs site is deployed to a Cloudflare Pages project named `oceans-llm-docs`.

Why:

- VitePress already emits static assets with no runtime dependency,
- Cloudflare Pages is operationally smaller than introducing a custom docs container or SSR service,
- the public site benefits from Cloudflare's edge delivery without changing the docs authoring model.

### 2. Keep `oceans-llm.com` as the canonical public docs hostname

The canonical production URL is `https://oceans-llm.com`.

Why:

- the product name already matches the root domain,
- the apex domain is the simplest public address to communicate,
- it avoids splitting canonical docs identity across `www`, `pages.dev`, and repo-local links.

### 3. Redirect `www.oceans-llm.com` to the apex at the Cloudflare zone layer

`https://www.oceans-llm.com` should redirect permanently to `https://oceans-llm.com`.

Why:

- users will try both hostnames,
- the redirect belongs to domain-level traffic management,
- keeping the redirect at the zone layer avoids coupling redirect behavior to VitePress output structure.

Operational consequence:

- the redirect is a required Cloudflare setup step, but it is not fully repo-managed in this pass.

### 4. Deploy docs from the existing tag-triggered release workflow

The release workflow now deploys the docs site on `v*` tag pushes, alongside the existing release distribution jobs.

Why:

- docs deployment becomes part of the same release boundary as image publication,
- the public docs site updates only from intentional releases, not every merge to `main`,
- maintainers do not need a second manual publish step after cutting a release.

### 5. Encode the deploy command in the docs package and expose it through `mise`

The checked-in deploy command lives in `docs/package.json` and is executed via `mise run docs-deploy`.

Why:

- the repo already uses `mise` as the tooling contract,
- the deployment command stays close to the docs package that owns the built artifact,
- future workflow changes can reuse the same local task instead of duplicating shell commands.

### 6. Keep the docs site static-only in this slice

This deployment does not introduce Pages Functions, Workers logic, or custom runtime code.

Why:

- the current site only needs static hosting,
- static asset delivery is cheaper and operationally simpler,
- adding dynamic logic would create new billing, testing, and failure modes with no immediate product value.

## Implementation notes

The checked-in implementation establishes:

- a pinned `wrangler` dependency in the docs package,
- a `docs:deploy` package script that deploys `.vitepress/dist` to the `oceans-llm-docs` Pages project,
- a `mise run docs-deploy` task that builds, verifies, and deploys the site,
- release workflow integration that runs the docs deployment on tag pushes,
- production docs metadata in VitePress for the public hostname.

The Cloudflare account still needs one-time dashboard setup:

- attach the custom domain `oceans-llm.com` to the Pages project,
- attach `www.oceans-llm.com`,
- configure a permanent redirect from `www` to the apex,
- store `CLOUDFLARE_ACCOUNT_ID` and `CLOUDFLARE_API_TOKEN` as GitHub Actions secrets.

## Alternatives considered

### Git-integrated Cloudflare Pages builds

Pros:

- fewer explicit deploy commands in the repo,
- native Pages preview/build flow.

Rejected because:

- the repo already has a release-oriented GitHub Actions contract,
- the deployment behavior would be split between GitHub Actions and Cloudflare-managed builds,
- direct upload from CI is more explicit for release-time publication.

### GitHub Pages

Pros:

- no additional hosting vendor,
- simple for static artifacts.

Rejected because:

- the request explicitly targets Cloudflare and `oceans-llm.com`,
- Cloudflare Pages aligns better with the desired custom-domain edge hosting model,
- keeping the site on Cloudflare avoids another CDN/domain handoff layer.

### Shipping docs inside an app container

Pros:

- one fewer external hosting platform,
- deployment could piggyback on the existing release images.

Rejected because:

- a static docs site does not justify a long-running application runtime,
- serving docs from a container would add avoidable infrastructure and operational coupling,
- docs outages should not be tied to gateway or admin UI runtime concerns.

### Deploy docs on every push to `main`

Pros:

- faster public docs updates,
- less coupling to release cadence.

Rejected because:

- this repo already treats tags as the distribution boundary,
- publish-on-merge would let unreleased behavior appear in the public docs site,
- keeping docs deployment aligned with releases is easier to reason about operationally.

## Follow-up

- If docs previews on pull requests become important, add a separate preview workflow and treat it as non-canonical.
- If the `www` redirect needs to become fully repo-managed, evaluate Cloudflare Rules or Terraform-backed zone automation in a separate change.
