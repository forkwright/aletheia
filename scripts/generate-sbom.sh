#!/usr/bin/env bash
set -euo pipefail
# Generate CycloneDX SBOM for the Aletheia Rust workspace.
#
# This script generates a CycloneDX Software Bill of Materials (SBOM) in JSON
# format for all workspace crates. The main crate's SBOM is copied to the
# workspace root as bom.cdx.json.
#
# Prerequisites: cargo (Rust toolchain)
# The script will install cargo-cyclonedx automatically if not present.
#
# Usage: ./scripts/generate-sbom.sh

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "${script_dir}/.." && pwd)"

# shellcheck source=../.github/tool-versions.sh
. "${repo_root}/.github/tool-versions.sh"

cd "${repo_root}"

# Check if cargo-cyclonedx is installed
if ! command -v cargo-cyclonedx &>/dev/null; then
    echo "cargo-cyclonedx not found. Installing ${CARGO_CYCLONEDX_VERSION}..."
    cargo install cargo-cyclonedx --version "${CARGO_CYCLONEDX_VERSION}" --locked
fi

echo "Generating CycloneDX SBOMs for all workspace crates..."
cargo cyclonedx --all --format json

# Copy the main aletheia crate SBOM to the workspace root
cp crates/aletheia/aletheia.cdx.json bom.cdx.json

echo ""
echo "SBOM generated successfully: bom.cdx.json"
echo "Location: $(pwd)/bom.cdx.json"
echo ""
echo "Individual crate SBOMs are available in their respective directories: crates/*/*.cdx.json"
