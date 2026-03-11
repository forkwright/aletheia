#!/usr/bin/env bash
# smoke-test.sh — Release binary smoke test for the aletheia CLI.
#
# Exercises every subcommand to verify the binary is correctly linked,
# parses arguments, and produces useful output or graceful failures.
# Does NOT require a live server instance.
#
# Usage:
#   ./scripts/smoke-test.sh [--binary PATH] [--build]
#
# Options:
#   --binary PATH   Use an existing binary at PATH (skips build)
#   --build         Force a release build before testing (default if no binary found)
#   --help          Show this message

set -euo pipefail

# ── Colours ──────────────────────────────────────────────────────────────────
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BOLD='\033[1m'
RESET='\033[0m'

# ── State ─────────────────────────────────────────────────────────────────────
PASS=0
FAIL=0
SKIP=0
FAILURES=()

# ── Helpers ───────────────────────────────────────────────────────────────────
pass() { echo -e "  ${GREEN}✓${RESET} $1"; PASS=$(( PASS + 1 )); }
fail() { echo -e "  ${RED}✗${RESET} $1"; FAIL=$(( FAIL + 1 )); FAILURES+=("$1"); }
skip() { echo -e "  ${YELLOW}○${RESET} $1 (skipped)"; SKIP=$(( SKIP + 1 )); }
section() { echo -e "\n${BOLD}$1${RESET}"; }

# Run the binary and check the exit code and optional output pattern.
# Usage: check <description> <expected-exit> [output-pattern] -- <binary-args...>
check() {
    local desc="$1"
    local want_exit="$2"
    local pattern="${3:-}"
    shift 3
    # consume the separator "--"
    if [[ "${1:-}" == "--" ]]; then shift; fi

    local actual_exit=0
    local output
    output=$("$BINARY" "$@" 2>&1) || actual_exit=$?

    if [[ "$actual_exit" -ne "$want_exit" ]]; then
        fail "$desc (exit $actual_exit, expected $want_exit)"
        return
    fi

    if [[ -n "$pattern" ]] && ! echo "$output" | grep -qE "$pattern"; then
        fail "$desc (output missing pattern: $pattern)"
        return
    fi

    pass "$desc"
}

# Runs the binary; passes if exit ≤ 1 (0 = success, 1 = expected failure like
# "no server"). Used for commands that gracefully fail without a live instance.
check_graceful() {
    local desc="$1"
    local pattern="${2:-}"
    shift 2
    if [[ "${1:-}" == "--" ]]; then shift; fi

    local actual_exit=0
    local output
    output=$("$BINARY" "$@" 2>&1) || actual_exit=$?

    if [[ "$actual_exit" -gt 1 ]]; then
        fail "$desc (unexpected exit $actual_exit)"
        return
    fi

    if [[ -n "$pattern" ]] && ! echo "$output" | grep -qiE "$pattern"; then
        fail "$desc (output missing pattern: $pattern)"
        return
    fi

    pass "$desc"
}

# ── Argument parsing ───────────────────────────────────────────────────────────
BINARY=""
FORCE_BUILD=0
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --binary) BINARY="$2"; shift 2 ;;
        --build)  FORCE_BUILD=1; shift ;;
        --help)
            sed -n '2,12p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *) echo "Unknown option: $1" >&2; exit 1 ;;
    esac
done

# ── Build ─────────────────────────────────────────────────────────────────────
DEFAULT_BINARY="$REPO_ROOT/target/release/aletheia"

if [[ -z "$BINARY" ]]; then
    if [[ "$FORCE_BUILD" -eq 1 ]] || [[ ! -x "$DEFAULT_BINARY" ]]; then
        section "Building release binary"
        cargo build --release --manifest-path "$REPO_ROOT/Cargo.toml" --bin aletheia
    fi
    BINARY="$DEFAULT_BINARY"
fi

if [[ ! -x "$BINARY" ]]; then
    echo -e "${RED}Binary not found or not executable: $BINARY${RESET}" >&2
    echo "Run with --build to build it first." >&2
    exit 1
fi

BINARY_VERSION=$("$BINARY" --version 2>&1 | head -1)
echo -e "${BOLD}Aletheia smoke test${RESET}"
echo "Binary : $BINARY"
echo "Version: $BINARY_VERSION"

# ── Tests ─────────────────────────────────────────────────────────────────────

section "Top-level flags"
check "--help exits 0 and shows usage"     0 "[Uu]sage" -- --help
check "--version exits 0 and shows version" 0 "aletheia" -- --version

section "Global help covers all subcommands"
for sub in health backup maintenance tls status credential eval export tui \
           migrate-memory init import seed-skills export-skills review-skills completions; do
    check "--help mentions '$sub'" 0 "$sub" -- --help
done

section "Subcommand --help (all 16)"
check "health --help"         0 "server" -- health --help
check "backup --help"         0 "backup" -- backup --help
check "maintenance --help"    0 "maintenance" -- maintenance --help
check "maintenance status --help"   0 "" -- maintenance status --help
check "maintenance run --help"      0 "" -- maintenance run --help
check "tls --help"            0 "tls\|TLS\|certificate" -- tls --help
check "tls generate --help"   0 "" -- tls generate --help
check "status --help"         0 "status" -- status --help
check "credential --help"     0 "credential\|Credential" -- credential --help
check "credential status --help"  0 "" -- credential status --help
check "credential refresh --help" 0 "" -- credential refresh --help
check "eval --help"           0 "eval\|scenario" -- eval --help
check "export --help"         0 "export\|agent" -- export --help
check "tui --help"            0 "tui\|dashboard" -- tui --help
check "migrate-memory --help" 0 "migrat\|qdrant\|Qdrant" -- migrate-memory --help
check "init --help"           0 "init\|instance" -- init --help
check "import --help"         0 "import\|agent" -- import --help
check "seed-skills --help"    0 "skill" -- seed-skills --help
check "export-skills --help"  0 "skill\|export" -- export-skills --help
check "review-skills --help"  0 "skill\|review" -- review-skills --help
check "completions --help"    0 "completions\|shell" -- completions --help

section "Shell completions (offline)"
check "completions bash exits 0"  0 "" -- completions bash
check "completions bash contains aletheia"  0 "aletheia" -- completions bash
check "completions zsh exits 0"   0 "" -- completions zsh
check "completions fish exits 0"  0 "" -- completions fish

# Verify that the bash completion output contains function/command markers
COMP_OUTPUT=$("$BINARY" completions bash 2>&1)
if echo "$COMP_OUTPUT" | grep -qE "^(function |_aletheia|complete )"; then
    pass "completions bash output looks like valid bash completion"
else
    fail "completions bash output missing expected completion markers"
fi

section "Health (graceful failure without server)"
check_graceful "health gracefully fails without server" \
    "error\|connect\|refused\|unreachable\|failed" -- \
    health --url "http://127.0.0.1:19999"

section "Status (graceful failure without instance)"
check_graceful "status gracefully fails without server" \
    "error\|connect\|refused\|unreachable\|failed\|status" -- \
    status --url "http://127.0.0.1:19999"

section "Init (non-destructive)"
TMPDIR_INIT=$(mktemp -d)
trap 'rm -rf "$TMPDIR_INIT"' EXIT
# init with --yes and a temp dir; will fail due to missing API key but should
# print a useful error rather than panicking
INIT_EXIT=0
INIT_OUT=$("$BINARY" init --instance-root "$TMPDIR_INIT/instance" --yes 2>&1) || INIT_EXIT=$?
if [[ "$INIT_EXIT" -le 1 ]]; then
    pass "init --yes exits cleanly (exit $INIT_EXIT)"
elif echo "$INIT_OUT" | grep -qiE "api.?key|anthropic|credential|error"; then
    pass "init --yes fails with useful error message"
else
    fail "init --yes panicked or produced no useful output (exit $INIT_EXIT)"
fi

section "Import (missing file — expect error)"
check "import with missing file exits non-zero" 1 "" -- \
    import /nonexistent/path/to/agent.json --dry-run 2>/dev/null || true
IMPORT_EXIT=0
"$BINARY" import /nonexistent/file.agent.json 2>&1 | grep -qiE "no such|not found|error|cannot" \
    && pass "import missing file produces useful error" \
    || fail "import missing file produced no error message"

section "Seed-skills (dry-run with missing dir — expect error)"
SEED_EXIT=0
SEED_OUT=$("$BINARY" seed-skills --dir /nonexistent/dir --nous-id test-id --dry-run 2>&1) || SEED_EXIT=$?
if [[ "$SEED_EXIT" -ne 0 ]]; then
    pass "seed-skills with missing dir exits non-zero"
else
    fail "seed-skills with missing dir should have exited non-zero"
fi

section "Unknown subcommand"
UNKNOWN_EXIT=0
"$BINARY" totally-unknown-subcommand 2>/dev/null || UNKNOWN_EXIT=$?
if [[ "$UNKNOWN_EXIT" -ne 0 ]]; then
    pass "unknown subcommand exits non-zero (exit $UNKNOWN_EXIT)"
else
    fail "unknown subcommand should exit non-zero"
fi

# ── Summary ───────────────────────────────────────────────────────────────────
TOTAL=$(( PASS + FAIL + SKIP ))
echo ""
echo "────────────────────────────────────"
echo -e "${BOLD}Results${RESET}: $TOTAL tests — ${GREEN}$PASS passed${RESET}, ${RED}$FAIL failed${RESET}, ${YELLOW}$SKIP skipped${RESET}"

if [[ "${#FAILURES[@]}" -gt 0 ]]; then
    echo ""
    echo -e "${RED}Failed tests:${RESET}"
    for f in "${FAILURES[@]}"; do
        echo "  • $f"
    done
fi

echo "────────────────────────────────────"

if [[ "$FAIL" -gt 0 ]]; then
    exit 1
fi
exit 0
