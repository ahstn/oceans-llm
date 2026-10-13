#!/usr/bin/env bash
set -euo pipefail

: "${RELEASE_TARGET:?RELEASE_TARGET is required}"
: "${RELEASE_TAG:?RELEASE_TAG is required}"

extract_dir="$(mktemp -d)"
trap 'rm -rf "$extract_dir"' EXIT

archive="target/distrib/oceans-cli-${RELEASE_TARGET}"
case "$RELEASE_TARGET" in
  *-windows-msvc) 7z x "${archive}.zip" "-o${extract_dir}" >/dev/null; executable=oceans.exe ;;
  *) tar -xzf "${archive}.tar.gz" -C "$extract_dir"; executable=oceans ;;
esac

binary="$(find "$extract_dir" -type f -name "$executable")"
test "$("$binary" --version)" = "oceans ${RELEASE_TAG#v}"
"$binary" --help >/dev/null
