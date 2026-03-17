#!/usr/bin/env bats

# Tests for aletheia-backup

SCRIPT_DIR="$(cd "$(dirname "$BATS_TEST_FILENAME")/.." && pwd)"
SCRIPT="$SCRIPT_DIR/aletheia-backup"

setup() {
    export ALETHEIA_ROOT="$(mktemp -d)"
    export ALETHEIA_CONFIG_DIR="$ALETHEIA_ROOT"
    export ALETHEIA_BACKUP_DIR="$ALETHEIA_ROOT/backups"
    mkdir -p "$ALETHEIA_ROOT"
}

teardown() {
    rm -rf "$ALETHEIA_ROOT"
}

@test "help flag exits 0 and shows usage" {
    run "$SCRIPT" --help
    [ "$status" -eq 0 ]
    [[ "$output" == *"Usage:"* ]]
    [[ "$output" == *"--full"* ]]
    [[ "$output" == *"--dest"* ]]
    [[ "$output" == *"--list"* ]]
}

@test "short help flag exits 0" {
    run "$SCRIPT" -h
    [ "$status" -eq 0 ]
    [[ "$output" == *"Usage:"* ]]
}

@test "unknown option exits non-zero" {
    run "$SCRIPT" --invalid-flag
    [ "$status" -ne 0 ]
    [[ "$output" == *"Unknown option"* ]]
}

@test "list with no backups shows none" {
    run "$SCRIPT" --list
    [ "$status" -eq 0 ]
    [[ "$output" == *"(none)"* ]] || [[ "$output" == *"No backup directory"* ]]
}

@test "list with existing backup directory shows backups header" {
    mkdir -p "$ALETHEIA_BACKUP_DIR"
    run "$SCRIPT" --list
    [ "$status" -eq 0 ]
    [[ "$output" == *"Backups in"* ]]
}

@test "creates backup archive from empty config" {
    mkdir -p "$ALETHEIA_ROOT"
    run "$SCRIPT"
    [ "$status" -eq 0 ]
    [[ "$output" == *"Backup complete"* ]]
    # Verify a tar.gz was created
    [ "$(ls "$ALETHEIA_BACKUP_DIR"/*.tar.gz 2>/dev/null | wc -l)" -ge 1 ]
}

@test "custom dest flag changes backup location" {
    custom_dest="$(mktemp -d)"
    run "$SCRIPT" --dest "$custom_dest"
    [ "$status" -eq 0 ]
    [[ "$output" == *"$custom_dest"* ]]
    [ "$(ls "$custom_dest"/*.tar.gz 2>/dev/null | wc -l)" -ge 1 ]
    rm -rf "$custom_dest"
}

@test "full flag includes full in backup name" {
    run "$SCRIPT" --full
    [ "$status" -eq 0 ]
    [[ "$output" == *"full (core + data)"* ]]
    ls "$ALETHEIA_BACKUP_DIR"/aletheia-full-*.tar.gz >/dev/null 2>&1
}
