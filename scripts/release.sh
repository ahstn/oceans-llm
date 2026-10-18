#!/usr/bin/env bash
# Prepare a release on main: choose the next version from the pull requests merged
# since the last tag, update Cargo versions and CHANGELOG.md, then commit and tag.
# `mise run release` refreshes the vendored snapshots below, runs this, and pushes
# the result. cliff.toml defines which pull requests count and how they bump the
# version.
set -euo pipefail

branch="$(git branch --show-current)"
if [[ "$branch" != "main" ]]; then
  echo "error: releases are cut from main, not '${branch:-detached HEAD}'" >&2
  exit 1
fi
# The release task's dependencies rewrite these before this script runs.
refreshed=(
  crates/gateway-service/data/pricing_catalog_fallback.json
  crates/gateway-service/data/model_benchmarks.json
)
if [[ -n "$(git status --porcelain --untracked-files=no -- . "${refreshed[@]/#/:(exclude)}")" ]]; then
  echo "error: commit or discard tracked changes before releasing" >&2
  exit 1
fi
# git-cliff reads pull request authors from GitHub starting at HEAD, so HEAD must
# already be on GitHub.
git fetch --quiet origin main
if [[ "$(git rev-parse HEAD)" != "$(git rev-parse FETCH_HEAD)" ]]; then
  echo "error: local main differs from origin/main; sync it before releasing" >&2
  exit 1
fi

tag="$(git-cliff --bumped-version)"
if git rev-parse --quiet --verify "refs/tags/$tag" >/dev/null; then
  echo "error: no pull requests to release since $tag" >&2
  exit 1
fi

cargo release version "${tag#v}" --workspace --execute --no-confirm
git-cliff --unreleased --tag "$tag" --prepend CHANGELOG.md
git commit --all --message "chore(version): $tag"
git tag "$tag"
echo "Prepared $tag"
