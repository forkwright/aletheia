#!/usr/bin/env bash
set -euo pipefail
# Fetch IBM Plex Mono and Cormorant Garamond font files (OFL licensed).
#
# Usage: ./scripts/fetch-fonts.sh
# Run from the desktop crate root (crates/theatron/desktop/).

FONT_DIR="assets/fonts"
WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT

mkdir -p "$FONT_DIR"

echo "Fetching IBM Plex Mono..."
PLEX_VERSION="v6.4.1"
PLEX_URL="https://github.com/IBM/plex/releases/download/%40ibm%2Fplex-mono%40${PLEX_VERSION}/ibm-plex-mono.zip"
curl -sSL "$PLEX_URL" -o "$WORK_DIR/plex-mono.zip"
unzip -qo "$WORK_DIR/plex-mono.zip" -d "$WORK_DIR/plex-mono"
find "$WORK_DIR/plex-mono" -name "*.woff2" -exec cp {} "$FONT_DIR/" \;

echo "Fetching Cormorant Garamond..."
CORM_VERSION="v4.0.0"
CORM_URL="https://github.com/CatharsisFonts/Cormorant/releases/download/${CORM_VERSION}/Cormorant_Install_${CORM_VERSION}.zip"
curl -sSL "$CORM_URL" -o "$WORK_DIR/cormorant.zip"
unzip -qo "$WORK_DIR/cormorant.zip" -d "$WORK_DIR/cormorant"
find "$WORK_DIR/cormorant" -name "*.woff2" -exec cp {} "$FONT_DIR/" \;

echo "Fonts installed to $FONT_DIR:"
ls -1 "$FONT_DIR"/*.woff2 2>/dev/null || echo "  (no .woff2 files found — check URLs)"
