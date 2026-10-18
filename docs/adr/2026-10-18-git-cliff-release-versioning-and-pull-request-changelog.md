# ADR: git-cliff Owns Release Versioning and a Pull-Request Changelog

- Date: 2026-10-18
- Status: Accepted
- Supersedes: Cocogitto version and tag ownership in [the release versioning ADR](2026-03-06-release-versioning-and-ghcr-publishing.md) and [the cargo-dist ADR](2026-10-05-cli-binary-distribution-with-cargo-dist.md).

## Current state

- [Release Process](../contributing/reference/release-process.md)

## Context

Releases used two Conventional Commit tools. Cocogitto calculated the next version, ran hooks for cargo-release and git-cliff, then created the version commit and tag. git-cliff wrote `CHANGELOG.md`, and cargo-dist copies that changelog section into the GitHub release.

The changelog listed every commit reachable from the release, including the commits inside each pull request. The template removed repeated subjects, but the 0.40.0 section still has 214 entries for 22 pull requests, and most entries have no pull request link.

Cocogitto added little that git-cliff could not do. Its own changelog was disabled, its monorepo settings had no packages, and nothing ran `cog check`; the `pr-title` workflow validates Conventional Commits. It also read every commit to choose the version. A changelog that lists only pull requests would then disagree with the version: a `chore` pull request containing a `feat` commit would produce a minor release with no new feature in its notes.

## Decision

### 1. List pull requests only

The changelog and GitHub release notes list one entry per merged pull request. The repository allows only merge commits, and GitHub titles each one with the pull request title followed by `(#123)`. The `cliff.toml` parsers match that suffix, and `filter_commits` drops every commit without it. Commits inside a pull request and commits pushed straight to `main` are not listed.

The pull request title sets the entry's group, scope, and breaking marker. Mark a breaking change with `!` in the pull request title. Breaking commits inside a pull request no longer bypass the filter.

Each entry links to its pull request from the number in the subject, so the link does not depend on the GitHub API. The API still supplies the author, preferring the pull request author over the merge commit author.

### 2. Use git-cliff to choose the version

`git-cliff --bumped-version` chooses the next version from the same filtered pull requests as the changelog. `[bump]` keeps the existing rules: breaking changes produce a major release and features a minor release, including before 1.0.

### 3. Replace Cocogitto with a release script

[scripts/release.sh](../../scripts/release.sh) prepares the release. It requires a `main` that matches `origin/main` with no tracked changes other than the vendored snapshots, chooses the version, runs cargo-release, prepends the changelog section, then creates the `chore(version): vX.Y.Z` commit and a lightweight `vX.Y.Z` tag. `mise run release` first refreshes the pricing catalog and the Artificial Analysis benchmark snapshot as task dependencies, so the version commit carries both. It then runs the script and pushes `main` and the tag atomically.

### 4. Prepend new sections

The script prepends each new section with `git-cliff --prepend` and leaves earlier sections as published. Version headings link to their compare view, so a new section needs no link definition at the end of the file. cargo-dist parses linked headings.

## Trade-offs

- Work pushed straight to `main` never appears in release notes and never bumps the version. Changes that should be announced must go through a pull request.
- A release that contains only `chore`, `docs`, or similar pull requests gets a patch bump. Cocogitto refused to bump in that case.
- Pull request titles carry more weight. The `pr-title` workflow already enforces their format.
- Changing the repository's merge commit title setting away from the pull request title would drop every entry. Squash merges with the default title still match.
- A short repository script replaces a maintained tool. It runs only on maintainer machines and has no branching beyond its guards.
- Sections up to 0.40.0 keep the per-commit format.

## Follow-up

- If GitHub stops adding `(#123)` to merge titles, match on the pull request number from the GitHub API instead. The parsers cannot use it today because git-cliff fetches remote data after parsing.
