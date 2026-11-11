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
    GITHUB_OUTPUT="$test_root/github-output" RELEASE_COMMIT=$1 "$checker"
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

# A tagged release from an approved main commit remains valid after main advances.
run_check "$approved_commit"
if ! rg --quiet --fixed-strings 'tag=v1.2.3' "$test_root/github-output"; then
  echo "error: approved release tag was not selected" >&2
  exit 1
fi

# An ordinary main push without a release tag remains a successful no-op.
run_check "$later_main_commit"
if ! rg --quiet --fixed-strings 'tag=' "$test_root/github-output"; then
  echo "error: ordinary main push did not produce an empty release tag" >&2
  exit 1
fi

# A sibling commit with the same approved parent must not gain release authority.
if run_check "$unreviewed_commit"; then
  echo "error: unreviewed commit passed the release provenance check" >&2
  exit 1
fi

# Ambiguous release authority on one commit must fail closed.
git -C "$test_root" tag v1.2.4 "$approved_commit"
if run_check "$approved_commit"; then
  echo "error: multiple release tags on one commit passed the provenance check" >&2
  exit 1
fi

# Invalid and abbreviated object names must fail closed.
if run_check deadbeef; then
  echo "error: abbreviated object ID passed the release provenance check" >&2
  exit 1
fi

blob_oid=$(git -C "$test_root" hash-object approved)
if run_check "$blob_oid"; then
  echo "error: non-commit object passed the release provenance check" >&2
  exit 1
fi

echo "Release provenance checks passed"
