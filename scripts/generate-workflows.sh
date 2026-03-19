#!/usr/bin/env bash
# Generate GitHub Actions workflow YAML from templates.
#
# Usage: scripts/generate-workflows.sh [--shards N] [--dry-run]
#
# Generated files are written to .github/workflows/ and contain a header
# indicating they must not be edited by hand. Edit this script instead.
#
# Targets generated:
#   .github/workflows/test-sharded.yml  (--shards N controls parallelism)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
WORKFLOWS_DIR="${REPO_ROOT}/.github/workflows"

# Defaults
SHARDS=4
DRY_RUN=false

# ── Argument parsing ──────────────────────────────────────────────────────────
while [[ $# -gt 0 ]]; do
    case "$1" in
        --shards)
            SHARDS="${2:?'--shards requires a value'}"
            shift 2
            ;;
        --dry-run)
            DRY_RUN=true
            shift
            ;;
        -h|--help)
            grep '^#' "$0" | cut -c3-
            exit 0
            ;;
        *)
            printf 'Unknown argument: %s\n' "$1" >&2
            exit 1
            ;;
    esac
done

if ! [[ "$SHARDS" =~ ^[2-9]$|^[1-9][0-9]+$ ]]; then
    printf 'error: --shards must be an integer >= 2 (got %s)\n' "$SHARDS" >&2
    exit 1
fi

# ── Helpers ───────────────────────────────────────────────────────────────────
write_file() {
    local path="$1"
    local content="$2"
    if [[ "$DRY_RUN" == true ]]; then
        printf '[dry-run] would write %s\n' "$path"
        return
    fi
    printf '%s\n' "$content" > "$path"
    printf 'wrote %s\n' "$path"
}

# Build the shard index list (0-based) as a JSON array for the matrix.
# e.g. SHARDS=4 → [1,2,3,4]
build_shard_list() {
    local n="$1"
    local list="["
    local i
    for (( i = 1; i <= n; i++ )); do
        list+="$i"
        [[ $i -lt $n ]] && list+=","
    done
    list+="]"
    printf '%s' "$list"
}

SHARD_LIST="$(build_shard_list "$SHARDS")"

# ── Template: test-sharded.yml ────────────────────────────────────────────────
#
# WHY nextest: cargo-nextest exposes --partition hash:M/N which assigns each
# test to a shard by stable hash of its fully-qualified name — no overlap, no
# gaps, deterministic across runs for the same test binary.
generate_test_sharded() {
    cat <<YAML
# !! GENERATED FILE — do not edit by hand.
# Edit scripts/generate-workflows.sh and re-run to update.
# Generated with: scripts/generate-workflows.sh --shards ${SHARDS}

name: Test (sharded)

on:
  push:
    branches: [main]
    paths:
      - "crates/**"
      - "Cargo.toml"
      - "Cargo.lock"
      - ".github/workflows/test-sharded.yml"
  pull_request:
    branches: [main]
    paths:
      - "crates/**"
      - "Cargo.toml"
      - "Cargo.lock"
      - ".github/workflows/test-sharded.yml"
  workflow_dispatch:

concurrency:
  group: test-sharded-\${{ github.ref }}
  cancel-in-progress: true

# WHY: Principle of least privilege for CI workflows
permissions:
  contents: read

env:
  CARGO_TERM_COLOR: always
  SHARD_COUNT: ${SHARDS}

jobs:
  # WHY: Split the workspace test suite across parallel runners using
  # nextest's hash-based partitioning. Each shard receives a stable,
  # non-overlapping subset of tests keyed by fully-qualified test name.
  # This cuts wall-clock time by ~1/N without duplication or gaps.
  test-shard:
    name: "Test shard \${{ matrix.shard }}/${SHARDS}"
    runs-on: ubuntu-latest
    strategy:
      fail-fast: false
      matrix:
        shard: ${SHARD_LIST}
    steps:
      - uses: actions/checkout@de0fac2e4500dabe0009e67214ff5f5447ce83dd # v6

      - uses: dtolnay/rust-toolchain@631a55b12751854ce901bb631d5902ceb48146f7 # stable

      - uses: Swatinem/rust-cache@e18b497796c12c097a38f9edb9d0641fb99eee32 # v2
        with:
          # WHY: Shard-specific cache key so each runner has its own cache
          # slot and doesn't thrash on concurrent writes.
          key: shard-\${{ matrix.shard }}

      - name: Install cargo-nextest
        uses: taiki-e/install-action@64b66df73c70298e1ad1a8d1ef49e06da2db64c8 # nextest

      - name: "Test (shard \${{ matrix.shard }}/${SHARDS})"
        run: |
          cargo nextest run \\
            --workspace \\
            --partition hash:\${{ matrix.shard }}/${SHARDS}

  # WHY: Gate merges on all shards passing. A single required status check
  # per "fan-in" job is simpler than listing N individual shard jobs in
  # branch protection rules and handles dynamic shard counts transparently.
  test-sharded-pass:
    name: Test shards passed
    needs: [test-shard]
    runs-on: ubuntu-latest
    if: always()
    steps:
      - name: Check all shards succeeded
        run: |
          result="\${{ needs.test-shard.result }}"
          if [[ "\$result" != "success" ]]; then
            printf 'One or more test shards failed (result: %s)\\n' "\$result" >&2
            exit 1
          fi
YAML
}

# ── Main ──────────────────────────────────────────────────────────────────────
write_file "${WORKFLOWS_DIR}/test-sharded.yml" "$(generate_test_sharded)"

if [[ "$DRY_RUN" == false ]]; then
    printf 'Done. Commit .github/workflows/test-sharded.yml together with this script.\n'
fi
