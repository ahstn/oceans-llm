#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SCRIPT_PATH="$ROOT_DIR/scripts/release-analyze.mjs"
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

json_field() {
  local file="$1"
  local field="$2"
  node -e "const data = JSON.parse(require('fs').readFileSync(process.argv[1], 'utf8')); console.log(data[process.argv[2]] ?? '');" "$file" "$field"
}

new_repo() {
  local name="$1"
  local repo_dir="$TMP_ROOT/$name"

  mkdir -p "$repo_dir"
  (
    cd "$repo_dir"
    git init -q -b main
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

repo="$(new_repo initial-minor)"
commit_change "$repo" "feat(gateway): add first release surface"
(
  cd "$repo"
  node "$SCRIPT_PATH" --json > result.json
)
assert_eq "true" "$(json_field "$repo/result.json" hasRelease)" "initial release availability"
assert_eq "0.1.0" "$(json_field "$repo/result.json" version)" "initial release version"

repo="$(new_repo patch-bump)"
(
  cd "$repo"
  git tag v1.2.3
)
commit_change "$repo" "fix(gateway): harden release parser"
(
  cd "$repo"
  node "$SCRIPT_PATH" --json > result.json
)
assert_eq "1.2.4" "$(json_field "$repo/result.json" version)" "patch bump"

repo="$(new_repo breaking-footer)"
(
  cd "$repo"
  git tag v1.2.3
)
commit_change "$repo" "chore(release): restructure output" "BREAKING CHANGE: release notes now include digests"
(
  cd "$repo"
  node "$SCRIPT_PATH" --json > result.json
)
assert_eq "2.0.0" "$(json_field "$repo/result.json" version)" "major bump from breaking footer"

repo="$(new_repo no-release)"
(
  cd "$repo"
  git tag v1.2.3
)
commit_change "$repo" "docs: add release guidance"
(
  cd "$repo"
  node "$SCRIPT_PATH" --json > result.json
)
assert_eq "false" "$(json_field "$repo/result.json" hasRelease)" "non-releasable range"

echo "release analysis tests passed"
