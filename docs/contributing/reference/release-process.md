# Release Process

`See also`: [Contributing](../../../CONTRIBUTING.md), [Deploy and Operations](../../setup/deploy-and-operations.md), [Admin Runbooks](../../operations/operator-runbooks.md), [ADR: Cocogitto Releases, git-cliff Changelogs, and GHCR Image Publishing](../../adr/2026-03-06-release-versioning-and-ghcr-publishing.md), [ADR: git-cliff Owns Release Versioning and a Pull-Request Changelog](../../adr/2026-10-18-git-cliff-release-versioning-and-pull-request-changelog.md)

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
- [scripts/release.sh](../../../scripts/release.sh) prepares the version commit and tag.
- [cliff.toml](../../../cliff.toml) defines which pull requests are released, how they bump the version, and the changelog layout.
- [release.yml](../../../.github/workflows/release.yml) builds and publishes release artifacts.
- [dist-workspace.toml](../../../dist-workspace.toml) configures CLI targets, archives, installers, and publication gates.
- [release-cli-check.yml](../../../.github/workflows/release-cli-check.yml) runs the packaged binaries on each target operating system. CLI behavior is tested before merge by `cli-ci.yml` and `ci.yml`.
- [release-product.yml](../../../.github/workflows/release-product.yml) publishes container images and the Helm chart.
- [Helm chart](../../../deploy/helm/oceans-llm/README.md) defines the Kubernetes package.

## Merge and Changelog Rules

Pull request titles must follow Conventional Commits. The changelog and release notes list one entry per merged pull request, using its title, author, and link. Commits inside a pull request and commits pushed straight to `main` do not appear and do not affect the version.

git-cliff recognizes a pull request by the `(#123)` suffix that GitHub adds to its merge commit title. Keep the repository's merge commit title set to the pull request title. Titles are untrusted, so the changelog escapes their link and HTML syntax (`[`, `]`, `<`, `>`); backticks still render as code.

The pull request title also chooses the version bump:

- A breaking change, marked with `!`, produces a major release.
- A `feat` pull request produces a minor release.
- Any other listed pull request produces a patch release.

The changelog uses these groups in this order:

1. `:rocket: New features` for `feat` pull requests.
2. `:bug: Bug fixes` for `fix` pull requests.
3. `Changed` for `perf`, `refactor`, `revert`, `docs`, `chore`, and other public changes.

Build, CI, style, and test pull requests do not appear unless their title marks a breaking change.

## Before a Release

Before you run the release command:

- Update local `main` from `origin/main`.
- Confirm that normal CI passed for the current commit.
- Confirm that generated admin contract files are current.
- Confirm that merged pull requests have clear titles. Preview the notes with `mise exec -- git-cliff --unreleased --bump`.
- Set a valid `GITHUB_TOKEN` for git-cliff and the GitHub CLI.

## Publish a Release

Run this command from `main`:

```bash
mise run release
```

The command completes these steps:

1. Refresh the vendored pricing catalog and the Artificial Analysis intelligence, coding, and agentic indexes. These run as `mise` dependencies of the release task.
2. Check that `main` matches `origin/main` and has no tracked changes other than those two snapshots.
3. Ask git-cliff to calculate the next version from the merged pull requests.
4. Update workspace Cargo versions.
5. Add the new section to the top of `CHANGELOG.md`.
6. Create the release commit and `vX.Y.Z` tag.
7. Push `main` and the tag to GitHub.
8. Start the tag workflow, which publishes the release after its required jobs pass.

The release tag points to a commit that contains the Cargo version changes and the new changelog section.

## Distribution Workflow

The pushed `vX.Y.Z` tag starts [release.yml](../../../.github/workflows/release.yml). cargo-dist generates this workflow from [dist-workspace.toml](../../../dist-workspace.toml). The workflow:

- Builds only `oceans-cli` and its dependencies using the Rust toolchain configured in `mise.toml`.
- Packages the `oceans` executable for Linux x86-64 and ARM64, macOS Intel and Apple Silicon, and Windows x86-64.
- Creates `.tar.gz` archives for Unix targets and `.zip` archives for Windows, with SHA-256 checksums and provenance attestations.
- Creates shell and PowerShell installers. The archives include the license, README, and changelog.
- Runs each archived executable's version and help commands on each target operating system. CLI and bundle tests run before merge in `cli-ci.yml` and `ci.yml`.
- Builds and publishes the gateway image for `linux/amd64`.
- Builds and publishes the admin UI image for `linux/amd64` and `linux/arm64`.
- Adds provenance attestations to both images.
- Validates, packages, and publishes the Helm chart after both image jobs pass.
- Creates the GitHub release only after those jobs succeed. Release notes come from the new `CHANGELOG.md` section, with installation instructions added by cargo-dist.

Linux binaries are built on Ubuntu 22.04 runners. Check the cargo-dist linkage report before claiming support for an older distribution. The initial release targets use glibc; musl archives are not configured.

Prerelease tags produce GitHub prereleases and versioned container images. They do not update the container `latest` tags.

On pull requests the generated release workflow only runs `dist plan`. It does not build CLI artifacts or publish images, charts, or GitHub releases.

The workflow publishes the Helm chart to:

```text
oci://ghcr.io/ahstn/charts/oceans-llm
```

For a tag named `vX.Y.Z`, the chart version is `X.Y.Z` and its `appVersion` is `vX.Y.Z`.

## Maintain Release Tooling

git-cliff, cargo-dist, and cargo-release are installed through `mise` with pinned versions. The release script owns the version commit and tag. It uses git-cliff for the version and changelog, and cargo-release for workspace Cargo versions.

Change distribution settings in `dist-workspace.toml` and regenerate the workflow:

```bash
mise run dist-generate
mise run dist-check
mise run dist-plan
```

Do not edit the generated workflow directly. Action commits in the generated workflow come from `[dist.github-action-commits]`, and Dependabot is configured to skip `release.yml`. When Dependabot bumps an action that the generated workflow also uses, such as `actions/upload-artifact` in `ci.yml`, copy the new commit into `dist-workspace.toml` and regenerate. Keep product publication and CLI checks in their reusable workflows. `dist-check` verifies the generated file and lints all three release workflows.

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

If the command fails before it pushes the tag, inspect the local commit, tag, and worktree before you rerun it. The script refuses to run on a dirty worktree. When there is nothing to release, it restores the refreshed snapshots before it exits. To start over, delete any local `vX.Y.Z` tag it created and reset `main` to `origin/main`.

If the tag was pushed but a required CI job failed, correct the problem and rerun the failed jobs for that tag when the source is valid. A failed prerequisite prevents the final GitHub release job. Images or charts that were already pushed remain published; there is no rollback across registries.

If GitHub publication failed, inspect whether a release or partial asset set exists before rerunning the final job. Do not replace published assets or move a published tag. A source correction requires a new version and tag.

The workflow change applies to tags whose source includes this configuration. Rerunning an old tag uses that tag's original workflow.

## CI Boundary

Normal CI is the quality gate for the release source. Tag CI builds and publishes the distribution artifacts. The release command does not replace either gate.
