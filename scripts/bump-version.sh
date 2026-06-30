#!/usr/bin/env bash
set -euo pipefail
# Bump every declared release version owner.
# Fallback for when release-please cannot create the release PR.
#
# Usage: scripts/bump-version.sh 0.11.0

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"

exec python3 "${REPO_ROOT}/scripts/check-release-versioning.py" bump "$@"
