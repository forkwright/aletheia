#!/usr/bin/env bash
set -euo pipefail
# Generate GitHub Actions workflow YAML from templates.
#
# Usage: scripts/generate-workflows.sh [--shards N] [--dry-run]
#
# Generated files are written to .github/workflows/ and contain a header
# indicating they must not be edited by hand. Edit this script instead.
#
# Targets generated:
#   .github/workflows/ci.yml           (shellcheck, commitlint, standards-sync, verify-generated)
#   .github/workflows/test-sharded.yml (smart-filtered PR runs + full sharded main runs)

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

# Build the shard index list (1-based) as a JSON array for the matrix.
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

# ── Template: ci.yml ──────────────────────────────────────────────────────────
#
# WHY shellcheck: shell linting catches errors before they reach CI runners.
# WHY commitlint: enforces conventional commit format required by release-please.
# WHY standards-sync: prevents local standards/ from drifting from kanon canonical.
# WHY verify-generated: prevents hand-edits to generated workflow files.
generate_ci() {
    cat <<'YAML'
# !! GENERATED FILE — do not edit by hand.
# Edit scripts/generate-workflows.sh and re-run to update.

name: CI

on:
  push:
    branches: [main]
    paths:
      - "scripts/**"
      - ".github/workflows/ci.yml"
  pull_request:
    branches: [main]

concurrency:
  group: ci-${{ github.ref }}
  cancel-in-progress: true

# WHY: Principle of least privilege for CI workflows
permissions:
  contents: read

jobs:
  shellcheck:
    runs-on: ubuntu-latest
    if: >-
      github.event_name == 'pull_request' ||
      contains(join(github.event.commits.*.modified, ','), 'scripts/')
    steps:
      - uses: actions/checkout@de0fac2e4500dabe0009e67214ff5f5447ce83dd # v6
      - name: Lint shell scripts
        run: |
          failed=0
          while IFS= read -r script; do
            if head -1 "$script" | grep -qE '(bash|sh)'; then
              echo "::group::$script"
              shellcheck -S warning "$script" || failed=1
              echo "::endgroup::"
            fi
          done < <(find scripts -type f -executable 2>/dev/null)
          exit "$failed"

  commitlint:
    if: github.event_name == 'pull_request'
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@de0fac2e4500dabe0009e67214ff5f5447ce83dd # v6
        with:
          fetch-depth: 0
      - name: Validate conventional commits
        run: |
          TYPES="feat|fix|refactor|chore|docs|test|ci|perf|style|build|revert"
          failed=0
          while IFS= read -r msg; do
            echo "$msg" | grep -qE "^Merge " && continue
            if ! echo "$msg" | grep -qE "^($TYPES)(\(.+\))?: .+"; then
              echo "::error::Bad commit message: $msg"
              echo "  Expected: <type>(<scope>): <description>"
              echo "  Types: $TYPES"
              failed=1
            fi
          done < <(git log --format='%s' origin/main..HEAD)
          exit "$failed"

  standards-sync:
    if: github.event_name == 'pull_request'
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@de0fac2e4500dabe0009e67214ff5f5447ce83dd # v6

      - name: Fetch and diff canonical standards from kanon
        env:
          GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
        run: |
          tmpdir="$(mktemp -d)"
          gh api repos/forkwright/kanon/contents/standards \
            --jq '.[].name' | while IFS= read -r fname; do
            gh api "repos/forkwright/kanon/contents/standards/${fname}" \
              --jq '.content' \
              | base64 -d > "${tmpdir}/${fname}"
          done
          failed=0
          while IFS= read -r fname; do
            local_file="standards/${fname}"
            canonical="${tmpdir}/${fname}"
            if [[ ! -f "$local_file" ]]; then
              echo "::error file=${local_file}::Canonical file '${fname}' from kanon is missing locally"
              failed=1
            elif ! diff -q -- "$canonical" "$local_file" > /dev/null 2>&1; then
              echo "::error file=${local_file}::Local '${fname}' diverges from kanon canonical"
              diff -- "$canonical" "$local_file" || true  # NOTE: show the diff output; non-zero exit expected
              failed=1
            fi
          done < <(ls "${tmpdir}/")
          if [[ "$failed" -ne 0 ]]; then
            echo ""
            echo "Standards diverged from kanon. Update local standards/ to match, or sync kanon."
            echo "Aletheia-specific additions (files only in standards/ but not in kanon) are allowed."
            exit 1
          fi
          echo "standards/ is in sync with kanon"

  # WHY: Ensures the checked-in workflow YAML always matches what the generator
  # would produce. Hand-edits are caught before merge; re-running the script is
  # the fix. Runs only on PRs so noise on main pushes is avoided.
  verify-generated:
    name: Verify generated workflows
    if: github.event_name == 'pull_request'
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@de0fac2e4500dabe0009e67214ff5f5447ce83dd # v6
      - name: Re-generate workflows
        run: scripts/generate-workflows.sh
      - name: Check for drift
        run: |
          if ! git diff --exit-code .github/workflows/; then
            echo "::error::Generated workflows are stale. Run scripts/generate-workflows.sh and commit the result."
            exit 1
          fi
          echo "Generated workflows are up to date."
YAML
}

# ── Template: test-sharded.yml ────────────────────────────────────────────────
#
# WHY nextest: cargo-nextest exposes --partition hash:M/N which assigns each
# test to a shard by stable hash of its fully-qualified name — no overlap, no
# gaps, deterministic across runs for the same test binary.
#
# WHY smart filtering: on PRs, testing only affected crates (changed + rdeps)
# cuts wall-clock time proportional to the unchanged fraction of the workspace.
# The full suite still runs on every push to main to catch cross-crate regressions
# that a PR's changed-set analysis might miss.
#
# WHY test-plan job: separates the "what to test" decision from the actual test
# execution so that both test-shard and test-filtered can branch on it.
generate_test_sharded() {
    sed "s/@@SHARDS@@/${SHARDS}/g; s/@@SHARD_LIST@@/${SHARD_LIST}/g" <<'YAML'
# !! GENERATED FILE — do not edit by hand.
# Edit scripts/generate-workflows.sh and re-run to update.
# Generated with: scripts/generate-workflows.sh --shards @@SHARDS@@

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
  group: test-sharded-${{ github.ref }}
  cancel-in-progress: true

# WHY: Principle of least privilege for CI workflows
permissions:
  contents: read

env:
  CARGO_TERM_COLOR: always
  SHARD_COUNT: @@SHARDS@@

jobs:
  # WHY: Determine whether to run the full sharded suite (pushes to main,
  # workflow_dispatch) or a filtered subset (pull requests). On PRs, uses
  # cargo metadata to walk the dependency graph and find all crates that
  # transitively depend on the changed files, so only those are tested.
  test-plan:
    name: Compute test plan
    runs-on: ubuntu-latest
    outputs:
      full_suite: ${{ steps.plan.outputs.full_suite }}
      packages: ${{ steps.plan.outputs.packages }}
    steps:
      - uses: actions/checkout@de0fac2e4500dabe0009e67214ff5f5447ce83dd # v6
        with:
          fetch-depth: 0

      - uses: dtolnay/rust-toolchain@631a55b12751854ce901bb631d5902ceb48146f7 # stable

      - uses: Swatinem/rust-cache@e18b497796c12c097a38f9edb9d0641fb99eee32 # v2
        with:
          # WHY: Shared cache key so the plan job benefits from the same
          # sccache/target warming as the test jobs.
          key: test-plan

      - name: Compute test plan
        id: plan
        env:
          EVENT: ${{ github.event_name }}
          BASE_REF: ${{ github.base_ref }}
        run: |
          if [[ "$EVENT" != "pull_request" ]]; then
            printf 'Event is %s; running full suite.\n' "$EVENT"
            echo "full_suite=true" >> "$GITHUB_OUTPUT"
            echo "packages=" >> "$GITHUB_OUTPUT"
            exit 0
          fi

          # PR: compute changed files relative to the merge base
          git fetch origin "$BASE_REF" --depth=1
          changed=$(git diff --name-only "origin/${BASE_REF}...HEAD" || true)

          if [[ -z "$changed" ]]; then
            printf 'No changed files detected; skipping tests.\n'
            echo "full_suite=false" >> "$GITHUB_OUTPUT"
            echo "packages=" >> "$GITHUB_OUTPUT"
            exit 0
          fi

          pkgs=$(echo "$changed" | python3 scripts/affected-crates.py | sort -u | tr '\n' ' ' | sed 's/[[:space:]]*$//')
          printf 'Affected packages: %s\n' "$pkgs"
          echo "full_suite=false" >> "$GITHUB_OUTPUT"
          echo "packages=${pkgs}" >> "$GITHUB_OUTPUT"

  # WHY: Split the workspace test suite across parallel runners using
  # nextest's hash-based partitioning. Each shard receives a stable,
  # non-overlapping subset of tests keyed by fully-qualified test name.
  # This cuts wall-clock time by ~1/N without duplication or gaps.
  # Only runs on main-branch pushes and workflow_dispatch (full suite).
  test-shard:
    name: "Test shard ${{ matrix.shard }}/@@SHARDS@@"
    needs: [test-plan]
    if: needs.test-plan.outputs.full_suite == 'true'
    runs-on: ubuntu-latest
    strategy:
      fail-fast: false
      matrix:
        shard: @@SHARD_LIST@@
    steps:
      - uses: actions/checkout@de0fac2e4500dabe0009e67214ff5f5447ce83dd # v6

      - uses: dtolnay/rust-toolchain@631a55b12751854ce901bb631d5902ceb48146f7 # stable

      - uses: Swatinem/rust-cache@e18b497796c12c097a38f9edb9d0641fb99eee32 # v2
        with:
          # WHY: Shard-specific cache key so each runner has its own cache
          # slot and doesn't thrash on concurrent writes.
          key: shard-${{ matrix.shard }}

      - name: Install nextest
        uses: taiki-e/install-action@e24b8b7a939c6a537188f34a4163cb153dd85cf6 # v2
        with:
          tool: nextest

      - name: "Test (shard ${{ matrix.shard }}/@@SHARDS@@)"
        env:
          SHARD: ${{ matrix.shard }}
        run: |
          cargo nextest run \
            --workspace \
            --partition "hash:${SHARD}/@@SHARDS@@"

  # WHY: On pull requests, only test the crates that were changed plus their
  # reverse dependencies. This avoids rebuilding and retesting unaffected
  # crates, reducing PR feedback time proportionally to the unchanged fraction.
  test-filtered:
    name: Test (affected crates)
    needs: [test-plan]
    if: needs.test-plan.outputs.full_suite == 'false'
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@de0fac2e4500dabe0009e67214ff5f5447ce83dd # v6

      - uses: dtolnay/rust-toolchain@631a55b12751854ce901bb631d5902ceb48146f7 # stable

      - uses: Swatinem/rust-cache@e18b497796c12c097a38f9edb9d0641fb99eee32 # v2
        with:
          key: filtered

      - name: Install nextest
        uses: taiki-e/install-action@e24b8b7a939c6a537188f34a4163cb153dd85cf6 # v2
        with:
          tool: nextest

      - name: Test (affected crates)
        env:
          PACKAGES: ${{ needs.test-plan.outputs.packages }}
        run: |
          if [[ -z "$PACKAGES" ]]; then
            printf 'No affected Rust crates detected; skipping tests.\n'
            exit 0
          fi
          pkg_flags=$(echo "$PACKAGES" | tr ' ' '\n' | grep -v '^$' | xargs -I{} printf -- '--package %s ' {})
          cargo nextest run $pkg_flags

  # WHY: Gate merges on a single required status check regardless of whether
  # the full sharded suite or the filtered subset ran. Simpler to configure in
  # branch protection rules than listing individual shard jobs.
  test-sharded-pass:
    name: Test shards passed
    needs: [test-plan, test-shard, test-filtered]
    runs-on: ubuntu-latest
    if: always()
    steps:
      - name: Check test results
        env:
          PLAN_RESULT: ${{ needs.test-plan.result }}
          FULL_SUITE: ${{ needs.test-plan.outputs.full_suite }}
          SHARD_RESULT: ${{ needs.test-shard.result }}
          FILTERED_RESULT: ${{ needs.test-filtered.result }}
        run: |
          if [[ "$PLAN_RESULT" != "success" ]]; then
            printf 'test-plan failed (result: %s)\n' "$PLAN_RESULT" >&2
            exit 1
          fi
          if [[ "$FULL_SUITE" == "true" ]]; then
            if [[ "$SHARD_RESULT" != "success" ]]; then
              printf 'One or more test shards failed (result: %s)\n' "$SHARD_RESULT" >&2
              exit 1
            fi
          else
            if [[ "$FILTERED_RESULT" != "success" ]]; then
              printf 'Filtered test job failed (result: %s)\n' "$FILTERED_RESULT" >&2
              exit 1
            fi
          fi
          printf 'All tests passed.\n'
YAML
}

# ── Main ──────────────────────────────────────────────────────────────────────
write_file "${WORKFLOWS_DIR}/ci.yml" "$(generate_ci)"
write_file "${WORKFLOWS_DIR}/test-sharded.yml" "$(generate_test_sharded)"

if [[ "$DRY_RUN" == false ]]; then
    printf 'Done. Commit .github/workflows/ together with this script.\n'
fi
