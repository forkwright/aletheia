#!/usr/bin/env bash
set -euo pipefail
# Scan the working tree for PII / secret patterns defined in
# .github/pii-patterns.txt. Designed to run locally (`scripts/scan-pii.sh`)
# and in CI. Emits plain-text diagnostics and exits non-zero on any
# unsuppressed match.
#
# Override mechanisms:
#   * PII_ALLOWLIST_PATHS    - newline-separated regexes of paths to skip
#   * PII_PATTERNS_EXTRA_FILE - path to an additional patterns file (same
#                             one-regex-per-line format), loaded after
#                             .github/pii-patterns.txt. Lets a maintainer
#                             layer their own private literal hostnames/paths
#                             from a file outside this repo without adding
#                             them to the public, shared pattern list.
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
EXTRA_PATTERNS_FILE="${PII_PATTERNS_EXTRA_FILE:-}"

if [[ ! -f "${PATTERNS_FILE}" ]]; then
    echo "scan-pii: patterns file not found at ${PATTERNS_FILE}" >&2
    exit 2
fi

if [[ -n "${EXTRA_PATTERNS_FILE}" && ! -f "${EXTRA_PATTERNS_FILE}" ]]; then
    echo "scan-pii: PII_PATTERNS_EXTRA_FILE set but not found at ${EXTRA_PATTERNS_FILE}" >&2
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
    # WHY: release-please generates CHANGELOG.md at the repo root from merged PR
    # titles; historical entries reflect already-public commit subjects and cannot
    # be rewritten without re-running release-please. The root path is canonical.
    '^CHANGELOG\.md$'
    '^docs/CONFIGURATION\.md$'
    '^docs/QUICKSTART\.md$'
    '^docs/RUNBOOK\.md$'
    '^docs/CUTOVER_CHECKLIST\.md$'
    '^\.gitleaks\.toml$'
    # WHY: intentionally holds fake credentials used by redaction tests.
    'crates/[^/]+/src/redact\.rs$'
    # WHY: PII-redaction implementation contains literal fixtures whose
    # whole purpose is to exercise the redactor on realistic shapes.
    '^crates/nous/src/training/pii\.rs$'
    # WHY: multilingual FTS stopword fixtures legitimately include common
    # non-English words that overlap with private fleet hostnames.
    '^crates/krites/src/fts/tokenizer/stop_word_filter/stopwords/'
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
    # Emit non-comment, non-blank lines from PATTERNS_FILE, then from
    # EXTRA_PATTERNS_FILE if one was supplied.
    awk '/^[[:space:]]*#/ {next} /^[[:space:]]*$/ {next} {print}' "${PATTERNS_FILE}"
    if [[ -n "${EXTRA_PATTERNS_FILE}" ]]; then
        awk '/^[[:space:]]*#/ {next} /^[[:space:]]*$/ {next} {print}' "${EXTRA_PATTERNS_FILE}"
    fi
}

# Credit-card pattern is recognised structurally so we can gate it on Luhn.
CC_PATTERN_MARKER='4[0-9]{3}|5[1-5][0-9]{2}|3[47][0-9]{2}|6(?:011|5[0-9]{2})'

# WHY(#5439): an invalid PCRE2 pattern must fail the scanner loudly, not be
# swallowed as "zero matches" for that pattern while CI stays green.
# Compiling against empty input is cheap (no repo walk) and exercises the
# same PCRE2 parser rg uses on the real scan; exit 1 there just means "no
# match against nothing" — a genuinely invalid pattern exits 2.
validate_pattern() {
    local pattern="$1" err rc
    err="$(printf '' | rg --pcre2 -e "${pattern}" 2>&1 1>/dev/null)" && rc=0 || rc=$?
    if (( rc != 0 && rc != 1 )); then
        printf 'scan-pii: invalid PCRE2 pattern %q\n' "${pattern}" >&2
        [[ -n "${err}" ]] && printf '  %s\n' "${err}" >&2
        return 1
    fi
    return 0
}

declare -a PATTERNS
pattern_errors=0
while IFS= read -r pattern; do
    [[ -z "${pattern}" ]] && continue
    PATTERNS+=("${pattern}")
    validate_pattern "${pattern}" || pattern_errors=$((pattern_errors + 1))
done < <(load_patterns)

if (( pattern_errors > 0 )); then
    printf '\nscan-pii: %d invalid pattern(s) in %s, refusing to scan\n' \
        "${pattern_errors}" "${PATTERNS_FILE}" >&2
    exit 2
fi

findings=0
scan_errors=0
rg_out="$(mktemp)"
rg_err="$(mktemp)"
trap 'rm -f "${rg_out}" "${rg_err}"' EXIT

cd "${REPO_ROOT}"

for pattern in "${PATTERNS[@]}"; do
    is_cc=0
    if [[ "${pattern}" == *"${CC_PATTERN_MARKER}"* ]]; then
        is_cc=1
    fi

    # rg output format: path:line:col:match. We honour .gitignore (so
    # instance/ and target/ are skipped) and enable PCRE2 for lookaround.
    # Exit 1 ("no matches") is the expected outcome for most patterns; any
    # other exit is a scanner failure, not a clean pattern (#5439) — it must
    # not be swallowed the way the pre-fix `2>/dev/null || true` did.
    if rg --pcre2 --no-heading --line-number --column \
        --color=never --with-filename \
        --glob '!.git' --glob '!target' --glob '!node_modules' \
        -e "${pattern}" . >"${rg_out}" 2>"${rg_err}"; then
        rg_exit=0
    else
        rg_exit=$?
    fi

    if (( rg_exit != 0 && rg_exit != 1 )); then
        printf 'scan-pii: ripgrep failed on pattern %q (exit %d)\n' \
            "${pattern}" "${rg_exit}" >&2
        [[ -s "${rg_err}" ]] && cat "${rg_err}" >&2
        scan_errors=$((scan_errors + 1))
        continue
    fi

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
    done < "${rg_out}"
done

if (( scan_errors > 0 )); then
    printf '\nscan-pii: %d pattern(s) failed to run, refusing to report clean\n' \
        "${scan_errors}" >&2
    exit 2
fi

if (( findings > 0 )); then
    printf '\nscan-pii: %d unsuppressed finding(s)\n' "${findings}" >&2
    exit 1
fi

echo "scan-pii: clean"
