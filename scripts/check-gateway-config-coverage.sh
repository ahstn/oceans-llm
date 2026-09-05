#!/usr/bin/env bash

set -euo pipefail

coverage_file="$(mktemp)"
trap 'rm -f "$coverage_file"' EXIT

cargo llvm-cov --package gateway --all-targets --lcov --output-path "$coverage_file"

awk '
  /^SF:/ {
    source_file = substr($0, 4)
    in_config = source_file ~ /\/crates\/gateway\/src\/config\.rs$/ ||
      (source_file ~ /\/crates\/gateway\/src\/config\/.*\.rs$/ &&
       source_file !~ /\/crates\/gateway\/src\/config\/tests\//)
  }
  in_config && /^DA:/ {
    split(substr($0, 4), coverage, ",")
    total_lines++
    if (coverage[2] > 0) {
      covered_lines++
    }
  }
  END {
    if (total_lines == 0) {
      print "No gateway configuration coverage data was found." > "/dev/stderr"
      exit 1
    }

    percentage = 100 * covered_lines / total_lines
    printf "Gateway configuration line coverage: %d/%d (%.2f%%)\n", covered_lines, total_lines, percentage
    if (percentage < 90) {
      print "Gateway configuration line coverage must be at least 90%." > "/dev/stderr"
      exit 1
    }
  }
' "$coverage_file"
