#!/usr/bin/env bash
set -euo pipefail

key_file="${OCEANS_LOCAL_PROVIDER_CREDENTIAL_KEY_FILE:-.local/provider-credential-encryption-key}"

if [[ -L "$key_file" ]]; then
  echo "refusing symbolic link for local provider credential key: $key_file" >&2
  exit 1
fi

if [[ ! -s "$key_file" ]]; then
  mkdir -p "$(dirname "$key_file")"
  umask 077
  temporary_key="$(mktemp "${key_file}.tmp.XXXXXX")"
  trap 'rm -f "$temporary_key"' EXIT
  openssl rand -base64 32 >"$temporary_key"
  ln "$temporary_key" "$key_file" 2>/dev/null || true
  rm -f "$temporary_key"
  trap - EXIT
fi

if [[ ! -s "$key_file" ]]; then
  echo "failed to create local provider credential key: $key_file" >&2
  exit 1
fi
chmod 600 "$key_file"

key="$(tr -d '\r\n' <"$key_file")"
if [[ ${#key} -ne 44 ]]; then
  echo "invalid local provider credential key in $key_file" >&2
  exit 1
fi

printf '%s' "$key"
