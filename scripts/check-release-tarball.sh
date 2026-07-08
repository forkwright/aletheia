#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 3 ]]; then
    echo "usage: scripts/check-release-tarball.sh <tarball> <version> <target>" >&2
    exit 2
fi

tarball="$1"
version="$2"
target="$3"
root="aletheia-${version}"

if [[ ! -f "${tarball}" ]]; then
    echo "release-tarball: missing tarball ${tarball}" >&2
    exit 1
fi

contents="$(tar -tzf "${tarball}")"

require_path() {
    local path="$1"
    if ! grep -Fxq "${path}" <<< "${contents}"; then
        echo "release-tarball: missing ${path}" >&2
        exit 1
    fi
}

required_paths=(
    "${root}/aletheia"
    "${root}/LICENSE"
    "${root}/LICENSE-DOCS"
    "${root}/README.md"
    "${root}/SECURITY.md"
    "${root}/CHANGELOG.md"
    "${root}/Cargo.toml"
    "${root}/Cargo.lock"
    "${root}/deny.toml"
    "${root}/docs/QUICKSTART.md"
    "${root}/docs/DEPLOYMENT.md"
    "${root}/docs/RELEASING.md"
    "${root}/docs/DISASTER-RECOVERY.md"
    "${root}/instance.example/README.md"
    "${root}/PACKAGE-MANIFEST.txt"
)

for path in "${required_paths[@]}"; do
    require_path "${path}"
done

tmpdir="$(mktemp -d)"
trap 'rm -rf "${tmpdir}"' EXIT

tar -xzf "${tarball}" -C "${tmpdir}" "${root}/PACKAGE-MANIFEST.txt"
manifest="${tmpdir}/${root}/PACKAGE-MANIFEST.txt"

grep -Fxq "version=${version}" "${manifest}" || {
    echo "release-tarball: manifest version mismatch" >&2
    exit 1
}

grep -Fxq "target=${target}" "${manifest}" || {
    echo "release-tarball: manifest target mismatch" >&2
    exit 1
}

grep -Fxq "features=recall,embed-candle" "${manifest}" || {
    echo "release-tarball: manifest feature set mismatch" >&2
    exit 1
}

grep -Eq '^source_commit=[0-9a-f]{40}$' "${manifest}" || {
    echo "release-tarball: manifest missing source commit" >&2
    exit 1
}

require_manifest_row() {
    local manifest_path="$1"
    awk -v path="${manifest_path}" '
        $1 ~ /^[0-9a-f]{64}$/ && $2 ~ /^[0-7]{4}$/ && $3 ~ /^[0-9]+$/ && $4 == path && NF == 4 {
            found = 1
        }
        END {
            exit found ? 0 : 1
        }
    ' "${manifest}" || {
        echo "release-tarball: manifest missing hash row for ${manifest_path}" >&2
        exit 1
    }
}

while IFS= read -r packaged_path; do
    [[ -z "${packaged_path}" || "${packaged_path}" == */ ]] && continue
    [[ "${packaged_path}" == "${root}/PACKAGE-MANIFEST.txt" ]] && continue

    if [[ "${packaged_path}" != "${root}/"* ]]; then
        echo "release-tarball: unexpected package path ${packaged_path}" >&2
        exit 1
    fi

    require_manifest_row "${packaged_path#${root}/}"
done <<< "${contents}"

while IFS= read -r manifest_path; do
    [[ -z "${manifest_path}" ]] && continue
    require_path "${root}/${manifest_path}"
done < <(
    awk '
        $1 ~ /^[0-9a-f]{64}$/ && $2 ~ /^[0-7]{4}$/ && $3 ~ /^[0-9]+$/ && NF == 4 {
            print $4
        }
    ' "${manifest}"
)

echo "release-tarball: clean"
