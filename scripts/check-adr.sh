#!/usr/bin/env bash
set -euo pipefail

# Guard public ADRs against private review provenance tokens.

cd "$(git rev-parse --show-toplevel)"

# WHY(#6523): the guard below is `if rg ...; then`, which reads a missing rg the
# same way it reads "no match" — as clean. A provenance-token check that cannot
# distinguish "found nothing" from "could not look" is worse than no check,
# because it is trusted.
if ! command -v rg >/dev/null 2>&1; then
    echo "check-adr: ripgrep (rg) is required" >&2
    exit 2
fi

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
