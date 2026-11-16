#!/usr/bin/env bash
# Workflow policy strings intentionally contain unevaluated GitHub and shell variables.
# shellcheck disable=SC2016
set -euo pipefail

# Uses only grep, sed and awk: CI runs this from repo-lint, which installs no ripgrep.
# shellcheck source=scripts/workflow-checkouts.sh
source "$(dirname "${BASH_SOURCE[0]}")/workflow-checkouts.sh"

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

# The npm workflow may stage reviewed source, but never approve or publish it directly.
opencode=.github/workflows/opencode-oceans.yml
published=.github/workflows/opencode-oceans-published.yml
require_fixed "$opencode" '  pull_request:'
require_fixed "$opencode" '  workflow_dispatch:'
require_fixed "$opencode" "    if: github.event_name == 'workflow_dispatch' && github.ref == 'refs/heads/main' && github.repository == 'ahstn/oceans-llm'"
require_fixed "$opencode" '          git merge-base --is-ancestor HEAD origin/main'
require_fixed "$opencode" '          test "$(git rev-parse HEAD)" = "$DISPATCH_COMMIT"'
require_fixed "$opencode" '          fetch-depth: 0'
require_fixed "$opencode" '          persist-credentials: false'
require_fixed "$opencode" '    needs: check'
require_fixed "$opencode" '    environment: npm-opencode'
require_fixed "$opencode" '                pkg.name !== "@ahstn/opencode-oceans" ||'
require_fixed "$opencode" '                pkg.publishConfig?.registry !== "https://registry.npmjs.org" ||'
require_fixed "$opencode" '                pkg.publishConfig?.access !== "public") {'
require_fixed "$opencode" '                process.env.RELEASE_TAG !== "opencode-oceans-v" + pkg.version) {'
reject_regex "$opencode" 'pull_request_target|secrets\.|packages:[[:space:]]*write|permissions:[[:space:]]*write-all'
reject_regex "$opencode" '(bun|npm)[[:space:]]+publish|npm[[:space:]]+stage[[:space:]]+(approve|reject)|npm[[:space:]]+dist-tag'

# Only the stage job may request an OIDC token. A PR's check job stays read-only.
stage_job=$(awk '/^  stage:$/ { in_job = 1 } in_job { print }' "$opencode")
write_permissions=$(grep --extended-regexp '^[[:space:]]+[A-Za-z-]+:[[:space:]]+write[[:space:]]*$' "$opencode" || true)
stage_permissions=$(awk '/^    permissions:/ { block = 1; next } block && /^      / { print; next } { block = 0 }' <<< "$stage_job")
if [[ "$write_permissions" != '      id-token: write' ||
      "$stage_permissions" != $'      contents: read\n      id-token: write' ]]; then
  echo "only the npm stage job may receive id-token: write" >&2
  exit 1
fi

# Transfer the checked archive and its independently recorded checksum; never rebuild in stage.
require_fixed "$opencode" '          name: opencode-oceans-source'
# GitHub expressions are literal workflow policy, not shell expansions.
# shellcheck disable=SC2016
require_fixed "$opencode" '      sha256: ${{ steps.package.outputs.sha256 }}'
# shellcheck disable=SC2016
require_fixed "$opencode" '          PACKAGE_SHA256: ${{ needs.check.outputs.sha256 }}'
require_fixed "$opencode" '          printf '\''%s  %s\n'\'' "$PACKAGE_SHA256" "dist/$PACKAGE_FILE" | sha256sum --check --strict'
require_fixed "$opencode" "          mise exec -- npm stage publish \"./dist/\$PACKAGE_FILE\" \\"
require_fixed "$opencode" '            --registry=https://registry.npmjs.org --access public --tag latest --ignore-scripts'
if grep --quiet --extended-regexp 'bun (run pack|pm pack|publish)|npm (pack|publish)|bun install|npm (ci|install)' <<< "$stage_job"; then
  echo "the npm stage job must use the checked archive without rebuilding or installing dependencies" >&2
  exit 1
fi

require_fixed "$published" '  workflow_dispatch:'
require_fixed "$published" "    if: github.ref == 'refs/heads/main' && github.repository == 'ahstn/oceans-llm'"
require_fixed "$published" '          mise run //opencode:smoke-published -- "$PACKAGE_VERSION" | tee npm-smoke.log'
reject_regex "$published" 'secrets\.|^[[:space:]]+[A-Za-z-]+:[[:space:]]+write[[:space:]]*$|permissions:[[:space:]]*write-all'

# Product changelog boundaries must not include independently versioned npm releases.
tag_pattern=$(awk -F '"' '/^tag_pattern = / { print $2 }' cliff.toml)
for tag in v1.2.3 v1.2.3-rc.1 v1.2.3+build.1; do
  if ! grep --quiet --extended-regexp -- "$tag_pattern" <<< "$tag"; then
    echo "product changelog tag pattern excludes $tag" >&2
    exit 1
  fi
done
for tag in opencode-oceans-v0.1.0 prefix-v1.2.3; do
  if grep --quiet --extended-regexp -- "$tag_pattern" <<< "$tag"; then
    echo "product changelog tag pattern accepts unrelated tag $tag" >&2
    exit 1
  fi
done

echo "CI trust boundaries are enforced"
