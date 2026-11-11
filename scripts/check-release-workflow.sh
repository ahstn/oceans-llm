#!/usr/bin/env bash
set -euo pipefail

release=.github/workflows/release.yml
release_cli=.github/workflows/release-cli-check.yml
release_product=.github/workflows/release-product.yml
config=dist-workspace.toml

require_fixed() {
  local file=$1 text=$2
  if ! rg --quiet --fixed-strings -- "$text" "$file"; then
    echo "missing required release control in $file: $text" >&2
    exit 1
  fi
}

require_fixed "$config" 'allow-dirty = ["ci"]'
require_fixed "$release" '  verify-release-provenance:'

trigger=$(sed -n '/^on:$/,/^jobs:$/p' "$release")
if [[ $trigger != *$'  push:\n    branches:\n      - main'* ]]; then
  echo "release publisher must run only from an approved main push" >&2
  exit 1
fi
if [[ $trigger == *'tags:'* ]]; then
  echo "release publisher must not load from an attacker-selected tag ref" >&2
  exit 1
fi

plan_job=$(sed -n '/^  plan:$/,/^  build-local-artifacts:$/p' "$release")
if [[ $plan_job != *$'    needs:\n      - verify-release-provenance'* ]]; then
  echo "release plan must require successful provenance verification" >&2
  exit 1
fi

job_body() {
  local job=$1
  awk -v header="  $job:" '
    $0 == header { in_job = 1; next }
    in_job && /^  [A-Za-z0-9_-]+:$/ { exit }
    in_job { print }
  ' "$release"
}

for job in \
  build-local-artifacts \
  build-global-artifacts \
  custom-release-cli-check \
  host \
  custom-release-product \
  announce; do
  if ! rg --quiet '^      - plan$' <<< "$(job_body "$job")"; then
    echo "release job $job must remain downstream of the verified plan" >&2
    exit 1
  fi
done

provenance_job=$(sed -n '/^  verify-release-provenance:$/,/^  plan:$/p' "$release")
if [[ $provenance_job == *'secrets:'* ]]; then
  echo "release provenance gate must not inherit repository secrets" >&2
  exit 1
fi

# GitHub expression syntax is literal policy text, not a shell expansion.
# shellcheck disable=SC2016
require_fixed "$release" '          ref: ${{ github.sha }}'
require_fixed "$release" '          fetch-depth: 0'
require_fixed "$release" '          persist-credentials: false'
# shellcheck disable=SC2016
require_fixed "$release" '          RELEASE_COMMIT: ${{ github.sha }}'
# shellcheck disable=SC2016
require_fixed "$release" '          release_commit_length=$(printf %s "$RELEASE_COMMIT" | wc -c)'
require_fixed "$release" "          if ! grep -Eq '^[[:xdigit:]]+$' <<< \"\$RELEASE_COMMIT\" \\"
# shellcheck disable=SC2016
require_fixed "$release" '          if ! git merge-base --is-ancestor "$release_oid" "$trusted_oid"; then'
require_fixed "$release" "              | grep -E '^v[0-9]+\\.[0-9]+\\.[0-9]+(-[0-9A-Za-z.-]+)?(\\+[0-9A-Za-z.-]+)?\$' \\"
# shellcheck disable=SC2016
require_fixed "$release" $'    if: ${{ needs.verify-release-provenance.outputs.tag != \'\' }}'
# shellcheck disable=SC2016
require_fixed "$release" '          RELEASE_TAG: ${{ needs.verify-release-provenance.outputs.tag }}'
# shellcheck disable=SC2016
require_fixed "$release" '          dist host --steps=create --tag="$RELEASE_TAG" --output-format=json'

if rg --quiet --regexp '^[[:space:]]+[A-Za-z-]+:[[:space:]]+write[[:space:]]*$' <<< "$provenance_job"; then
  echo "release provenance gate must remain read-only" >&2
  exit 1
fi

for workflow in "$release" "$release_cli" "$release_product"; do
  checkout_count=$(rg --count 'uses: actions/checkout@' "$workflow")
  # GitHub expression syntax is literal policy text, not a shell expansion.
  # shellcheck disable=SC2016
  pinned_ref_count=$(rg --count --fixed-strings 'ref: ${{ github.sha }}' "$workflow")
  if [[ $checkout_count -ne $pinned_ref_count ]]; then
    echo "every checkout in $workflow must use the verified immutable event SHA" >&2
    exit 1
  fi
done

# Releases run on main pushes, so the event ref is `main`, not the release tag.
for workflow in "$release_cli" "$release_product"; do
  if rg --quiet --regexp 'github\.ref_name|GITHUB_REF_NAME|github\.ref\b|GITHUB_REF\b' "$workflow"; then
    echo "$workflow must take the release version from the verified plan, not the event ref" >&2
    exit 1
  fi
done

echo "Release workflow provenance gate is enforced"
