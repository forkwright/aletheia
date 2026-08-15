#!/usr/bin/env bash
set -euo pipefail
# Generate CycloneDX SBOM for the Aletheia Rust workspace.
#
# This script generates a CycloneDX Software Bill of Materials (SBOM) in JSON
# format for all workspace crates. The main crate's SBOM is copied to the
# workspace root as bom.cdx.json.
#
# Prerequisites: cargo (Rust toolchain)
# The script will install cargo-cyclonedx automatically if not present or
# pinned to a different version.
#
# Usage: ./scripts/generate-sbom.sh
#
# WHY this is the SOLE source of the cargo-cyclonedx pin (#4945): release.yml
# shells out to this script rather than repeating the version literal in its
# own "Install cargo-cyclonedx" step — two copies of one floating fact is
# exactly the SSOT gap this pin closes. Bump CARGO_CYCLONEDX_VERSION here and
# both call sites pick it up.
CARGO_CYCLONEDX_VERSION="0.5.9"

installed_version=""
if command -v cargo-cyclonedx &>/dev/null; then
    installed_version=$(cargo-cyclonedx --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1)
fi
if [ "$installed_version" != "$CARGO_CYCLONEDX_VERSION" ]; then
    echo "Installing cargo-cyclonedx ${CARGO_CYCLONEDX_VERSION} (found: ${installed_version:-none})..."
    cargo install cargo-cyclonedx --version "$CARGO_CYCLONEDX_VERSION" --locked
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
