#!/usr/bin/env bats

# Tests for credential-refresh (Python script)

SCRIPT_DIR="$(cd "$(dirname "$BATS_TEST_FILENAME")/.." && pwd)"
SCRIPT="$SCRIPT_DIR/credential-refresh"

resolved_claude_code_cred() {
    python3 - "$SCRIPT" <<'PY'
import importlib.machinery
import importlib.util
import sys

loader = importlib.machinery.SourceFileLoader("credential_refresh", sys.argv[1])
spec = importlib.util.spec_from_loader(loader.name, loader)
module = importlib.util.module_from_spec(spec)
loader.exec_module(module)
print(module.CLAUDE_CODE_CRED)
PY
}

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

@test "status reads credential file from ALETHEIA_ROOT" {
    export HOME="$(mktemp -d)"
    mkdir -p "$HOME/instance/config/credentials"
    cat > "$HOME/instance/config/credentials/anthropic.json" <<'EOF'
{"token": "sk-ant-oat01-fake-token", "refreshToken": "fake-refresh", "expiresAt": 4102444800000}
EOF

    run env HOME="$HOME" ALETHEIA_ROOT="$HOME/instance" "$SCRIPT" --status
    [ "$status" -eq 0 ]
    [[ "$output" == *"$HOME/instance/config/credentials/anthropic.json"* ]]
    [[ "$output" == *"VALID"* ]]

    rm -rf "$HOME"
}

@test "claude code credential path uses CLAUDE_CODE_CREDS" {
    export HOME="$(mktemp -d)"
    local override="$HOME/custom/claude-code.json"

    run env HOME="$HOME" CLAUDE_CODE_CREDS="$override" SCRIPT="$SCRIPT" bash -c "$(declare -f resolved_claude_code_cred); resolved_claude_code_cred"
    [ "$status" -eq 0 ]
    [ "$output" = "$override" ]

    rm -rf "$HOME"
}

@test "claude code credential path reads claudeCodeCredentials from config" {
    export HOME="$(mktemp -d)"
    mkdir -p "$HOME/instance/config"
    cat > "$HOME/instance/config/aletheia.toml" <<'EOF'
[credential]
claudeCodeCredentials = "~/custom/claude-code.json"
EOF

    run env HOME="$HOME" ALETHEIA_ROOT="$HOME/instance" SCRIPT="$SCRIPT" bash -c "$(declare -f resolved_claude_code_cred); resolved_claude_code_cred"
    [ "$status" -eq 0 ]
    [ "$output" = "$HOME/custom/claude-code.json" ]

    rm -rf "$HOME"
}
