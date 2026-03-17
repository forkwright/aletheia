#!/usr/bin/env bats

# Tests for credential-refresh (Python script)

SCRIPT_DIR="$(cd "$(dirname "$BATS_TEST_FILENAME")/.." && pwd)"
SCRIPT="$SCRIPT_DIR/credential-refresh"

@test "help flag exits 0 and shows usage" {
    run "$SCRIPT" --help
    [ "$status" -eq 0 ]
    [[ "$output" == *"Auto-refresh"* ]]
    [[ "$output" == *"--force"* ]]
    [[ "$output" == *"--daemon"* ]]
    [[ "$output" == *"--status"* ]]
}

@test "short help flag exits 0" {
    run "$SCRIPT" -h
    [ "$status" -eq 0 ]
    [[ "$output" == *"Auto-refresh"* ]]
}

@test "unknown option exits non-zero" {
    run "$SCRIPT" --nonexistent-flag
    [ "$status" -ne 0 ]
}

@test "status with missing credential file reports error" {
    export HOME="$(mktemp -d)"
    run "$SCRIPT" --status
    # Should handle gracefully (prints error, doesn't crash with traceback)
    [[ "$output" == *"Cannot read"* ]] || [[ "$output" == *"ERROR"* ]]
    rm -rf "$HOME"
}

@test "refresh with missing credential file exits non-zero" {
    export HOME="$(mktemp -d)"
    run "$SCRIPT"
    [ "$status" -ne 0 ]
    rm -rf "$HOME"
}
