# Trusted code boundaries for privileged pull-request automation

- Date: 2026-11-11
- Status: Accepted

## Context

The review workflow needs an OpenRouter credential and permission to publish a GitHub check. The cargo-dist release workflow needs write permission when it publishes a tagged release. Both workflows also ran for pull requests and checked out the pull-request merge tree before running repository scripts or actions.

A same-repository branch is not a trusted code boundary. A collaborator who can open a pull request can change build scripts, local actions, or workflow-adjacent configuration without having the provider credential or release authority directly.

## Decision

- The review workflow uses `pull_request_target`, keeps the same-repository and Dependabot guards, and checks out the immutable base SHA as its executable workspace.
- The pull-request head is checked out at `.review-source` and is passed to the review action only as data. Credentialed jobs must not install dependencies, run tasks, or load local actions from that directory.
- The generated cargo-dist release workflow runs only on a `vX.Y.Z` tag push. `pr-run-mode = "skip"` keeps pull-request triggers out when the workflow is regenerated. The release does not run for each merge to `main`.
- Before any publish job, the read-only `verify-release-provenance` job checks that the pushed tag is a version tag, that it names the event commit, and that this commit is reachable from `main`. A tag on an unmerged branch commit therefore cannot publish. Downstream jobs take the version from that job's output, not from the event ref.
- Pull requests validate `dist plan` in `release-plan.yml`, which has `contents: read`, receives no repository secrets, and cannot publish a release.
- `scripts/check-ci-trust-boundaries.sh` enforces these properties in `mise run actions-audit` alongside actionlint and zizmor. It requires the base SHA checkout at the workspace root and the head SHA checkout at `.review-source`, both without persisted credentials. `scripts/check-release-workflow.sh` requires the exact release trigger set and the provenance checks in `mise run dist-check`. Both scripts use only `grep`, `sed` and `awk`, because CI runners do not install ripgrep.

## Why

The jobs still perform the same review and release-plan validation, but contributor-controlled files no longer share a process boundary with reusable credentials or a write-capable repository token. The split also makes future drift fail in repository linting instead of relying on a reviewer to notice a changed checkout ref or permission block.

## Trade-offs

- A review-workflow change in a pull request does not take effect until it reaches the base branch. This is the intended trust boundary.
- `pull_request_target` is unsafe if a later step executes files from `.review-source`. The policy check therefore allows that path only in the data checkout and the review action input.
- Pull-request release planning now has a small workflow separate from cargo-dist's generated publisher. The cargo-dist version appears in both `dist-workspace.toml` and `release-plan.yml` and must stay aligned.
- Cargo-dist still grants `contents: write` at workflow scope for tagged releases. Removing pull-request triggers closes the contributor path; a future cargo-dist version should be adopted if it can generate job-level release permissions without hand-editing `release.yml`.
- A tag push loads `release.yml` from the tagged commit. The provenance job runs from that same file, so it stops mistakes, such as a tag on an unmerged commit, but it is not a boundary against a collaborator who can push tags. That collaborator can already push branches that run their own workflows. The repository has no tag ruleset, so a version tag can also move or be deleted after its run starts.

## Follow-ups

- Revisit the release workflow when cargo-dist supports job-level least privilege for all generated jobs.
- Keep the trust-boundary policy check in the required actions audit when either workflow changes.
- Add a repository tag ruleset for `refs/tags/v*` that limits creation to maintainers and blocks updates and deletion. This closes the moved-tag window, which workflow code cannot close.
