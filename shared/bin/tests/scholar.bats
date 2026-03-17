#!/usr/bin/env bats

# Tests for scholar (academic research tool)

SCRIPT_DIR="$(cd "$(dirname "$BATS_TEST_FILENAME")/.." && pwd)"
SCRIPT="$SCRIPT_DIR/scholar"

@test "help flag exits 0 and shows usage" {
    run "$SCRIPT" --help
    [ "$status" -eq 0 ]
    [[ "$output" == *"scholar"* ]]
    [[ "$output" == *"Search"* ]] || [[ "$output" == *"search"* ]]
}

@test "short help flag exits 0" {
    run "$SCRIPT" -h
    [ "$status" -eq 0 ]
}

@test "no arguments shows help" {
    run "$SCRIPT"
    [ "$status" -eq 0 ]
    [[ "$output" == *"scholar"* ]]
}

@test "fetch subcommand with no identifier exits non-zero" {
    run "$SCRIPT" fetch
    [ "$status" -ne 0 ]
}

@test "bib subcommand with no DOI exits non-zero" {
    run "$SCRIPT" bib
    [ "$status" -ne 0 ]
}

@test "info subcommand with no DOI exits non-zero" {
    run "$SCRIPT" info
    [ "$status" -ne 0 ]
}

@test "cite subcommand with no paper ID exits non-zero" {
    run "$SCRIPT" cite
    [ "$status" -ne 0 ]
}

@test "refs subcommand with no paper ID exits non-zero" {
    run "$SCRIPT" refs
    [ "$status" -ne 0 ]
}
