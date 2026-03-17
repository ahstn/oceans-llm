# Release Process

`Owns`: maintainer release workflow, local release task behavior, and tag-triggered CI release distribution.
`Depends on`: [../CONTRIBUTING.md](../CONTRIBUTING.md)
`See also`: [deploy-and-operations.md](deploy-and-operations.md), [adr/2026-03-06-release-versioning-and-ghcr-publishing.md](adr/2026-03-06-release-versioning-and-ghcr-publishing.md), [../mise.toml](../mise.toml), [../.github/workflows/release.yml](../.github/workflows/release.yml)

This page is the canonical maintainer-facing release runbook.

## Source of Truth

- local task: [../mise.toml](../mise.toml)
- release workflow: [../.github/workflows/release.yml](../.github/workflows/release.yml)
- changelog config: [../cliff.toml](../cliff.toml)

## Current Release Flow

1. Update `main` locally and confirm the intended release state.
2. Run `mise run release`.
3. Review the generated release commit, tag, changelog, and GitHub release draft state.
4. Push the release commit and tag.
5. Let the tag-triggered GitHub Actions workflow build and publish images.

Important current reality:

- `mise run release` creates the version bump, changelog, and GitHub release metadata locally
- the task does not push for you
- the maintainer is the gate between local release authoring and public CI distribution

## What `mise run release` Does

Current task steps:

1. `cog bump --auto --skip-untracked`
2. `git-cliff -o CHANGELOG.md`
3. `gh release create v$(cog get-version) ...`
4. `cargo release version $(cog get-version) --execute`

That means the task creates release metadata and Cargo version updates locally before any push happens.

## What GitHub Actions Does

The pushed `v*` tag triggers [../.github/workflows/release.yml](../.github/workflows/release.yml).

Current workflow responsibilities:

- build and publish the gateway image
- build and publish the admin UI image
- attest image provenance
- publish/update the GitHub release for the tag

## Current Image Reality

The workflow is not symmetric across both deployables today:

- gateway image: `linux/amd64`
- admin UI image: `linux/amd64` and `linux/arm64`

Current workflow tags:

- `vX.Y.Z`
- `sha-<fullsha>`
- `latest`

Floating `X.Y` tags are not part of the current live workflow.

## CI Responsibility Boundary

The release workflow does not explicitly depend on prior CI runs.

In practice:

- maintainers are responsible for cutting releases from a known-good state
- normal CI workflows are the preflight signal
- the tag-triggered workflow is the distribution step, not the quality gate

## When To Update Other Docs

- if workflow mechanics change, update this page first
- if release philosophy changes, update the ADR and link back here
- if deploy topology changes, update [deploy-and-operations.md](deploy-and-operations.md), not this page
