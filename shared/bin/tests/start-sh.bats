#!/usr/bin/env bats
bats_require_minimum_version 1.5.0

# Tests for start.sh

SCRIPT_DIR="$(cd "$(dirname "$BATS_TEST_FILENAME")/.." && pwd)"
SCRIPT="$SCRIPT_DIR/start.sh"

setup() {
    export TEST_HOME="$(mktemp -d)"
}

teardown() {
    rm -rf "$TEST_HOME"
}

@test "exits non-zero when no credentials exist" {
    run env HOME="$TEST_HOME" ALETHEIA_ROOT="$TEST_HOME/.aletheia" bash "$SCRIPT"
    [ "$status" -ne 0 ]
    [[ "$output" == *"No credentials found"* ]]
}

@test "exits non-zero when aletheia binary does not exist" {
    mkdir -p "$TEST_HOME/.aletheia/credentials"
    cat > "$TEST_HOME/.aletheia/credentials/anthropic.json" <<'EOF'
{"token": "sk-ant-api03-fake", "apiKey": "sk-ant-api03-fake"}
EOF
    chmod 600 "$TEST_HOME/.aletheia/credentials/anthropic.json"
    run -127 env HOME="$TEST_HOME" ALETHEIA_ROOT="$TEST_HOME/.aletheia" bash "$SCRIPT"
    [ "$status" -eq 127 ]
}
