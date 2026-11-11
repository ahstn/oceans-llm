# Trusted code boundaries for privileged pull-request automation

- Date: 2026-11-11
- Status: Accepted

## Context

The review workflow needs an OpenRouter credential and permission to publish a GitHub check. The cargo-dist release workflow needs write permission when it publishes a tagged release. Both workflows also ran for pull requests and checked out the pull-request merge tree before running repository scripts or actions.

A same-repository branch is not a trusted code boundary. A collaborator who can open a pull request can change build scripts, local actions, or workflow-adjacent configuration without having the provider credential or release authority directly.

## Decision

- The review workflow uses `pull_request_target`, keeps the same-repository and Dependabot guards, and checks out the immutable base SHA as its executable workspace.
- The pull-request head is checked out at `.review-source` and is passed to the review action only as data. Credentialed jobs must not install dependencies, run tasks, or load local actions from that directory.
- The generated cargo-dist release workflow is tag-only. `pr-run-mode = "skip"` preserves that property when the workflow is regenerated.
- Pull requests validate `dist plan` in `release-plan.yml`, which has `contents: read`, receives no repository secrets, and cannot publish a release.
- `scripts/check-ci-trust-boundaries.sh` enforces these properties in `mise run actions-audit` alongside actionlint and zizmor.

## Why

The jobs still perform the same review and release-plan validation, but contributor-controlled files no longer share a process boundary with reusable credentials or a write-capable repository token. The split also makes future drift fail in repository linting instead of relying on a reviewer to notice a changed checkout ref or permission block.

## Trade-offs

- A review-workflow change in a pull request does not take effect until it reaches the base branch. This is the intended trust boundary.
- `pull_request_target` is unsafe if a later step executes files from `.review-source`. The policy check therefore allows that path only in the data checkout and the review action input.
- Pull-request release planning now has a small workflow separate from cargo-dist's generated publisher. The cargo-dist version appears in both `dist-workspace.toml` and `release-plan.yml` and must stay aligned.
- Cargo-dist still grants `contents: write` at workflow scope for tagged releases. Removing pull-request triggers closes the contributor path; a future cargo-dist version should be adopted if it can generate job-level release permissions without hand-editing `release.yml`.

## Follow-ups

- Revisit the release workflow when cargo-dist supports job-level least privilege for all generated jobs.
- Keep the trust-boundary policy check in the required actions audit when either workflow changes.
