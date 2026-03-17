#!/usr/bin/env bats

# Tests for transcribe

SCRIPT_DIR="$(cd "$(dirname "$BATS_TEST_FILENAME")/.." && pwd)"
SCRIPT="$SCRIPT_DIR/transcribe"

setup() {
    export ALETHEIA_ROOT="$(mktemp -d)"
}

teardown() {
    rm -rf "$ALETHEIA_ROOT"
}

@test "help flag exits 0" {
    run "$SCRIPT" --help
    [ "$status" -eq 0 ]
}

@test "short help flag exits 0" {
    run "$SCRIPT" -h
    [ "$status" -eq 0 ]
}

@test "exits non-zero with no input file" {
    run "$SCRIPT"
    [ "$status" -ne 0 ]
    [[ "$output" == *"No input file"* ]]
}

@test "exits non-zero with nonexistent file" {
    run "$SCRIPT" /nonexistent/file.wav
    [ "$status" -ne 0 ]
    [[ "$output" == *"File not found"* ]]
}
