#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 3 ]]; then
    echo "usage: scripts/verify-sha256.sh <file> <checksum-file> <expected-name>" >&2
    exit 2
fi

subject="$1"
checksum="$2"
expected_name="$3"

[[ -f "$subject" ]] || { echo "sha256: missing file: $subject" >&2; exit 1; }
[[ -f "$checksum" ]] || { echo "sha256: missing checksum: $checksum" >&2; exit 1; }
[[ "$expected_name" != */* && -n "$expected_name" ]] || {
    echo "sha256: expected name must be one basename" >&2
    exit 1
}

record_count="$(awk 'END { print NR + 0 }' "$checksum")"
if [[ "$record_count" -ne 1 ]]; then
    echo "sha256: checksum file must contain exactly one record" >&2
    exit 1
fi
checksum_line="$(awk 'NR == 1 { print; exit }' "$checksum")"
if [[ ! "$checksum_line" =~ ^([0-9a-f]{64})[[:space:]]+\*?([^[:space:]]+)$ ]]; then
    echo "sha256: malformed checksum record" >&2
    exit 1
fi
expected_digest="${BASH_REMATCH[1]}"
recorded_name="${BASH_REMATCH[2]}"
if [[ "$recorded_name" != "$expected_name" ]]; then
    echo "sha256: checksum names ${recorded_name}, expected ${expected_name}" >&2
    exit 1
fi

if command -v sha256sum >/dev/null 2>&1; then
    actual_digest="$(sha256sum -- "$subject" | awk '{print $1}')"
elif command -v shasum >/dev/null 2>&1; then
    actual_digest="$(shasum -a 256 -- "$subject" | awk '{print $1}')"
else
    echo "sha256: neither sha256sum nor shasum is available" >&2
    exit 1
fi

if [[ "$actual_digest" != "$expected_digest" ]]; then
    echo "sha256: digest mismatch for ${expected_name}" >&2
    exit 1
fi
echo "sha256: verified ${expected_name}"
