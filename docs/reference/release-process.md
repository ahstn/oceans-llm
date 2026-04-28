# Release Process

`See also`: [Contributing](../../CONTRIBUTING.md), [Deploy and Operations](../setup/deploy-and-operations.md), [Admin Runbooks](../operations/operator-runbooks.md), [ADR: Cocogitto Releases, git-cliff Changelogs, and GHCR Image Publishing](../adr/2026-03-06-release-versioning-and-ghcr-publishing.md)

This page is the maintainer-facing release runbook.

## Source of Truth

- local release task:
  - [../mise.toml](../../mise.toml)
- release workflow:
  - [../.github/workflows/release.yml](../../.github/workflows/release.yml)
- Helm chart:
  - [../../deploy/helm/oceans-llm](../../deploy/helm/oceans-llm/README.md)
- changelog config:
  - [../cliff.toml](../../cliff.toml)

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
3. review the generated version changes and `CHANGELOG.md`
4. confirm the GitHub release was created for the new tag
5. push the release commit and tag
6. let the tag-triggered GitHub Actions workflow build, attest, and publish images, then publish the Helm chart

## What `mise run release` Does

Current task steps:

1. `cog bump --auto --skip-untracked`
2. `git-cliff -o CHANGELOG.md`
3. `gh release create v$(cog get-version) ...`
4. `cargo release version $(cog get-version) --execute`

That means release metadata and version updates are authored locally before any push happens. The GitHub release is not created as a draft by this task.

## What GitHub Actions Does

The pushed `v*` tag triggers [../.github/workflows/release.yml](../../.github/workflows/release.yml).

Current workflow responsibilities:

- build and publish the gateway image
- build and publish the admin UI image
- attest image provenance
- validate, package, and publish the Helm chart after both image jobs succeed

The workflow does not create or update the GitHub release body. It consumes the pushed tag as the image distribution trigger.

## Helm Chart Publishing

The release workflow publishes:

```bash
oci://ghcr.io/ahstn/charts/oceans-llm
```

For a tag `vX.Y.Z`, the chart is packaged with:

- chart version: `X.Y.Z`
- chart appVersion: `vX.Y.Z`

The publish step runs `mise run helm-check`, packages [../../deploy/helm/oceans-llm](../../deploy/helm/oceans-llm/README.md), logs in to GHCR with the workflow token, and pushes the package with `helm push ... oci://ghcr.io/ahstn/charts`.

The push target intentionally omits the chart basename and tag. Helm infers `oceans-llm:X.Y.Z` from the packaged chart name and version.

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
- the expected chart version was published at `oci://ghcr.io/ahstn/charts/oceans-llm`
- the release notes look sane
- the deploy docs still match the image reality

If the release changed admin- or user-visible behavior, update the canonical docs in the same pass.

## CI Responsibility Boundary

The release workflow does not explicitly depend on prior CI runs.

In practice:

- maintainers are responsible for cutting releases from a known-good state
- normal CI is the preflight signal
- tag CI is the distribution step

## Failure Recovery Notes

- If `mise run release` creates a release but local version changes fail afterward, inspect the working tree before rerunning.
- If the tag was not pushed, fix the local state and either reuse or delete the created GitHub release deliberately.
- If the tag was pushed but image or chart publishing failed, fix the workflow issue and rerun the failed workflow for the same tag when possible.
- Avoid retagging an existing published version unless the release is still private to maintainers and no deploy path consumed it.
