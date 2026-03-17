#!/usr/bin/env bats

# Tests for pplx (Perplexity research wrapper)

SCRIPT_DIR="$(cd "$(dirname "$BATS_TEST_FILENAME")/.." && pwd)"
SCRIPT="$SCRIPT_DIR/pplx"

@test "exits non-zero with no arguments" {
    # pplx requires PERPLEXITY_API_KEY, so unset it to test arg validation
    # The script checks args before using the key due to set -euo pipefail,
    # but the :? expansion fires first. Either way, non-zero exit is correct.
    unset PERPLEXITY_API_KEY
    run "$SCRIPT"
    [ "$status" -ne 0 ]
}

@test "exits non-zero without PERPLEXITY_API_KEY" {
    unset PERPLEXITY_API_KEY
    run "$SCRIPT" "test query"
    [ "$status" -ne 0 ]
    [[ "$output" == *"PERPLEXITY_API_KEY"* ]]
}

@test "shows usage in error when no args and key is set" {
    export PERPLEXITY_API_KEY="test-key-not-real"
    run "$SCRIPT"
    [ "$status" -ne 0 ]
    [[ "$output" == *"Usage:"* ]]
}
