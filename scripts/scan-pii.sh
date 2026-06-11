#!/usr/bin/env bash
set -euo pipefail
# Scan the working tree for PII / secret patterns defined in
# .github/pii-patterns.txt. Designed to run locally (`scripts/scan-pii.sh`)
# and in CI. Emits plain-text diagnostics and exits non-zero on any
# unsuppressed match.
#
# Override mechanisms:
#   * PII_ALLOWLIST_PATHS  - newline-separated regexes of paths to skip
#   * pii-allow: <reason>  - trailing marker on the same source line
#                             (after any comment leader) to suppress one
#                             match. The reason is not parsed but is
#                             required by convention.
#
# Candidate credit-card matches are post-filtered through Luhn. All other
# patterns are reported as-is.
#
# Shell standards: bash 5.x, set -euo pipefail, shellcheck-clean.

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
PATTERNS_FILE="${REPO_ROOT}/.github/pii-patterns.txt"

if [[ ! -f "${PATTERNS_FILE}" ]]; then
    echo "scan-pii: patterns file not found at ${PATTERNS_FILE}" >&2
    exit 2
fi

if ! command -v rg >/dev/null 2>&1; then
    echo "scan-pii: ripgrep (rg) is required" >&2
    exit 2
fi

# Paths ignored by default. Extend via PII_ALLOWLIST_PATHS (one regex/line).
# WHY: tracked test fixtures and documentation may legitimately reference
# example values that overlap with PII shapes. The allowlist mirrors
# .gitleaks.toml where appropriate so the two scanners agree.
DEFAULT_ALLOWLIST=(
    '^\.git/'
    '^target/'
    '^node_modules/'
    '^vendor/'
    '^\.github/pii-patterns\.txt$'
    '^scripts/scan-pii\.sh$'
    '^tests/pii-scanner/'
    '^docs/specs/'
    '^docs/CHANGELOG\.md$'
    '^docs/CONFIGURATION\.md$'
    '^docs/QUICKSTART\.md$'
    '^docs/RUNBOOK\.md$'
    '^docs/CUTOVER_CHECKLIST\.md$'
    '^\.gitleaks\.toml$'
    # WHY: standards are documentation with illustrative examples.
    '^standards/'
    # WHY: intentionally holds fake credentials used by redaction tests.
    'crates/[^/]+/src/redact\.rs$'
    # WHY: PII-redaction implementation contains literal fixtures whose
    # whole purpose is to exercise the redactor on realistic shapes.
    '^crates/nous/src/training/pii\.rs$'
    # WHY: multilingual FTS stopword fixtures legitimately include common
    # non-English words that overlap with private fleet hostnames.
    '^crates/krites/src/fts/tokenizer/stop_word_filter/stopwords/'
    # WHY: operator-specific template; mirrors .gitleaks.toml allowlist.
    '^shared/'
    '^instance\.example/'
    '^infrastructure/runtime/'
    '^infrastructure/prosoche/'
)

declare -a ALLOWLIST
ALLOWLIST=("${DEFAULT_ALLOWLIST[@]}")
if [[ -n "${PII_ALLOWLIST_PATHS:-}" ]]; then
    while IFS= read -r line; do
        [[ -z "${line}" ]] && continue
        ALLOWLIST+=("${line}")
    done <<< "${PII_ALLOWLIST_PATHS}"
fi

path_allowed() {
    local path="$1"
    local pattern
    for pattern in "${ALLOWLIST[@]}"; do
        if [[ "${path}" =~ ${pattern} ]]; then
            return 0
        fi
    done
    return 1
}

# Luhn check for credit-card candidates. Accepts digits + separators.
luhn_ok() {
    local raw="$1"
    local digits="${raw//[^0-9]/}"
    local len=${#digits}
    if (( len < 13 || len > 19 )); then
        return 1
    fi
    local sum=0 parity=$((len % 2)) i d
    for (( i=0; i<len; i++ )); do
        d="${digits:$i:1}"
        if (( i % 2 == parity )); then
            d=$((d * 2))
            (( d > 9 )) && d=$((d - 9))
        fi
        sum=$((sum + d))
    done
    (( sum % 10 == 0 ))
}

load_patterns() {
    # Emit non-comment, non-blank lines from PATTERNS_FILE.
    awk '/^[[:space:]]*#/ {next} /^[[:space:]]*$/ {next} {print}' "${PATTERNS_FILE}"
}

# Credit-card pattern is recognised structurally so we can gate it on Luhn.
CC_PATTERN_MARKER='4[0-9]{3}|5[1-5][0-9]{2}|3[47][0-9]{2}|6(?:011|5[0-9]{2})'

findings=0

cd "${REPO_ROOT}"

while IFS= read -r pattern; do
    [[ -z "${pattern}" ]] && continue

    is_cc=0
    if [[ "${pattern}" == *"${CC_PATTERN_MARKER}"* ]]; then
        is_cc=1
    fi

    # rg output format: path:line:col:match. We honour .gitignore (so
    # instance/ and target/ are skipped) and enable PCRE2 for lookaround.
    while IFS= read -r hit; do
        [[ -z "${hit}" ]] && continue
        path="${hit%%:*}"
        # Strip the leading `./` that rg emits for relative walks so the
        # allowlist regexes can anchor at `^<dir>/`.
        path="${path#./}"
        rest="${hit#*:}"
        lineno="${rest%%:*}"
        rest="${rest#*:}"
        # Drop column; remainder is the match text.
        rest="${rest#*:}"
        match="${rest}"

        if path_allowed "${path}"; then
            continue
        fi

        # Per-line override: a trailing `pii-allow: <reason>` marker
        # following any comment leader (`#`, `//`, `--`, `;`) suppresses
        # this match.
        line_content="$(awk -v ln="${lineno}" 'NR==ln' "${path}" 2>/dev/null || true)"
        if [[ "${line_content}" == *"pii-allow:"* ]]; then
            continue
        fi

        if (( is_cc == 1 )); then
            if ! luhn_ok "${match}"; then
                continue
            fi
        fi

        printf 'PII: %s:%s: match=%q pattern=%q\n' \
            "${path}" "${lineno}" "${match}" "${pattern}" >&2
        findings=$((findings + 1))
    done < <(rg --pcre2 --no-heading --line-number --column \
        --color=never --with-filename \
        --glob '!.git' --glob '!target' --glob '!node_modules' \
        -e "${pattern}" . 2>/dev/null || true)
done < <(load_patterns)

if (( findings > 0 )); then
    printf '\nscan-pii: %d unsuppressed finding(s)\n' "${findings}" >&2
    exit 1
fi

echo "scan-pii: clean"
