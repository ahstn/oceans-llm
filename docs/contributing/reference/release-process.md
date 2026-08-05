# Release Process

`See also`: [Contributing](../../../CONTRIBUTING.md), [Deploy and Operations](../../setup/deploy-and-operations.md), [Admin Runbooks](../../operations/operator-runbooks.md), [ADR: Cocogitto Releases, git-cliff Changelogs, and GHCR Image Publishing](../../adr/2026-03-06-release-versioning-and-ghcr-publishing.md)

This runbook explains how maintainers prepare, review, publish, and verify an Oceans LLM release.

## Release Contract

Oceans LLM uses one Semantic Version for the gateway, admin UI, container images, and Helm chart. A tag named `vX.Y.Z` identifies the source for all release artifacts.

The release has three maintainer gates:

1. `mise run release` prepares a local release commit and tag.
2. `mise run release-publish` pushes the reviewed commit and tag, then creates a draft GitHub release.
3. `mise run release-finalize` publishes the GitHub release after distribution checks pass.

The first command has no remote side effects. The second command starts distribution because the pushed tag triggers GitHub Actions.

## Source Files

- [mise.toml](../../../mise.toml) defines the release commands.
- [cog.toml](../../../cog.toml) defines versioning and pre-bump hooks.
- [cliff.toml](../../../cliff.toml) defines changelog content and layout.
- [release.yml](../../../.github/workflows/release.yml) builds and publishes release artifacts.
- [Helm chart](../../../deploy/helm/oceans-llm/README.md) defines the Kubernetes package.

## Merge and Changelog Rules

The repository uses squash merges. The pull request title becomes the commit title on `main`, so each pull request produces one changelog candidate. Pull request titles must follow Conventional Commits.

git-cliff also removes merge commits when it reads old history. This prevents a merge title and its source commit from producing duplicate entries.

The changelog contains user-facing changes under these headings:

- `Added` for new behavior.
- `Changed` for changes to existing behavior.
- `Fixed` for bug fixes.
- `Security` for `fix(security)` commits.

Build, chore, CI, documentation, style, and test commits do not appear by default. Use a `changelog: ignore` commit footer when another conventional commit must not appear. Breaking commits remain visible even when their type is normally hidden.

The changelog is a release record, not a copy of the Git log. Commit titles must explain the effect of a notable change.

## Release Preflight

Before release preparation:

- Update local `main` from `origin/main`.
- Confirm that the worktree is clean.
- Confirm that normal CI passed for the current commit.
- Confirm that generated admin contract files are current.
- Confirm that each changelog-worthy commit has a clear title.
- Set a valid `GITHUB_TOKEN` to let git-cliff read GitHub contributor and pull request data.

Run the preview:

```bash
mise run release-dry-run
```

The preview calculates the next version, previews Cargo version changes, and renders the next changelog without changing the repository.

## Prepare the Release

Run this command from a clean `main` branch:

```bash
mise run release
```

The command completes these steps:

1. Verify that the worktree is clean.
2. Update the pricing catalog.
3. Ask Cocogitto to calculate the next version.
4. Update workspace Cargo versions through a Cocogitto pre-bump hook.
5. Regenerate `CHANGELOG.md` through a Cocogitto pre-bump hook.
6. Create the local release commit and `vX.Y.Z` tag.

The tag points to a commit that contains the Cargo version changes and the new changelog section. The command does not push the commit or tag and does not create a GitHub release.

## Review the Prepared Release

Get the prepared tag and confirm that it points to `HEAD`:

```bash
tag="$(cog get-version --tag)"
test "$(git rev-parse "${tag}^{commit}")" = "$(git rev-parse HEAD)"
```

Review these items before publication:

- The version matches the expected impact.
- `CHANGELOG.md` has no duplicate or internal-only entries.
- Cargo manifests and `Cargo.lock` use the new version.
- The release commit contains only expected generated and version files.
- The worktree is clean.

Useful review commands:

```bash
git status --short
git show --stat --decorate HEAD
git show "${tag}:CHANGELOG.md"
```

Do not publish a release that needs edits. The tag is still local, so remove the local release tag and repair the release commits before you run the preparation step again. Inspect `git reflog` before any history repair so the release commit remains recoverable.

## Publish the Tag and Draft Release

After review, run:

```bash
mise run release-publish
```

The command:

1. Confirms that the current branch is `main` and the worktree is clean.
2. Confirms that the release tag points to `HEAD`.
3. Extracts the newest release section from the committed `CHANGELOG.md` into a temporary notes file.
4. Pushes `main` and the tag in one atomic Git operation.
5. Creates a draft GitHub release from the pushed tag and the reviewed notes file.

An atomic push prevents the branch and tag from moving separately. The draft release lets maintainers verify the final Markdown before publication.

If GitHub release creation fails after the push, fix the GitHub CLI or permission problem and run `mise run release-publish` again. The repeated Git push is safe when the same commit and tag already exist on the remote.

## Distribution Workflow

The pushed `v*` tag triggers [release.yml](../../../.github/workflows/release.yml). The workflow:

- Builds and publishes the gateway image for `linux/amd64`.
- Builds and publishes the admin UI image for `linux/amd64` and `linux/arm64`.
- Adds provenance attestations to both images.
- Validates, packages, and publishes the Helm chart after both image jobs pass.

The workflow publishes the Helm chart to:

```text
oci://ghcr.io/ahstn/charts/oceans-llm
```

For a tag named `vX.Y.Z`, the chart version is `X.Y.Z` and its `appVersion` is `vX.Y.Z`.

The workflow does not create, edit, or publish the draft GitHub release.

## Verify and Finalize

After the tag workflow passes, verify:

- The gateway and admin UI image tags exist.
- Image digests and provenance attestations exist.
- The Helm chart version exists at the expected OCI path.
- The draft release notes match the reviewed changelog section.
- The deploy documentation still matches the published image platforms.

If the release changed behavior for admins or users, confirm that the canonical documentation describes that behavior.

Publish the verified draft release:

```bash
mise run release-finalize
```

## Failure Recovery

### Preparation failed before tag creation

Inspect the worktree and command output. Correct the local problem, restore a clean starting state, and run `mise run release` again.

### Preparation failed after local tag creation

Do not push the tag. Confirm the tag and release commit with `git show` and `git reflog`. Remove only the local release state that you inspected, then rerun preparation.

### Atomic push failed

Neither `main` nor the tag should move when the remote supports atomic pushes. Update local `main`, resolve the conflict, prepare a new valid release state, and retry.

### Tag workflow failed

Keep the GitHub release as a draft. Fix the workflow problem and rerun the failed jobs for the same tag when the source is valid. Do not move a published tag to a different commit.

### Draft release is wrong

Edit the draft or correct its notes before finalization. If the tag source is wrong, stop distribution and follow the tag recovery policy. Do not publish the draft to hide a source or artifact mismatch.

## CI Boundary

Normal CI is the quality gate for the release source. Tag CI is the distribution gate. The release commands do not replace either gate.

Maintainers must start from a known-good `main` commit. The draft release must stay unpublished until the tag workflow and artifact checks pass.
