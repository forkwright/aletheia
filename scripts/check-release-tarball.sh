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
script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "${script_dir}/.." && pwd)"

# shellcheck source=../.github/tool-versions.sh
. "${repo_root}/.github/tool-versions.sh"

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

grep -Fxq "release_tool_versions_manifest=.github/tool-versions.sh" "${manifest}" || {
    echo "release-tarball: manifest missing tool versions manifest pointer" >&2
    exit 1
}

grep -Eq '^release_tool_versions_sha256=[0-9a-f]{64}$' "${manifest}" || {
    echo "release-tarball: manifest missing tool versions hash" >&2
    exit 1
}

tool_version_lines=(
    "tool_cargo-nextest=${CARGO_NEXTEST_VERSION}"
    "tool_cargo-audit=${CARGO_AUDIT_VERSION}"
    "tool_cargo-fuzz=${CARGO_FUZZ_VERSION}"
    "tool_cross=${CROSS_VERSION}"
    "tool_cargo-cyclonedx=${CARGO_CYCLONEDX_VERSION}"
    "tool_cargo-auditable=${CARGO_AUDITABLE_VERSION}"
    "tool_uv=${UV_VERSION}"
)

for tool_version_line in "${tool_version_lines[@]}"; do
    grep -Fxq "${tool_version_line}" "${manifest}" || {
        echo "release-tarball: manifest missing ${tool_version_line}" >&2
        exit 1
    }
done

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
