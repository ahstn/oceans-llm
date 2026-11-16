# Release Process

`See also`: [Contributing](../../../CONTRIBUTING.md), [Topology and operations](https://oceans-llm.com/setup/deploy-and-operations/), [Admin Runbooks](https://oceans-llm.com/operations/admin-runbooks/), [ADR: Cocogitto Releases, git-cliff Changelogs, and GHCR Image Publishing](../../adr/2026-03-06-release-versioning-and-ghcr-publishing.md), [ADR: git-cliff Owns Release Versioning and a Pull-Request Changelog](../../adr/2026-10-18-git-cliff-release-versioning-and-pull-request-changelog.md)

This runbook explains how maintainers publish an Oceans LLM release.

## Release Contract

Oceans LLM uses one Semantic Version for the CLI, gateway, admin UI, container images, and Helm chart. A tag named `vX.Y.Z` identifies the source for all release artifacts.

The [`@ahstn/opencode-oceans` plugin](https://www.npmjs.com/package/@ahstn/opencode-oceans) has independent versions, release notes and `opencode-oceans-vX.Y.Z` tags. It is not released by `mise run release` or cargo-dist. See [OpenCode plugin releases](#opencode-plugin-releases).

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

1. Refresh the vendored models.dev, OpenRouter, and LiteLLM catalog snapshots and the Artificial Analysis intelligence, coding, and agentic indexes. These run as `mise` dependencies of the release task.
2. Check that `main` matches `origin/main` and has no tracked changes other than those snapshots.
3. Ask git-cliff to calculate the next version from the merged pull requests.
4. Render the updated changelog in a temporary file, including GitHub author data.
5. Update workspace Cargo versions and replace `CHANGELOG.md` with the rendered file.
6. Create the release commit and `vX.Y.Z` tag.
7. Push `main` and the tag to GitHub.
8. Start the tag workflow, which publishes the release after its required jobs pass.

The release tag points to a commit that contains the Cargo version changes and the new changelog section.

## Distribution Workflow

The pushed `vX.Y.Z` tag starts [release.yml](../../../.github/workflows/release.yml). A merge to `main` without a tag does not start a release. cargo-dist generates this workflow from [dist-workspace.toml](../../../dist-workspace.toml). The workflow:

- Checks that the tag is a version tag, names the pushed commit, and that this commit is on `main`. Push `main` before or with the tag, as `mise run release` does, or the check fails.
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

On pull requests, [release-plan.yml](../../../.github/workflows/release-plan.yml) runs `dist plan` with read-only permissions. It does not build CLI artifacts or publish images, charts, or GitHub releases.

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

If GitHub metadata lookup or changelog rendering fails, Cargo versions and `CHANGELOG.md` stay unchanged. The refreshed snapshots remain in the worktree; rerun `mise run release` when GitHub is available.

If a later step fails before it pushes the tag, inspect the local commit, tag, and worktree before you rerun it. The script refuses to run on a dirty worktree. Preserve changes you need before you undo a partial release. When there is nothing to release, it restores the refreshed snapshots before it exits.

If the tag was pushed but a required CI job failed, correct the problem and rerun the failed jobs for that tag when the source is valid. A failed prerequisite prevents the final GitHub release job. Images or charts that were already pushed remain published; there is no rollback across registries.

If GitHub publication failed, inspect whether a release or partial asset set exists before rerunning the final job. Do not replace published assets or move a published tag. A source correction requires a new version and tag.

The workflow change applies to tags whose source includes this configuration. Rerunning an old tag uses that tag's original workflow.

## OpenCode plugin releases

The plugin ships TypeScript source on public npm. Its versions and release notes are independent of the CLI, gateway, images and Helm chart. Do not use a product `vX.Y.Z` tag or the product changelog as the plugin's release identity. Record plugin-specific changes in release notes for the matching `opencode-oceans-vX.Y.Z` tag.

### Trusted publisher setup

Use npm 11.15.0 or later with Node 22.14.0 or later through `mise` for staged publishing. Enable 2FA on the maintainer account. Configure the npm trusted publisher for this public package:

| Field | Value |
| --- | --- |
| Organisation or user | `ahstn` |
| Repository | `oceans-llm` |
| Workflow filename | `opencode-oceans.yml` |
| Environment | `npm-opencode` |
| Allowed actions | `npm stage publish` only |

Create the `npm-opencode` GitHub environment and allow deployments from `main` only. Protect `main` and plugin release tags from unreviewed changes. Use GitHub-hosted runners and validate a new trusted publisher within npm's two-day window. Set npm publishing access to require 2FA and disallow tokens once trusted publishing is configured. Do not put a long-lived npm token in CI. See [npm trusted publishers](https://docs.npmjs.com/trusted-publishers/) and [staged publishing](https://docs.npmjs.com/staged-publishing/).

### Prepare and stage

1. Choose the next plugin version, review dependency updates and plugin-specific changes, update the package version and lockfile, and merge the release source to `main`. Run `mise run //opencode:check` and `mise run //opencode:pack` as appropriate before release. The check includes the source smoke: it packs this checkout and runs OpenCode against a local fixture.
2. Create and push the exact tag `opencode-oceans-vX.Y.Z` on the merged commit. Write release notes for that plugin version. Never move a published release tag.
3. Dispatch the [OpenCode Oceans plugin workflow](../../../.github/workflows/opencode-oceans.yml) from `main`, supplying that existing tag. The tag must point to the exact workflow-dispatch commit (`github.sha`), not just an ancestor of `main`, so npm provenance identifies the source being staged. The read-only check job validates that match, main ancestry, the tag, version, package name, public npm registry and access. It checks and packs the tagged source, then uploads the tarball and checksum. If `main` advances, rerun the original workflow run; a fresh dispatch against the older tag fails intentionally.
4. The `npm-opencode` stage job downloads that exact artifact, validates its checksum and stages it with short-lived OIDC credentials using `npm stage publish`, public access, `latest` and `--ignore-scripts`. Only this job has `id-token: write`. CI cannot approve the stage or publish directly.

### Review, approve and verify

Sign in to npm as a maintainer. List and inspect the candidate with `mise exec -- npm stage list @ahstn/opencode-oceans` and `mise exec -- npm stage view <stage-id>`, then download it with `mise exec -- npm stage download <stage-id>`. Inspect its version and contents, compare the downloaded tarball's SHA-256 checksum with the workflow artifact checksum, and check the provenance and workflow commit. Later OIDC stages should have npm provenance; the first locally staged `0.1.0` release has no CI provenance.

Reject an incorrect candidate with `mise exec -- npm stage reject <stage-id>` before trying again. A staged version is reserved until rejected. Correct the source as needed, then use an appropriate matching tag and dispatch again; do not move a published tag. A maintainer approves a correct candidate with `mise exec -- npm stage approve <stage-id>` and 2FA. Approval publishes it with the staged `latest` tag; CI does not approve it.

After approval, run `mise run //opencode:smoke-published -- X.Y.Z` with the exact published version, or dispatch the [published smoke workflow](../../../.github/workflows/opencode-oceans-published.yml) with that version. In a fresh isolated environment, this runs `opencode plugin add @ahstn/opencode-oceans@X.Y.Z` before starting a private server. It installs from public npm without registry credentials or a source fallback and exercises a local synthetic gateway without paid requests. It is not the source smoke or the live provider harness suite. Record the result rather than assuming a successful stage proves installation.

Only after release, update the exact version pins in the package README, OpenCode setup page and landing page. Published npm versions are immutable. If a defect is found after approval, deprecate the affected version as appropriate and publish a fixed patch under a new version and tag; do not replace the published tarball.

## CI Boundary

Normal CI is the quality gate for the release source. Tag CI builds and publishes the distribution artifacts. The release command does not replace either gate.
