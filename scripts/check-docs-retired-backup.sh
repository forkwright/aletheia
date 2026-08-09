#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

# WHY(#6523): scan_pattern discards rg's exit status, because rg exits 1 on "no
# matches" and that is the passing case. Without this preflight a missing rg is
# indistinguishable from a clean tree: the loop reads no lines, failures stays
# 0, and this script reports clean having scanned nothing. That is what it did
# on every PR while the runner had no ripgrep installed.
if ! command -v rg >/dev/null 2>&1; then
    printf 'retired-backup-docs: ripgrep (rg) is required\n' >&2
    exit 2
fi

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
        -- "${pattern}" docs instance.example shared scripts crates _llm || true)
}

scan_pattern "--export-json flag" '--export-json'
scan_pattern "backup-cron.sh helper" 'backup-cron\.sh'
scan_pattern "aletheia-backup helper" '(^|[^[:alnum:]_.-])aletheia-backup([^[:alnum:]_.-]|$)'
scan_pattern "ergon default path" '(~|\$HOME)/ergon|/ergon/(instance|bin)'
# WHY(#5107): the top-level `backup --list`/`backup --prune` flags let
# invalid combinations parse and silently dispatch list before prune.
# They are retired in favor of the `list`/`prune` subcommands.
scan_pattern "backup --list legacy flag" 'backup --list'
scan_pattern "backup --prune legacy flag" 'backup --prune'

if (( failures > 0 )); then
    exit 1
fi

echo "retired-backup-docs: clean"
