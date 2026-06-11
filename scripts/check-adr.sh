#!/usr/bin/env bash
set -euo pipefail

# Guard public ADRs against private review provenance tokens.

cd "$(git rev-parse --show-toplevel)"

pattern='operator-review-pending|DIRECTIVE v[0-9]+|\bT0\b|m[e]tis CC|FERRYMAN|greedy-swimming|recon'\''s'

if rg --pcre2 --line-number --color=never -i -e "${pattern}" decisions/; then
    echo "check-adr: private provenance token found in decisions/" >&2
    exit 1
fi

echo "check-adr: clean"
