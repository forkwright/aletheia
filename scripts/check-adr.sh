#!/usr/bin/env bash
set -euo pipefail

# Guard public ADRs against private review provenance tokens.

cd "$(git rev-parse --show-toplevel)"

# ADRs moved to kanon/projects/aletheia/decisions/; their PII guard lives with
# that canonical copy.
if [[ ! -d decisions ]]; then
    echo "check-adr: decisions directory absent"
    exit 0
fi

adr_dir=decisions
pattern='operator-review-pending|DIRECTIVE v[0-9]+|\bT0\b|m[e]tis CC|FERRYMAN|greedy-swimming|recon'\''s'

if rg --pcre2 --line-number --color=never -i -e "${pattern}" "${adr_dir}"; then
    echo "check-adr: private provenance token found in ${adr_dir}" >&2
    exit 1
fi

echo "check-adr: clean"
