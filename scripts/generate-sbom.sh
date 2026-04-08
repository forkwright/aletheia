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

# Check if cargo-cyclonedx is installed
if ! command -v cargo-cyclonedx &>/dev/null; then
    echo "cargo-cyclonedx not found. Installing..."
    cargo install cargo-cyclonedx --version ^0.5 --locked
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
