#!/usr/bin/env bats

# Tests for wiki (Wikipedia lookup)

SCRIPT_DIR="$(cd "$(dirname "$BATS_TEST_FILENAME")/.." && pwd)"
SCRIPT="$SCRIPT_DIR/wiki"

@test "help flag exits 0 and shows usage" {
    run "$SCRIPT" --help
    [ "$status" -eq 0 ]
    [[ "$output" == *"Wikipedia"* ]]
    [[ "$output" == *"--search"* ]]
    [[ "$output" == *"--refs"* ]]
    [[ "$output" == *"--full"* ]]
}

@test "short help flag exits 0" {
    run "$SCRIPT" -h
    [ "$status" -eq 0 ]
    [[ "$output" == *"Wikipedia"* ]]
}

@test "no arguments shows help" {
    run "$SCRIPT"
    [ "$status" -eq 0 ]
    [[ "$output" == *"usage"* ]] || [[ "$output" == *"Wikipedia"* ]]
}

@test "json flag is accepted without error on valid query" {
    # This makes a network call, but validates the flag parsing
    run "$SCRIPT" --json "Test" 2>/dev/null
    # Either succeeds (network available) or fails gracefully
    # The key assertion: the flag doesn't cause an argument parsing error
    [[ "$status" -eq 0 ]] || [[ "$output" != *"unrecognized arguments"* ]]
}
