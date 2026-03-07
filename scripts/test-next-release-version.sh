#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SCRIPT_PATH="$ROOT_DIR/scripts/next-release-version.sh"
TMP_ROOT="$(mktemp -d)"

cleanup() {
  rm -rf "$TMP_ROOT"
}

trap cleanup EXIT INT TERM

assert_eq() {
  local expected="$1"
  local actual="$2"
  local label="$3"

  if [[ "$expected" != "$actual" ]]; then
    echo "assertion failed for $label: expected '$expected', got '$actual'" >&2
    exit 1
  fi
}

assert_fails() {
  local label="$1"
  shift

  if "$@" >/dev/null 2>&1; then
    echo "expected failure for $label" >&2
    exit 1
  fi
}

new_repo() {
  local name="$1"
  local repo_dir="$TMP_ROOT/$name"

  mkdir -p "$repo_dir"
  (
    cd "$repo_dir"
    git init -q
    git config user.name "Codex"
    git config user.email "codex@example.com"
    printf 'seed\n' > README.md
    git add README.md
    git commit -q -m 'chore: bootstrap repo'
  )

  printf '%s\n' "$repo_dir"
}

commit_change() {
  local repo_dir="$1"
  local title="$2"
  local body="${3:-}"

  (
    cd "$repo_dir"
    printf '%s\n' "$title" >> history.log
    git add history.log
    if [[ -n "$body" ]]; then
      git commit -q -m "$title" -m "$body"
    else
      git commit -q -m "$title"
    fi
  )
}

repo="$(new_repo explicit-version)"
explicit_version="$(
  cd "$repo"
  RELEASE_AS=0.1.0 "$SCRIPT_PATH"
)"
assert_eq "v0.1.0" "$explicit_version" "explicit version normalization"

repo="$(new_repo patch-bump)"
(
  cd "$repo"
  git tag v1.2.3
)
commit_change "$repo" "fix(gateway): harden release parser"
patch_version="$(
  cd "$repo"
  "$SCRIPT_PATH"
)"
assert_eq "v1.2.4" "$patch_version" "patch bump"

repo="$(new_repo minor-bump)"
(
  cd "$repo"
  git tag v1.2.3
)
commit_change "$repo" "feat(admin-ui): add release dashboard"
minor_version="$(
  cd "$repo"
  "$SCRIPT_PATH"
)"
assert_eq "v1.3.0" "$minor_version" "minor bump"

repo="$(new_repo major-bump)"
(
  cd "$repo"
  git tag v1.2.3
)
commit_change "$repo" "refactor(gateway)!: change release contract"
major_version="$(
  cd "$repo"
  "$SCRIPT_PATH"
)"
assert_eq "v2.0.0" "$major_version" "major bump from !"

repo="$(new_repo breaking-footer)"
(
  cd "$repo"
  git tag v1.2.3
)
commit_change "$repo" "chore(release): restructure output" "BREAKING CHANGE: release notes now include digests"
footer_major_version="$(
  cd "$repo"
  "$SCRIPT_PATH"
)"
assert_eq "v2.0.0" "$footer_major_version" "major bump from breaking footer"

repo="$(new_repo no-releasable-commits)"
(
  cd "$repo"
  git tag v1.2.3
)
commit_change "$repo" "docs: add release guidance"
assert_fails "non-releasable commit range" bash -lc "cd '$repo' && '$SCRIPT_PATH'"

repo="$(new_repo missing-tag)"
commit_change "$repo" "fix(gateway): still not enough without a baseline tag"
assert_fails "missing baseline tag" bash -lc "cd '$repo' && '$SCRIPT_PATH'"

echo "release version tests passed"
