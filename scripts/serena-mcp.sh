#!/usr/bin/env bash
set -euo pipefail
# serena-mcp.sh - register or start the Serena MCP server against this workspace.
#
# Serena (https://github.com/oraios/serena) wraps rust-analyzer as MCP tools,
# giving agents LSP-powered navigation (find_symbol, find_referencing_symbols,
# rename_symbol, etc.) across aletheia's 23 crates.
#
# See docs/MCP-SERVERS.md for the full reference.
#
# Usage:
#   scripts/serena-mcp.sh register       # claude mcp add - project-scoped
#   scripts/serena-mcp.sh register-user  # claude mcp add --scope user
#   scripts/serena-mcp.sh start          # foreground stdio server (for smoke tests or non-Claude clients)
#   scripts/serena-mcp.sh check          # verify serena + uv are installed
#
# Requires: uv (for install/upgrade), claude CLI (for register commands).

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" &>/dev/null && pwd)"
WORKSPACE_ROOT="$(cd -- "${SCRIPT_DIR}/.." &>/dev/null && pwd)"

log() { printf '[serena-mcp] %s\n' "$*" >&2; }
die() { log "ERROR: $*"; exit 1; }

ensure_serena() {
    if ! command -v serena >/dev/null 2>&1; then
        die "serena not found on PATH. Install with: uv tool install -p 3.13 'serena-agent@latest' --prerelease=allow"
    fi
}

ensure_claude_cli() {
    if ! command -v claude >/dev/null 2>&1; then
        die "claude CLI not found on PATH. Install Claude Code first: https://docs.anthropic.com/en/docs/claude-code"
    fi
}

cmd_check() {
    if command -v uv >/dev/null 2>&1; then log "uv: $(uv --version)"; else log "uv: NOT INSTALLED"; fi
    if command -v serena >/dev/null 2>&1; then log "serena: $(serena --help 2>&1 | head -1)"; else log "serena: NOT INSTALLED"; fi
    if command -v claude >/dev/null 2>&1; then log "claude: $(claude --version 2>/dev/null || echo present)"; else log "claude: NOT INSTALLED"; fi
    log "workspace: ${WORKSPACE_ROOT}"
}

cmd_register() {
    ensure_serena
    ensure_claude_cli
    log "Registering project-scoped serena MCP server for ${WORKSPACE_ROOT}"
    claude mcp add serena -- serena start-mcp-server \
        --context claude-code \
        --project "${WORKSPACE_ROOT}" \
        --enable-web-dashboard false \
        --open-web-dashboard false
}

cmd_register_user() {
    ensure_serena
    ensure_claude_cli
    log "Registering user-scoped serena MCP server (project auto-detected from CWD)"
    claude mcp add --scope user serena -- serena start-mcp-server \
        --context claude-code \
        --project-from-cwd \
        --enable-web-dashboard false \
        --open-web-dashboard false
}

cmd_start() {
    ensure_serena
    log "Starting Serena MCP (stdio) for ${WORKSPACE_ROOT}"
    exec serena start-mcp-server \
        --context claude-code \
        --project "${WORKSPACE_ROOT}" \
        --transport stdio \
        --enable-web-dashboard false \
        --open-web-dashboard false
}

usage() {
    sed -n '2,16p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
    exit "${1:-0}"
}

main() {
    local sub="${1:-}"
    case "${sub}" in
        register)      cmd_register ;;
        register-user) cmd_register_user ;;
        start)         cmd_start ;;
        check)         cmd_check ;;
        -h|--help|help|"") usage 0 ;;
        *) log "unknown subcommand: ${sub}"; usage 1 ;;
    esac
}

main "$@"
