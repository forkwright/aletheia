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

for manifest_path in LICENSE LICENSE-DOCS README.md SECURITY.md docs/QUICKSTART.md instance.example/README.md; do
    grep -Eq "^[0-9a-f]{64} [0-7]{4} [0-9]+ ${manifest_path}$" "${manifest}" || {
        echo "release-tarball: manifest missing hash row for ${manifest_path}" >&2
        exit 1
    }
done

echo "release-tarball: clean"
