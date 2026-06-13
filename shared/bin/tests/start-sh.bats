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
    run env HOME="$TEST_HOME" ALETHEIA_ROOT="$TEST_HOME/instance" bash "$SCRIPT"
    [ "$status" -ne 0 ]
    [[ "$output" == *"No credentials found"* ]]
}

@test "exits non-zero when aletheia binary does not exist" {
    mkdir -p "$TEST_HOME/instance/config/credentials"
    cat > "$TEST_HOME/instance/config/credentials/anthropic.json" <<'EOF'
{"token": "sk-ant-api03-fake", "apiKey": "sk-ant-api03-fake"}
EOF
    chmod 600 "$TEST_HOME/instance/config/credentials/anthropic.json"
    run -127 env HOME="$TEST_HOME" ALETHEIA_ROOT="$TEST_HOME/instance" ALETHEIA_BIN="$TEST_HOME/missing/aletheia" bash "$SCRIPT"
    [ "$status" -eq 127 ]
    [[ "$output" == *"Aletheia binary not executable"* ]]
}

@test "runs ALETHEIA_BIN with credentials from instance env file" {
    mkdir -p "$TEST_HOME/instance/config"
    cat > "$TEST_HOME/instance/config/env" <<'EOF'
ANTHROPIC_API_KEY=sk-ant-api03-fake
EOF
    cat > "$TEST_HOME/aletheia" <<'EOF'
#!/usr/bin/env bash
printf 'root=%s\n' "$ALETHEIA_ROOT"
printf 'key=%s\n' "${ANTHROPIC_API_KEY:-}"
printf 'args=%s\n' "$*"
EOF
    chmod +x "$TEST_HOME/aletheia"

    run env HOME="$TEST_HOME" ALETHEIA_ROOT="$TEST_HOME/instance" ALETHEIA_BIN="$TEST_HOME/aletheia" bash "$SCRIPT" serve
    [ "$status" -eq 0 ]
    [[ "$output" == *"root=$TEST_HOME/instance"* ]]
    [[ "$output" == *"key=sk-ant-api03-fake"* ]]
    [[ "$output" == *"args=serve"* ]]
}
