# Release Process

`See also`: [Contributing](../../../CONTRIBUTING.md), [Deploy and Operations](../../setup/deploy-and-operations.md), [Admin Runbooks](../../operations/operator-runbooks.md), [ADR: Cocogitto Releases, git-cliff Changelogs, and GHCR Image Publishing](../../adr/2026-03-06-release-versioning-and-ghcr-publishing.md)

This runbook explains how maintainers publish an Oceans LLM release.

## Release Contract

Oceans LLM uses one Semantic Version for the gateway, admin UI, container images, and Helm chart. A tag named `vX.Y.Z` identifies the source for all release artifacts.

Run one command to create and publish a release:

```bash
mise run release
```

The command creates the version commit and tag, pushes them, and creates a published GitHub release. The pushed tag starts the distribution workflow.

## Source Files

- [mise.toml](../../../mise.toml) defines the release command.
- [cog.toml](../../../cog.toml) defines versioning and pre-bump hooks.
- [cliff.toml](../../../cliff.toml) defines changelog content and layout.
- [release.yml](../../../.github/workflows/release.yml) builds and publishes release artifacts.
- [Helm chart](../../../deploy/helm/oceans-llm/README.md) defines the Kubernetes package.

## Merge and Changelog Rules

Pull request titles must follow Conventional Commits. Merge commits include the pull request number, author, and link in the generated changelog.

git-cliff removes duplicate messages within each release. When both a source commit and its merge commit have the same message, it keeps the later merge commit so the pull request link remains.

The changelog uses these groups in this order:

1. `:rocket: New features` for `feat` commits.
2. `:bug: Bug fixes` for `fix` commits.
3. `Changed` for `perf`, `refactor`, `revert`, `docs`, `chore`, and other public changes.

Build, CI, style, and test commits do not appear by default. Use a `changelog: ignore` commit footer when another conventional commit must not appear. Breaking commits remain visible even when their type is normally hidden.

## Before a Release

Before you run the release command:

- Update local `main` from `origin/main`.
- Confirm that normal CI passed for the current commit.
- Confirm that generated admin contract files are current.
- Confirm that changelog-worthy commits have clear titles.
- Set a valid `GITHUB_TOKEN` for git-cliff and the GitHub CLI.

You can preview the next version and changelog without creating a release:

```bash
mise run release-dry-run
```

## Publish a Release

Run this command from `main`:

```bash
mise run release
```

The command completes these steps:

1. Update the pricing catalog.
2. Ask Cocogitto to calculate the next version.
3. Update workspace Cargo versions.
4. Regenerate `CHANGELOG.md`.
5. Create the release commit and `vX.Y.Z` tag.
6. Push `main` and the tag to GitHub.
7. Create the published GitHub release with notes from git-cliff.

The release tag points to a commit that contains the Cargo version changes and the new changelog section.

## Distribution Workflow

The pushed `v*` tag starts [release.yml](../../../.github/workflows/release.yml). The workflow:

- Builds and publishes the gateway image for `linux/amd64`.
- Builds and publishes the admin UI image for `linux/amd64` and `linux/arm64`.
- Adds provenance attestations to both images.
- Validates, packages, and publishes the Helm chart after both image jobs pass.

The workflow publishes the Helm chart to:

```text
oci://ghcr.io/ahstn/charts/oceans-llm
```

For a tag named `vX.Y.Z`, the chart version is `X.Y.Z` and its `appVersion` is `vX.Y.Z`.

## Verify the Release

After the workflow finishes, verify:

- The GitHub release notes are correct.
- The gateway and admin UI image tags exist.
- Image digests and provenance attestations exist.
- The Helm chart version exists at the expected OCI path.
- The deploy documentation matches the published image platforms.

If the release changed behavior for admins or users, confirm that the canonical documentation describes that behavior.

## Failure Recovery

If the command fails before it pushes the tag, inspect the local commit, tag, and worktree before you rerun it.

If the tag was pushed but GitHub release creation failed, create the release for the same tag after you correct the GitHub CLI or permission problem.

If the distribution workflow failed, fix the workflow problem and rerun the failed jobs for the same tag when the source is valid. Do not move a published tag to a different commit.

## CI Boundary

Normal CI is the quality gate for the release source. Tag CI builds and publishes the distribution artifacts. The release command does not replace either gate.
