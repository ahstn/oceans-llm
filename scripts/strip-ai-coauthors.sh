#!/usr/bin/env bash
set -euo pipefail

# commit-msg hook: drop Co-Authored-By trailers that coding agents add.
# Git AI records agent attribution, so these trailers only add noise.
# Human co-authors are kept.
msg_file="${1:?usage: strip-ai-coauthors.sh <commit-msg-file>}"

agent_pattern='^co-authored-by:[[:space:]]*(claude|openai codex|codex|copilot|cursor agent|devin)([^[:alnum:]]|$)|^co-authored-by:.*(noreply@anthropic\.com|codex@openai\.com|cursoragent@cursor\.com|\[bot\]@users\.noreply\.github\.com)'

if grep -qiE "$agent_pattern" "$msg_file"; then
  tmp="$(mktemp)"
  grep -viE "$agent_pattern" "$msg_file" >"$tmp" || true
  # Trim the blank lines left where the trailer block used to be.
  awk '{ lines[NR] = $0 } NF { last = NR } END { for (i = 1; i <= last; i++) print lines[i] }' "$tmp" >"$msg_file"
  rm -f "$tmp"
fi
