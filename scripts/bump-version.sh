#!/usr/bin/env bash
set -euo pipefail
# Bump the workspace version in Cargo.toml and the release-please manifest.
# Fallback for when release-please TOML updater doesn't handle Cargo.toml.
#
# Usage: scripts/bump-version.sh 0.11.0

VERSION="${1:?Usage: $0 <version>}"

if [[ ! "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[a-z0-9.]+)?$ ]]; then
  echo "error: invalid version format: $VERSION" >&2
  exit 1
fi

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"

sed -i "s/^version = \"[0-9]*\.[0-9]*\.[0-9]*\"/version = \"${VERSION}\"/" \
  "${REPO_ROOT}/Cargo.toml"

MANIFEST="${REPO_ROOT}/.release-please-manifest.json"
if [[ -f "$MANIFEST" ]]; then
  sed -i "s/\"\\.: \"[0-9]*\\.[0-9]*\\.[0-9]*\"/\".\": \"${VERSION}\"/" \
    "$MANIFEST"
fi

echo "Bumped workspace version to ${VERSION}"
echo "Verify: cargo metadata --format-version 1 | jq '.packages[] | select(.name | startswith(\"aletheia\")) | .version'"
