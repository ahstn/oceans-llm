#!/usr/bin/env bash
set -euo pipefail

repo_root=$(git rev-parse --show-toplevel)
test_root=$(mktemp -d "${TMPDIR:-/tmp}/oceans-release-test.XXXXXX")
trap 'rm -rf "$test_root"' EXIT
mkdir -p "$test_root/bin" "$test_root/repo"

# Use a real repository and remote so the production release guards still run.
git -C "$test_root/repo" init --quiet --initial-branch=main
git -C "$test_root/repo" config user.email release-test@example.invalid
git -C "$test_root/repo" config user.name "Release Test"
printf '[package]\nname = "release-test"\nversion = "1.0.0"\n' > "$test_root/repo/Cargo.toml"
printf '# Changelog\n\nPublished notes.\n' > "$test_root/repo/CHANGELOG.md"
git -C "$test_root/repo" add Cargo.toml CHANGELOG.md
tree=$(git -C "$test_root/repo" write-tree)
commit=$(printf 'feat: release test (#1)\n' | git -C "$test_root/repo" commit-tree "$tree")
git -C "$test_root/repo" update-ref refs/heads/main "$commit"
git -C "$test_root/repo" remote add origin "$test_root/repo"
cp "$test_root/repo/CHANGELOG.md" "$test_root/original-changelog"
cp "$test_root/repo/Cargo.toml" "$test_root/original-cargo"

# git-cliff is the external API boundary: fail after a partial output write, as a
# failed generator must not damage the published notes or bump package versions.
cat > "$test_root/bin/git-cliff" <<'SCRIPT'
#!/usr/bin/env bash
set -euo pipefail
for arg in "$@"; do
  if [[ "$arg" == --bumped-version ]]; then
    echo v1.1.0
    exit 0
  fi
done
while [[ $# -gt 0 ]]; do
  if [[ "$1" == --prepend ]]; then
    printf 'incomplete changelog\n' > "$2"
    break
  fi
  shift
done
echo 'GitHub metadata: HTTP 504' >&2
exit 1
SCRIPT
cat > "$test_root/bin/cargo" <<'SCRIPT'
#!/usr/bin/env bash
printf '[package]\nname = "release-test"\nversion = "1.1.0"\n' > Cargo.toml
SCRIPT
chmod +x "$test_root/bin/git-cliff" "$test_root/bin/cargo"

if (cd "$test_root/repo" && PATH="$test_root/bin:$PATH" bash "$repo_root/scripts/release.sh"); then
  echo 'error: release succeeded despite a changelog failure' >&2
  exit 1
fi
cmp "$test_root/original-cargo" "$test_root/repo/Cargo.toml"
cmp "$test_root/original-changelog" "$test_root/repo/CHANGELOG.md"
if [[ $(git -C "$test_root/repo" rev-parse HEAD) != "$commit" ]] ||
  [[ -n $(git -C "$test_root/repo" tag --list) ]]; then
  echo 'error: changelog failure created a release commit or tag' >&2
  exit 1
fi

echo 'Release API failure leaves Cargo versions, changelog, commits, and tags unchanged'
