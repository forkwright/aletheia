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

@test "does not import Claude Code credentials from default home path" {
    mkdir -p "$TEST_HOME/.claude"
    cat > "$TEST_HOME/.claude/.credentials.json" <<'EOF'
{"apiKey": "sk-ant-api03-from-claude"}
EOF

    run env HOME="$TEST_HOME" ALETHEIA_ROOT="$TEST_HOME/instance" bash "$SCRIPT"
    [ "$status" -ne 0 ]
    [[ "$output" == *"No credentials found"* ]]
    [[ "$output" != *"API key synced from Claude Code credentials"* ]]
    [ ! -f "$TEST_HOME/instance/config/credentials/anthropic.json" ]
}

@test "imports Claude Code credentials only from CLAUDE_CODE_CREDS" {
    local cc_creds="$TEST_HOME/custom-claude.json"
    cat > "$cc_creds" <<'EOF'
{"apiKey": "sk-ant-api03-from-claude"}
EOF
    cat > "$TEST_HOME/aletheia" <<'EOF'
#!/usr/bin/env bash
printf 'started\n'
EOF
    chmod +x "$TEST_HOME/aletheia"

    run env HOME="$TEST_HOME" ALETHEIA_ROOT="$TEST_HOME/instance" CLAUDE_CODE_CREDS="$cc_creds" ALETHEIA_BIN="$TEST_HOME/aletheia" bash "$SCRIPT"
    [ "$status" -eq 0 ]
    [[ "$output" == *"API key synced from Claude Code credentials"* ]]
    [[ "$output" == *"started"* ]]
    [[ "$(python3 -c 'import json, sys; print(json.load(open(sys.argv[1]))["apiKey"])' "$TEST_HOME/instance/config/credentials/anthropic.json")" == "sk-ant-api03-from-claude" ]]
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

@test "defaults ALETHEIA_ROOT to ~/aletheia/instance when unset" {
    # WHY: verify the canonical default path; drift to ~/.aletheia or other paths must fail this test.
    local default_instance="$TEST_HOME/aletheia/instance"
    mkdir -p "$default_instance/config"
    cat > "$default_instance/config/env" <<'EOF'
ANTHROPIC_API_KEY=sk-ant-api03-fake
EOF
    cat > "$TEST_HOME/aletheia-bin" <<'EOF'
#!/usr/bin/env bash
printf 'root=%s\n' "$ALETHEIA_ROOT"
EOF
    chmod +x "$TEST_HOME/aletheia-bin"

    # Run without ALETHEIA_ROOT set; HOME is overridden to TEST_HOME so the
    # default ~/aletheia/instance resolves to $TEST_HOME/aletheia/instance.
    run env HOME="$TEST_HOME" ALETHEIA_BIN="$TEST_HOME/aletheia-bin" bash "$SCRIPT" serve
    [ "$status" -eq 0 ]
    [[ "$output" == *"root=$default_instance"* ]]
}
