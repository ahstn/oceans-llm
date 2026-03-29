# Release Process

`Owns`: the maintainer release workflow, the local release task behavior, and the tag-triggered CI release distribution flow.
`Depends on`: [../CONTRIBUTING.md](../CONTRIBUTING.md)
`See also`: [deploy-and-operations.md](deploy-and-operations.md), [operator-runbooks.md](operator-runbooks.md), [adr/2026-03-06-release-versioning-and-ghcr-publishing.md](adr/2026-03-06-release-versioning-and-ghcr-publishing.md), [../mise.toml](../mise.toml), [../.github/workflows/release.yml](../.github/workflows/release.yml)

This page is the maintainer-facing release runbook.

## Source of Truth

- local release task:
  - [../mise.toml](../mise.toml)
- release workflow:
  - [../.github/workflows/release.yml](../.github/workflows/release.yml)
- changelog config:
  - [../cliff.toml](../cliff.toml)

## Release Preflight

Before `mise run release`, confirm:

- `main` is up to date locally
- the intended release state is already merged
- normal CI is green for that commit
- generated admin contract artifacts are current
- changelog-worthy commits are in the expected shape

The tag workflow is distribution, not the quality gate.

## Current Release Flow

1. update `main` locally
2. run `mise run release`
3. review the generated release commit, tag, changelog, and GitHub release draft state
4. push the release commit and tag
5. let the tag-triggered GitHub Actions workflow build and publish images

## What `mise run release` Does

Current task steps:

1. `cog bump --auto --skip-untracked`
2. `git-cliff -o CHANGELOG.md`
3. `gh release create v$(cog get-version) ...`
4. `cargo release version $(cog get-version) --execute`

That means release metadata and version updates are authored locally before any push happens.

## What GitHub Actions Does

The pushed `v*` tag triggers [../.github/workflows/release.yml](../.github/workflows/release.yml).

Current workflow responsibilities:

- build and publish the gateway image
- build and publish the admin UI image
- attest image provenance
- publish or update the GitHub release for the tag

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
- the release notes look sane
- the deploy docs still match the image reality

If the release changed operator-visible behavior, update the canonical docs in the same pass.

## CI Responsibility Boundary

The release workflow does not explicitly depend on prior CI runs.

In practice:

- maintainers are responsible for cutting releases from a known-good state
- normal CI is the preflight signal
- tag CI is the distribution step
