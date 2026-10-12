# Release Process

`See also`: [Contributing](../../../CONTRIBUTING.md), [Deploy and Operations](../../setup/deploy-and-operations.md), [Admin Runbooks](../../operations/operator-runbooks.md), [ADR: Cocogitto Releases, git-cliff Changelogs, and GHCR Image Publishing](../../adr/2026-03-06-release-versioning-and-ghcr-publishing.md)

This runbook explains how maintainers publish an Oceans LLM release.

## Release Contract

Oceans LLM uses one Semantic Version for the CLI, gateway, admin UI, container images, and Helm chart. A tag named `vX.Y.Z` identifies the source for all release artifacts.

Run one command to create and publish a release:

```bash
mise run release
```

The command creates the version commit and tag, then pushes them. The pushed tag starts CI, which publishes the GitHub release after the CLI checks, images, and Helm chart succeed.

## Source Files

- [mise.toml](../../../mise.toml) defines the release and changelog tasks.
- [cog.toml](../../../cog.toml) defines versioning and pre-bump hooks.
- [cliff.toml](../../../cliff.toml) defines changelog content and layout.
- [release.yml](../../../.github/workflows/release.yml) builds and publishes release artifacts.
- [dist-workspace.toml](../../../dist-workspace.toml) configures CLI targets, archives, installers, and publication gates.
- [release-cli-check.yml](../../../.github/workflows/release-cli-check.yml) checks packaged binaries and tests CLI behavior on each target operating system.
- [release-product.yml](../../../.github/workflows/release-product.yml) publishes container images and the Helm chart.
- [Helm chart](../../../deploy/helm/oceans-llm/README.md) defines the Kubernetes package.

## Merge and Changelog Rules

Pull request titles must follow Conventional Commits. Merge commits include the pull request number, author, and link in the generated changelog.

git-cliff removes duplicate entries across each release. It keeps the later commit so the pull request link remains. It also keeps distinct scoped entries and any breaking-change marker from the duplicate commits.

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
7. Start the tag workflow, which publishes the release after its required jobs pass.

The release tag points to a commit that contains the Cargo version changes and the new changelog section.

## Distribution Workflow

The pushed `vX.Y.Z` tag starts [release.yml](../../../.github/workflows/release.yml). cargo-dist generates this workflow from [dist-workspace.toml](../../../dist-workspace.toml). The workflow:

- Builds only `oceans-cli` and its dependencies using the Rust toolchain configured in `mise.toml`.
- Packages the `oceans` executable for Linux x86-64 and ARM64, macOS Intel and Apple Silicon, and Windows x86-64.
- Creates `.tar.gz` archives for Unix targets and `.zip` archives for Windows, with SHA-256 checksums and provenance attestations.
- Creates shell and PowerShell installers. The archives include the license, README, and changelog.
- Checks each archive checksum, runs its version and help commands, and tests the CLI and shared bundle code on each target operating system.
- Builds and publishes the gateway image for `linux/amd64`.
- Builds and publishes the admin UI image for `linux/amd64` and `linux/arm64`.
- Adds provenance attestations to both images.
- Validates, packages, and publishes the Helm chart after both image jobs pass.
- Creates the GitHub release only after those jobs succeed. Release notes come from the git-cliff changelog, with installation instructions added by cargo-dist.

Linux binaries are built on Ubuntu 22.04 runners. Check the cargo-dist linkage report before claiming support for an older distribution. The initial release targets use glibc; musl archives are not configured.

Prerelease tags produce GitHub prereleases and versioned container images. They do not update the container `latest` tags.

The generated release workflow also builds and checks CLI artifacts on pull requests. It stores them as workflow artifacts and does not publish images, charts, or GitHub releases for pull requests.

The workflow publishes the Helm chart to:

```text
oci://ghcr.io/ahstn/charts/oceans-llm
```

For a tag named `vX.Y.Z`, the chart version is `X.Y.Z` and its `appVersion` is `vX.Y.Z`.

## Maintain Release Tooling

cargo-dist and cargo-release are installed through `mise` with pinned versions. Cocogitto owns the version commit and tag. Its cargo-release hook updates all workspace Cargo versions, and its git-cliff hook writes `CHANGELOG.md` before that commit.

Change distribution settings in `dist-workspace.toml` and regenerate the workflow:

```bash
mise run dist-generate
mise run dist-check
mise run dist-plan
```

Do not edit the generated workflow directly. Keep product publication and CLI checks in their reusable workflows. `dist-check` verifies the generated file and lints all three release workflows.

To check local packaging without publishing a release:

```bash
mise run dist-build
```

cargo-dist reports artifact paths under `target/distrib`. It resolves the compiled executable through Cargo metadata, so the repository's shared Cargo cache paths do not require manual binary copies.

## Verify the Release

After the workflow finishes, verify:

- The GitHub release notes are correct.
- All five CLI archives, their checksums, and both installers exist.
- The downloaded CLI reports the version from the release tag.
- CLI archive attestations can be verified with `gh attestation verify FILE --repo ahstn/oceans-llm` through `mise exec`.
- The gateway and admin UI image tags exist.
- Image digests and provenance attestations exist.
- The Helm chart version exists at the expected OCI path.
- The deploy documentation matches the published image platforms.

If the release changed behavior for admins or users, confirm that the canonical documentation describes that behavior.

## Failure Recovery

If the command fails before it pushes the tag, inspect the local commit, tag, and worktree before you rerun it.

If the tag was pushed but a required CI job failed, correct the problem and rerun the failed jobs for that tag when the source is valid. A failed prerequisite prevents the final GitHub release job. Images or charts that were already pushed remain published; there is no rollback across registries.

If GitHub publication failed, inspect whether a release or partial asset set exists before rerunning the final job. Do not replace published assets or move a published tag. A source correction requires a new version and tag.

The workflow change applies to tags whose source includes this configuration. Rerunning an old tag uses that tag's original workflow.

## CI Boundary

Normal CI is the quality gate for the release source. Tag CI builds and publishes the distribution artifacts. The release command does not replace either gate.
