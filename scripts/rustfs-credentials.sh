#!/usr/bin/env bash
set -euo pipefail

credentials_file=".local/rustfs/credentials.env"

if [[ -L "$credentials_file" ]]; then
  echo "refusing symbolic link for RustFS credentials: $credentials_file" >&2
  exit 1
fi

if [[ -e "$credentials_file" ]]; then
  [[ -s "$credentials_file" ]] || { echo "RustFS credentials file is empty; restore it from backup" >&2; exit 1; }
  chmod 600 "$credentials_file"
  echo "Keeping existing RustFS credentials."
  exit 0
fi

if [[ -d "$OCEANS_RUSTFS_DATA_DIR" ]] && [[ -n "$(find "$OCEANS_RUSTFS_DATA_DIR" -mindepth 1 -maxdepth 1 -print -quit)" ]]; then
  echo "RustFS data exists without its credentials file; restore the credentials from backup" >&2
  exit 1
fi

umask 077
mkdir -p .local/rustfs
temporary_file="$(mktemp "${credentials_file}.tmp.XXXXXX")"
trap 'rm -f "$temporary_file"' EXIT
{
  printf 'RUSTFS_ACCESS_KEY=oceans-%s\n' "$(openssl rand -hex 12)"
  printf 'RUSTFS_SECRET_KEY=%s\n' "$(openssl rand -hex 32)"
} > "$temporary_file"
# A second setup process must not replace credentials created by the first.
ln "$temporary_file" "$credentials_file" 2>/dev/null || [[ -s "$credentials_file" ]]
echo "RustFS credentials are ready in $credentials_file."
