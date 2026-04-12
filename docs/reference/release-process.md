# Release Process

`See also`: [Contributing](../../CONTRIBUTING.md), [Deploy and Operations](../setup/deploy-and-operations.md), [Operator Runbooks](../operations/operator-runbooks.md), [ADR: Cocogitto Releases, git-cliff Changelogs, and GHCR Image Publishing](../adr/2026-03-06-release-versioning-and-ghcr-publishing.md), [ADR: Cloudflare Pages Deployment For Public Docs](../adr/2026-03-31-cloudflare-pages-docs-deployment.md)

This page is the maintainer-facing release runbook.

## Source of Truth

- local release task:
  - [../mise.toml](../../mise.toml)
- release workflow:
  - [../.github/workflows/release.yml](../../.github/workflows/release.yml)
- docs deploy task:
  - [../mise.toml](../../mise.toml)
- docs package deploy command:
  - [../package.json](../package.json)
- changelog config:
  - [../cliff.toml](../../cliff.toml)

## Release Preflight

Before `mise run release`, confirm:

- `main` is up to date locally
- the intended release state is already merged
- normal CI is green for that commit
- generated admin contract artifacts are current
- `CLOUDFLARE_ACCOUNT_ID` and `CLOUDFLARE_API_TOKEN` GitHub secrets still exist for the docs deploy job
- changelog-worthy commits are in the expected shape

The tag workflow is distribution, not the quality gate.

## Current Release Flow

1. update `main` locally
2. run `mise run release`
3. review the generated release commit, tag, changelog, and GitHub release draft state
4. push the release commit and tag
5. let the tag-triggered GitHub Actions workflow build and publish images
6. let the same tag workflow build and deploy the public docs site to `https://oceans-llm.com`

## What `mise run release` Does

Current task steps:

1. `cog bump --auto --skip-untracked`
2. `git-cliff -o CHANGELOG.md`
3. `gh release create v$(cog get-version) ...`
4. `cargo release version $(cog get-version) --execute`

That means release metadata and version updates are authored locally before any push happens.

## What GitHub Actions Does

The pushed `v*` tag triggers [../.github/workflows/release.yml](../../.github/workflows/release.yml).

Current workflow responsibilities:

- build, verify, and deploy the public VitePress docs site to Cloudflare Pages
- build and publish the gateway image
- build and publish the admin UI image
- attest image provenance

## Docs Site Release Prerequisites

The docs deployment is release-driven, but Cloudflare still needs one-time project setup outside the repo.

Required Cloudflare state:

- Pages project:
  - `oceans-llm-docs`
- production custom domain:
  - `oceans-llm.com`
- secondary hostname:
  - `www.oceans-llm.com`
- redirect policy:
  - `www.oceans-llm.com` permanently redirects to `https://oceans-llm.com`

Required GitHub Actions secrets:

- `CLOUDFLARE_ACCOUNT_ID`
- `CLOUDFLARE_API_TOKEN`

The repo-managed workflow only performs the build and deploy. Custom-domain attachment and the `www` redirect remain Cloudflare-managed prerequisites.

## Current Image Reality

The workflow is not symmetric across both deployables today:

- gateway image:
  - `linux/amd64`
- admin UI image:
  - `linux/amd64`
  - `linux/arm64`

## Post-Release Verification

After the workflow finishes, verify:

- the GitHub release exists for the pushed tag
- the expected image tags were published
- `https://oceans-llm.com` serves the new docs deployment
- `https://www.oceans-llm.com` redirects to `https://oceans-llm.com`
- the release notes look sane
- the deploy docs still match the image reality

If the release changed operator-visible behavior, update the canonical docs in the same pass.

## CI Responsibility Boundary

The release workflow does not explicitly depend on prior CI runs.

In practice:

- maintainers are responsible for cutting releases from a known-good state
- normal CI is the preflight signal
- tag CI is the distribution step
