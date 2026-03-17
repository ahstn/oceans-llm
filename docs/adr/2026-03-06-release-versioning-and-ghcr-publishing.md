# ADR: Cocogitto Releases, git-cliff Changelogs, and GHCR Image Publishing

- Date: 2026-03-06
- Status: Accepted

## Context

Implemented by:

- [../release-process.md](../release-process.md)
- [../deploy-and-operations.md](../deploy-and-operations.md)

The repository had a single Rust CI workflow, no release workflow, no Docker image publishing, no release tags, and no structured release-note configuration. The codebase also ships two deployable applications:

- the Rust gateway on port `8080`,
- the admin UI on port `3000`, which the gateway reaches through `ADMIN_UI_UPSTREAM`.

We wanted to add release automation that:

- preserves the simple `main` plus short-lived feature/bug branch flow,
- does not introduce dedicated release branches or release PR churn,
- publishes one coherent product release for both deployables,
- generates a repo-managed changelog and GitHub release notes automatically,
- publishes versioned multi-architecture Docker images to GHCR,
- stays compatible with repo-local `mise` tasks and CI.

We also needed to account for current repo reality:

- historic merge titles on `main` are not consistently Conventional Commits,
- `main` was not protected,
- only generic GitHub labels existed,
- the admin UI production build needed to be reliable enough for release gating.

## Decision

### 1. Create releases locally from `main`, then publish from the tag workflow

Releases are created locally from `main` with `mise run release`. That task computes the next version, creates the release commit and tag, regenerates `CHANGELOG.md`, and prepares the release metadata. Maintainers then push the release commit and tag to GitHub, which triggers the GitHub Actions release workflow.

Why:
- every merge to `main` should not automatically become a public release,
- maintainers still get a deliberate batching point for release cadence,
- this avoids release branches, release PRs, and bot-owned version commits.

### 2. Use one repo-wide product version per release

Each release uses a single semver tag such as `v0.1.0`.

That version is shared by:
- the GitHub release,
- the gateway GHCR image,
- the admin UI GHCR image.

Why:
- the repo ships one backend application plus one frontend application as one product,
- internal Cargo crate versions are not the public deployment contract,
- a single product version is simpler to operate and communicate.

### 3. Use git tags and GitHub releases as the public version source of truth

We do not use Cargo package versions as the primary public release contract.

Why:
- this repo is not currently publishing crates to crates.io,
- Cargo-focused tools optimize for crate publication and version-file churn,
- product release identity is better represented by the git tag and release metadata.

### 4. Use Cocogitto to infer versions and create release tags locally

We use `cog bump --auto` to infer the next version from Conventional Commit history and create the release tag locally.

In practice this means:
- breaking changes produce a major release,
- `feat` commits produce a minor release,
- `fix` commits produce a patch release,
- other commit types do not independently force a version bump.

Why:
- it keeps versioning deterministic and reviewable,
- it aligns with standard Conventional Commit semver rules,
- the release step stays simple enough to run and understand locally.

### 5. Enforce Conventional Commits at PR title level

We enforce Conventional Commits on pull request titles and keep squash-only merges on `main`.

Why:
- with squash merge, the PR title becomes the final commit on `main`,
- this keeps release-version inference simple and stable,
- contributors can keep local branch history flexible while maintainers control the final merge title.

### 6. Keep Rust CI and add dedicated UI and release-tooling CI

We keep the existing Rust-only CI workflow, add a dedicated UI CI workflow through `mise`, and validate release tooling with repo-local tasks plus Docker image builds.

Why:
- Rust and admin UI checks should stay independently visible,
- release tooling should be tested before release day,
- the release workflow should depend on a green preflight, not untested release automation.

### 7. Use `git-cliff` for the repository changelog and GitHub release notes

We use `git-cliff` to generate the repo-managed changelog from Conventional Commit history.

Why:
- changelog rendering stays repo-owned and reviewable,
- the same commit history drives both version inference and release documentation,
- the tooling is lightweight and easy to run locally.

### 8. Publish GitHub releases after images are available

The pushed release tag triggers GitHub Actions, which builds and publishes both images, then publishes the GitHub release for that tag.

Why:
- a public release should not appear before both images exist,
- image publishing stays in CI where registry credentials and attestations already live,
- local release creation stays small while CI handles distribution.

### 9. Publish GHCR images for both deployables

Each release publishes:

- `ghcr.io/ahstn/oceans-llm-gateway`
- `ghcr.io/ahstn/oceans-llm-admin-ui`

with release and moving tags.

Why:
- GHCR is a natural fit for a GitHub-hosted repo,
- multi-arch images keep deployment options open,
- floating tags help operations without replacing immutable version tags.

## Release Flow Overview

The release flow is intentionally split into a small local step and a tag-driven CI step.

### Local release step

1. A maintainer updates `main` locally and runs `mise run release`.
2. `cog bump --auto` determines the next version from Conventional Commit history, creates the release commit, and creates the `vX.Y.Z` tag.
3. `git-cliff` regenerates `CHANGELOG.md` for the new release.
4. The maintainer pushes the release commit and tag to GitHub.

This keeps versioning and changelog generation explicit and easy to inspect before the release is published.

### GitHub Actions release step

1. The pushed `vX.Y.Z` tag triggers [../../.github/workflows/release.yml](../../.github/workflows/release.yml).
2. The workflow builds and publishes the gateway and admin UI images to GHCR.
3. The workflow applies the release image tags and provenance attestations.
4. The workflow publishes or updates the GitHub release associated with the tag.

This split keeps local release authoring simple while leaving image publication and release distribution to CI.

## Alternatives Considered

### `release-plz`

Pros:
- Rust-native,
- good Cargo workspace support,
- conventional commit driven,
- strong fit for crates.io publication.

Rejected because:
- it centers on release PRs and extra release branches,
- it is optimized for crate versioning rather than product releases for app images,
- it adds workflow complexity we do not need for this repo’s current deployment model.

### `release-please`

Pros:
- mature GitHub release automation,
- good monorepo support,
- conventional commit driven.

Rejected because:
- it also centers on release PRs,
- Rust workspace support is more configuration-heavy for this repo’s shape,
- it is still less aligned than a GitHub-native manual release for one product version.

### `cargo-release`

Pros:
- strong Cargo release and tagging workflow,
- good fit for local/manual crate publication.

Rejected because:
- it is Cargo-centric rather than product-release-centric,
- it does not solve admin UI image publishing or GitHub release-note composition cleanly,
- it would still leave us stitching together frontend and GHCR logic around it.

### `semantic-release`

Pros:
- strong Conventional Commit automation,
- integrated versioning, notes, and GitHub release support,
- large plugin ecosystem.

Rejected because:
- it is optimized for CI-owned releases rather than a simple local `mise run release` flow,
- keeping repo-managed changelogs, Cargo workspace bumps, image metadata, and GitHub release customization still required non-trivial glue,
- the resulting setup was more complex than needed for this repo’s release model.

### Automatic releases on every merge to `main`

Pros:
- minimal human intervention,
- shortest path from merge to release.

Rejected because:
- not every merge should become a public release,
- batching changes into one intentional release is operationally safer,
- it would make current non-conventional history and release-note hygiene more brittle.

## Consequences

Positive:
- releases are intentional and auditable,
- both deployables ship under one version,
- changelog and release notes are automatic and consistent,
- image publishing is tied directly to successful release execution,
- release automation stays understandable with small local tooling.

Tradeoffs:
- maintainers still need to trigger releases manually,
- PR titles now carry more operational weight,
- changelog generation and tag creation now happen locally rather than entirely in CI,
- Cargo package versions are not the public release identity for now,
- branch governance must be kept aligned with the workflow assumptions.

## Current Implementation Status

The live workflow is slightly narrower than the original ADR language:

- `mise run release` does not push automatically
- the current workflow publishes `vX.Y.Z`, `sha-<sha>`, and `latest`
- the gateway image is currently `linux/amd64` only
- the admin UI image is currently `linux/amd64` and `linux/arm64`

Treat [../release-process.md](../release-process.md) as the canonical operational description.

## Follow-up Work

- Apply and maintain GitHub repository settings so `main` stays squash-only and PR-gated.
- Ensure the local release task and tag-triggered CI workflow remain aligned.
- Revisit Cargo-level release automation only if the repo starts publishing crates externally.

## Attribution

This ADR was prepared through collaborative human + AI implementation/design work.
