#!/usr/bin/env bash
set -euo pipefail

key_file="${OCEANS_LOCAL_PROVIDER_CREDENTIAL_KEY_FILE:-.local/provider-credential-encryption-key}"

if [[ ! -s "$key_file" ]]; then
  mkdir -p "$(dirname "$key_file")"
  umask 077
  openssl rand -base64 32 >"$key_file"
fi
chmod 600 "$key_file"

key="$(tr -d '\r\n' <"$key_file")"
if [[ ${#key} -ne 44 ]]; then
  echo "invalid local provider credential key in $key_file" >&2
  exit 1
fi

printf '%s' "$key"
