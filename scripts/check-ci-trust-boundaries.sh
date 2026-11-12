#!/usr/bin/env bash
set -euo pipefail

# Uses only grep, sed and awk: CI runs this from repo-lint, which installs no ripgrep.
review=.github/workflows/oceans-review-agent.yml
release=.github/workflows/release.yml
release_plan=.github/workflows/release-plan.yml

require_fixed() {
  local file=$1 text=$2
  if ! grep --quiet --fixed-strings -- "$text" "$file"; then
    echo "missing required trust-boundary control in $file: $text" >&2
    exit 1
  fi
}

reject_regex() {
  local file=$1 pattern=$2
  if grep --quiet --extended-regexp -- "$pattern" "$file"; then
    echo "unsafe trust-boundary pattern in $file: $pattern" >&2
    exit 1
  fi
}

# One line per actions/checkout step: its ref, path and persist-credentials inputs.
checkout_steps() {
  awk '
    function flush() {
      if (checkout) print "ref=" ref " path=" path " persist-credentials=" persist
      checkout = 0; ref = ""; path = ""; persist = ""
    }
    /^      - / { flush() }
    /^      -? *uses: actions\/checkout@/ { checkout = 1 }
    /^          ref: / { sub(/^          ref: /, ""); ref = $0 }
    /^          path: / { sub(/^          path: /, ""); path = $0 }
    /^          persist-credentials: / { sub(/^          persist-credentials: /, ""); persist = $0 }
    END { flush() }
  ' "$1"
}

require_fixed "$review" '  pull_request_target:'
require_fixed "$review" '          review-workspace: ${{ github.workspace }}/.review-source'
require_fixed "$review" '        uses: ./actions/review-agent'
reject_regex "$review" '^  pull_request:$'

# The workspace root runs code with credentials, so it must be the base revision. The PR
# head may only appear as data under .review-source. A swapped pair must fail.
# GitHub expression syntax is literal policy text, not a shell expansion.
# shellcheck disable=SC2016
expected_checkouts=$'ref=${{ github.event.pull_request.base.sha }} path= persist-credentials=false\nref=${{ github.event.pull_request.head.sha }} path=.review-source persist-credentials=false'
if [[ "$(checkout_steps "$review")" != "$expected_checkouts" ]]; then
  echo "review checkouts must be the base SHA at the workspace root, then the head SHA at .review-source, both without persisted credentials" >&2
  exit 1
fi

review_data_references=$(grep --fixed-strings '.review-source' "$review" | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//')
# shellcheck disable=SC2016
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
