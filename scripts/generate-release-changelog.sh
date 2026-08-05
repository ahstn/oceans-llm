#!/usr/bin/env bash

set -euo pipefail

readonly version_tag="${1:?usage: generate-release-changelog.sh <version-tag>}"

if git rev-parse --verify --quiet "refs/tags/${version_tag}" >/dev/null; then
  echo "tag already exists: ${version_tag}" >&2
  exit 1
fi

cleanup() {
  git tag --delete "$version_tag" >/dev/null 2>&1 || true
}
trap cleanup EXIT

git tag "$version_tag" HEAD
git-cliff -o CHANGELOG.md
