#!/usr/bin/env bash
set -euo pipefail

: "${RELEASE_TARGET:?RELEASE_TARGET is required}"
: "${RELEASE_TAG:?RELEASE_TAG is required}"

archive="oceans-cli-${RELEASE_TARGET}"
case "$RELEASE_TARGET" in
  *-windows-msvc) extension=zip; executable=oceans.exe ;;
  *) extension=tar.gz; executable=oceans ;;
esac

cd target/distrib
if command -v sha256sum >/dev/null; then
  actual_checksum="$(sha256sum "${archive}.${extension}" | cut -d ' ' -f 1)"
else
  actual_checksum="$(shasum -a 256 "${archive}.${extension}" | cut -d ' ' -f 1)"
fi
expected_checksum="$(head -n 1 "${archive}.${extension}.sha256" | cut -d ' ' -f 1)"
test "$actual_checksum" = "$expected_checksum"

extract_dir="$(mktemp -d)"
trap 'rm -rf "$extract_dir"' EXIT
if [[ "$extension" == zip ]]; then
  7z x "${archive}.${extension}" "-o${extract_dir}" >/dev/null
else
  tar -xzf "${archive}.${extension}" -C "$extract_dir"
fi

binary="$(find "$extract_dir" -type f -name "$executable")"
test -n "$binary"
test "$("$binary" --version)" = "oceans ${RELEASE_TAG#v}"
"$binary" --help >/dev/null
