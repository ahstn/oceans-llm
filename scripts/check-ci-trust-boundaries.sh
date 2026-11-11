#!/usr/bin/env bash
set -euo pipefail

review=.github/workflows/oceans-review-agent.yml
release=.github/workflows/release.yml
release_plan=.github/workflows/release-plan.yml

require_fixed() {
  local file=$1 text=$2
  if ! rg --quiet --fixed-strings -- "$text" "$file"; then
    echo "missing required trust-boundary control in $file: $text" >&2
    exit 1
  fi
}

reject_regex() {
  local file=$1 pattern=$2
  if rg --quiet --regexp "$pattern" "$file"; then
    echo "unsafe trust-boundary pattern in $file: $pattern" >&2
    exit 1
  fi
}

require_fixed "$review" '  pull_request_target:'
# GitHub expression syntax is literal policy text, not a shell expansion.
# shellcheck disable=SC2016
require_fixed "$review" '          ref: ${{ github.event.pull_request.base.sha }}'
# shellcheck disable=SC2016
require_fixed "$review" '          ref: ${{ github.event.pull_request.head.sha }}'
require_fixed "$review" '          path: .review-source'
# shellcheck disable=SC2016
require_fixed "$review" '          review-workspace: ${{ github.workspace }}/.review-source'
require_fixed "$review" '        uses: ./actions/review-agent'
reject_regex "$review" '^  pull_request:$'

review_data_references=$(rg --no-line-number --trim --fixed-strings '.review-source' "$review")
expected_review_data_references=$'path: .review-source\nreview-workspace: ${{ github.workspace }}/.review-source'
if [[ "$review_data_references" != "$expected_review_data_references" ]]; then
  echo "PR review data is referenced outside the approved checkout and action input" >&2
  exit 1
fi

reject_regex "$release" '^  pull_request(_target)?:$'

require_fixed "$release_plan" '  pull_request:'
require_fixed "$release_plan" '  contents: read'
require_fixed "$release_plan" '          persist-credentials: false'
reject_regex "$release_plan" '^  pull_request_target:$'
reject_regex "$release_plan" '^[[:space:]]+[A-Za-z-]+:[[:space:]]+write[[:space:]]*$'
reject_regex "$release_plan" '\$\{\{[[:space:]]*secrets\.'

echo "CI trust boundaries are enforced"
