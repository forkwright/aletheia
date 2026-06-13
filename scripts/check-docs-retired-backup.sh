#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

failures=0

report() {
    printf 'retired-backup-docs: %s\n' "$*" >&2
    failures=$((failures + 1))
}

allowed_compatibility_note() {
    local path="$1"
    local line="$2"

    case "${path}" in
        docs/RUNBOOK.md|docs/DEPLOYMENT.md|docs/DISASTER-RECOVERY.md)
            [[ "${line}" == *removed* || "${line}" == *retired* || "${line}" == *Legacy* ]]
            ;;
        *)
            return 1
            ;;
    esac
}

scan_pattern() {
    local label="$1"
    local pattern="$2"
    local path
    local lineno
    local line

    while IFS=: read -r path lineno line; do
        if allowed_compatibility_note "${path}" "${line}"; then
            continue
        fi
        report "${path}:${lineno}: retired ${label} reference: ${line}"
    done < <(rg --line-number --no-heading --color=never \
        --glob '!scripts/check-docs-retired-backup.sh' \
        -- "${pattern}" docs instance.example shared scripts crates || true)
}

scan_pattern "--export-json flag" '--export-json'
scan_pattern "backup-cron.sh helper" 'backup-cron\.sh'
scan_pattern "aletheia-backup helper" '(^|[^[:alnum:]_.-])aletheia-backup([^[:alnum:]_.-]|$)'
scan_pattern "ergon default path" '(~|\$HOME)/ergon|/ergon/(instance|bin)'

if (( failures > 0 )); then
    exit 1
fi

echo "retired-backup-docs: clean"
