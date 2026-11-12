#!/usr/bin/env bash
set -euo pipefail

repo_root=$(git rev-parse --show-toplevel)
test_root=$(mktemp -d "${TMPDIR:-/tmp}/oceans-release-provenance.XXXXXX")
trap 'rm -rf "$test_root"' EXIT
checker="$test_root/check-release-provenance.sh"

# Exercise the exact inline command run by the release workflow.
printf '#!/usr/bin/env bash\n' > "$checker"
awk '
  /^  verify-release-provenance:$/ { in_job = 1 }
  in_job && /^        run: \|$/ { in_run = 1; next }
  in_run && $0 == "" { print; next }
  in_run && substr($0, 1, 10) == "          " { print substr($0, 11); next }
  in_run { exit }
' "$repo_root/.github/workflows/release.yml" >> "$checker"
chmod +x "$checker"

run_check() {
  : > "$test_root/github-output"
  (
    cd "$test_root"
    GITHUB_OUTPUT="$test_root/github-output" RELEASE_COMMIT=$1 RELEASE_TAG=$2 "$checker"
  )
}

git -C "$test_root" init --quiet --initial-branch=main
git -C "$test_root" config user.email release-test@example.invalid
git -C "$test_root" config user.name "Release Test"

touch "$test_root/approved"
git -C "$test_root" add approved
git -C "$test_root" commit --quiet --message "approved release"
approved_commit=$(git -C "$test_root" rev-parse HEAD)
git -C "$test_root" tag v1.2.3 "$approved_commit"

touch "$test_root/later"
git -C "$test_root" add later
git -C "$test_root" commit --quiet --message "later main commit"
later_main_commit=$(git -C "$test_root" rev-parse HEAD)
git -C "$test_root" update-ref refs/remotes/origin/main HEAD

git -C "$test_root" switch --quiet --create attacker "$approved_commit"
touch "$test_root/unreviewed"
git -C "$test_root" add unreviewed
git -C "$test_root" commit --quiet --message "unreviewed commit"
unreviewed_commit=$(git -C "$test_root" rev-parse HEAD)
git -C "$test_root" tag v9.9.9 "$unreviewed_commit"

# A tag on an approved main commit remains valid after main advances.
run_check "$approved_commit" v1.2.3
if ! grep --quiet --line-regexp --fixed-strings 'tag=v1.2.3' "$test_root/github-output"; then
  echo "error: approved release tag was not selected" >&2
  exit 1
fi

# A sibling commit with the same approved parent must not gain release authority.
if run_check "$unreviewed_commit" v9.9.9; then
  echo "error: unreviewed commit passed the release provenance check" >&2
  exit 1
fi

# The event commit must be the commit the tag names, as when a tag moves after its push.
if run_check "$later_main_commit" v1.2.3; then
  echo "error: a tag naming a different commit passed the provenance check" >&2
  exit 1
fi

# Only version tags carry release authority.
git -C "$test_root" tag v1.2 "$approved_commit"
for tag in v1.2 main 'v1.2.3;id' missing; do
  if run_check "$approved_commit" "$tag"; then
    echo "error: tag name $tag passed the release provenance check" >&2
    exit 1
  fi
done

# A version tag that does not exist must fail closed.
if run_check "$approved_commit" v7.7.7; then
  echo "error: a missing release tag passed the provenance check" >&2
  exit 1
fi

# Invalid and abbreviated object names must fail closed.
if run_check deadbeef v1.2.3; then
  echo "error: abbreviated object ID passed the release provenance check" >&2
  exit 1
fi

blob_oid=$(git -C "$test_root" hash-object approved)
if run_check "$blob_oid" v1.2.3; then
  echo "error: non-commit object passed the release provenance check" >&2
  exit 1
fi

echo "Release provenance checks passed"
