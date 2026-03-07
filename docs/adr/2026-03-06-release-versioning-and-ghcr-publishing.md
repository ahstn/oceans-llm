# ADR: Manual Product Releases and GHCR Image Publishing

- Date: 2026-03-06
- Status: Accepted

## Context

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

### 1. Use manual `workflow_dispatch` releases from `main`

Releases are created by an explicit manual workflow run from `main`.

Why:
- every merge to `main` should not automatically become a public release,
- maintainers need a deliberate batching point for release cadence,
- this avoids introducing release branches or bot-created release PRs.

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

We do not add Cargo-centric version bump automation for this slice.

Why:
- this repo is not currently publishing crates to crates.io,
- Cargo-focused tools optimize for crate publication and version-file churn,
- product release identity is better represented by the git tag and release metadata.

### 4. Infer versions from Conventional Commit history

The release workflow computes the next version from commits since the last stable `vX.Y.Z` tag using these rules:

- any `!` or `BREAKING CHANGE:` -> major,
- any `feat` -> minor,
- any `fix` -> patch,
- otherwise do not release.

Why:
- it keeps versioning deterministic and reviewable,
- it aligns with standard Conventional Commit semver rules.

For the very first release, the workflow bootstraps a local `v0.0.0` baseline tag so the first real stable release can be `v0.1.0`.

### 5. Enforce Conventional Commits at PR title level

We enforce Conventional Commits on pull request titles and keep squash-only merges on `main`.

Why:
- with squash merge, the PR title becomes the final commit on `main`,
- this keeps release-version inference simple and stable,
- contributors can keep local branch history flexible while maintainers control the final merge title.

### 6. Keep Rust CI and add dedicated UI and release-tooling CI

We keep the existing Rust-only CI workflow, add a dedicated UI CI workflow through `mise`, and add release-tooling CI that validates the release analyzer, semantic-release configuration, and both Dockerfiles.

Why:
- Rust and admin UI checks should stay independently visible,
- release tooling should be tested before release day,
- the release workflow should depend on a green preflight, not untested release automation.

### 7. Publish GitHub releases after images are available

The release workflow analyzes `main`, builds and publishes both images for the analyzed commit, then runs `semantic-release` once to update `CHANGELOG.md`, bump workspace crate versions, create the release commit and tag, and publish the GitHub release with the image digests in the release body.

Why:
- a public release should not appear before both images exist,
- the tagged release commit should contain the changelog that describes it,
- the final published release can include both generated notes and concrete image references.

### 8. Use semantic-release plugins for changelog and release notes

We use `@semantic-release/release-notes-generator`, `@semantic-release/changelog`, `@semantic-release/git`, `@semantic-release/github`, and `@semantic-release/exec`.

Why:
- versioning, changelog generation, git tagging, and GitHub release publication stay inside one supported lifecycle,
- the repo can still commit `CHANGELOG.md` and workspace `Cargo.toml` updates,
- a small `prepare` hook is enough for Rust workspace version bumps before the release commit is created.

### 9. Publish multi-arch GHCR images for both deployables

Each release publishes:

- `ghcr.io/ahstn/oceans-llm-gateway`
- `ghcr.io/ahstn/oceans-llm-admin-ui`

for:
- `linux/amd64`
- `linux/arm64`

with tags:
- full release tag, such as `v0.1.0`,
- floating `X.Y`,
- `sha-<shortsha>`,
- `latest` only for stable releases.

Why:
- GHCR is a natural fit for a GitHub-hosted repo,
- multi-arch images keep deployment options open,
- floating tags help operations without replacing immutable version tags.

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
- release automation stays close to native GitHub capabilities.

Tradeoffs:
- maintainers still need to trigger releases manually,
- PR titles now carry more operational weight,
- the release workflow now writes a changelog commit directly to `main`,
- Cargo package versions are not the public release identity for now,
- branch governance must be kept aligned with the workflow assumptions.

## Follow-up Work

- Apply and maintain GitHub repository settings so `main` stays squash-only and PR-gated.
- Ensure GitHub Actions can continue to write the changelog commit to `main` under branch protection.
- Revisit Cargo-level release automation only if the repo starts publishing crates externally.

## Attribution

This ADR was prepared through collaborative human + AI implementation/design work.
