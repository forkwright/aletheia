#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
verifier="${repo_root}/scripts/verify-sha256.sh"
tmpdir="$(mktemp -d)"
trap 'rm -rf -- "$tmpdir"' EXIT

subject="${tmpdir}/downloaded"
checksum="${tmpdir}/downloaded.sha256"
name="aletheia-linux-x86_64-1.2.3"
printf 'fixture binary\n' > "$subject"
if command -v sha256sum >/dev/null 2>&1; then
    digest="$(sha256sum -- "$subject" | awk '{print $1}')"
else
    digest="$(shasum -a 256 -- "$subject" | awk '{print $1}')"
fi
printf '%s  %s\n' "$digest" "$name" > "$checksum"
"$verifier" "$subject" "$checksum" "$name" >/dev/null

expect_failure() {
    local label="$1"
    shift
    if "$@" >/dev/null 2>&1; then
        echo "FAIL: ${label} unexpectedly passed" >&2
        exit 1
    fi
}

printf '%064d  %s\n' 0 "$name" > "$checksum"
expect_failure "wrong digest" "$verifier" "$subject" "$checksum" "$name"

printf '%s  %s-other\n' "$digest" "$name" > "$checksum"
expect_failure "wrong filename" "$verifier" "$subject" "$checksum" "$name"

printf '%s  %s\n%s  %s\n' "$digest" "$name" "$digest" "$name" > "$checksum"
expect_failure "multiple checksum rows" "$verifier" "$subject" "$checksum" "$name"

echo "OK: release SHA-256 verifier rejects bad evidence"
