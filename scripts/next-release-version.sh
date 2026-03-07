#!/usr/bin/env bash
set -euo pipefail

release_as="${RELEASE_AS:-${1:-}}"

normalize_version() {
  local raw="$1"

  if [[ ! "$raw" =~ ^v?[0-9]+\.[0-9]+\.[0-9]+([-.][0-9A-Za-z.-]+)?$ ]]; then
    echo "invalid release version: $raw" >&2
    exit 1
  fi

  if [[ "$raw" != v* ]]; then
    raw="v$raw"
  fi

  printf '%s\n' "$raw"
}

if [[ -n "$release_as" ]]; then
  normalize_version "$release_as"
  exit 0
fi

last_tag="$(git describe --tags --match 'v[0-9]*' --abbrev=0 2>/dev/null || true)"
if [[ -z "$last_tag" ]]; then
  echo "no existing v* tag found; set RELEASE_AS or pass an explicit version" >&2
  exit 1
fi

if [[ ! "$last_tag" =~ ^v([0-9]+)\.([0-9]+)\.([0-9]+)$ ]]; then
  echo "latest tag is not a stable semver tag: $last_tag" >&2
  exit 1
fi

major="${BASH_REMATCH[1]}"
minor="${BASH_REMATCH[2]}"
patch="${BASH_REMATCH[3]}"

bump="none"
range="${last_tag}..HEAD"

while IFS= read -r -d '' commit; do
  [[ -z "$commit" ]] && continue

  IFS=$'\n' read -r subject _ <<<"$commit"

  if printf '%s\n' "$commit" | grep -Eq '^BREAKING[ -]CHANGE:'; then
    bump="major"
    break
  fi

  if [[ "$subject" =~ ^[a-z]+(\([[:alnum:]_.\/-]+\))?!:\  ]]; then
    bump="major"
    break
  fi

  if [[ "$bump" == "none" && "$subject" =~ ^feat(\([[:alnum:]_.\/-]+\))?:\  ]]; then
    bump="minor"
    continue
  fi

  if [[ "$bump" == "none" && "$subject" =~ ^fix(\([[:alnum:]_.\/-]+\))?:\  ]]; then
    bump="patch"
  fi
done < <(git log -z --format='%s%n%b%x00' "$range")

case "$bump" in
  major)
    major=$((major + 1))
    minor=0
    patch=0
    ;;
  minor)
    minor=$((minor + 1))
    patch=0
    ;;
  patch)
    patch=$((patch + 1))
    ;;
  none)
    echo "no releasable changes found since $last_tag; use RELEASE_AS to override" >&2
    exit 1
    ;;
esac

printf 'v%s.%s.%s\n' "$major" "$minor" "$patch"
