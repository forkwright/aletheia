#!/usr/bin/env bash
set -euo pipefail

# Reject fixed paths under the shared temp root in shared tooling, hook
# examples, and the runbook.
#
# WHY: a fixed name under /tmp or $TMPDIR belongs to whichever UNIX account
# creates it first. Every other account on the host then fails to write there,
# and anything persisted is world-readable and open to symlink attack. Hook
# examples are copied into production configs, so an unsafe example ships the
# defect to every reader. scripts/health-monitor.sh already carries the rule
# this enforces; forkwright/aletheia#5332 is the finding.
#
# WARNING: this check reads text, so it cannot see a path assembled at runtime
# from parts, nor one reached through a variable defined in another file. It
# catches the literal shape that has actually appeared here.
#
# `mktemp` is the correct remedy for genuinely ephemeral scratch — it yields a
# unique name per run and so excludes nobody — and is deliberately not flagged.

REPO_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

# WHY(#6523): the scan below discards rg's exit status, because rg exits 1 on
# "no matches" and that is the passing case. Without this preflight a missing rg
# is indistinguishable from a clean tree: the loop reads no lines, failures
# stays 0, and this script reports clean having scanned nothing. That is what it
# did on every PR while the runner had no ripgrep installed.
if ! command -v rg >/dev/null 2>&1; then
    printf 'shared-temp-path: ripgrep (rg) is required\n' >&2
    exit 2
fi

SCAN_PATHS=(shared docs/RUNBOOK.md)

failures=0

report() {
    printf 'shared-temp-path: %s\n' "$*" >&2
    failures=$((failures + 1))
}

# A line is a finding when it names a path under the shared temp root and does
# not obtain that path from mktemp on the same line.
while IFS=: read -r path lineno line; do
    case "${line}" in
        *mktemp*) continue ;;
    esac
    report "${path}:${lineno}: fixed path under the shared temp root — use \"\${XDG_STATE_HOME:-\$HOME/.local/state}/aletheia\" for persistent state, \"\${XDG_CACHE_HOME:-\$HOME/.cache}/aletheia\" for caches, or mktemp for ephemeral scratch: ${line}"
done < <(rg --line-number --no-heading --color=never \
    --glob '!scripts/check-shared-temp-paths.sh' \
    -e '/tmp/[A-Za-z0-9._-]' \
    -e '\$\{TMPDIR:-/tmp\}' \
    -e '\$TMPDIR/' \
    -- "${SCAN_PATHS[@]}" || true)

if [[ "${failures}" -gt 0 ]]; then
    printf 'shared-temp-path: %d finding(s)\n' "${failures}" >&2
    exit 1
fi

printf 'shared-temp-path: clean\n'
