#!/usr/bin/env bats

# Tests for gcal (Google Calendar CLI)

SCRIPT_DIR="$(cd "$(dirname "$BATS_TEST_FILENAME")/.." && pwd)"
SCRIPT="$SCRIPT_DIR/gcal"

@test "help command exits 0 and shows usage" {
    run "$SCRIPT" help
    [ "$status" -eq 0 ]
    [[ "$output" == *"Usage:"* ]]
    [[ "$output" == *"today"* ]]
    [[ "$output" == *"tomorrow"* ]]
    [[ "$output" == *"week"* ]]
    [[ "$output" == *"calendars"* ]]
}

@test "no arguments shows usage" {
    run "$SCRIPT"
    [ "$status" -eq 0 ]
    [[ "$output" == *"Usage:"* ]]
}

@test "unknown command exits non-zero" {
    run "$SCRIPT" nonexistent-command
    [ "$status" -ne 0 ]
    [[ "$output" == *"Unknown command"* ]]
}
