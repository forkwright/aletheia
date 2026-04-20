#!/usr/bin/env bash
set -euo pipefail
# git-guard.sh - inspect tool:called payloads for destructive git operations.
#
# Receives the tool:called event payload on stdin as JSON.
# Exits:
#   0 - no issue detected
#   1 - operator-approval required (warns or blocks per ALETHEIA_GIT_GUARD_BLOCK)
#   2 - permanently blocked operation (always an error)

BLOCK_MODE="${ALETHEIA_GIT_GUARD_BLOCK:-0}"

payload=$(cat)
tool_name=$(printf '%s' "$payload" | grep -o '"toolName":"[^"]*"' | head -1 | cut -d'"' -f4)

# Only inspect tool calls that could run shell commands.
case "${tool_name:-}" in
  Bash|bash|shell|run_command|execute) : ;;
  *) exit 0 ;;
esac

# Extract the command text from the payload.
# The field may be "command", "cmd", or "input" depending on the tool.
command_text=$(printf '%s' "$payload" \
  | grep -o '"command":"[^"]*"' \
  | head -1 \
  | cut -d'"' -f4 || true)

if [[ -z "${command_text:-}" ]]; then
  # No parseable command field — pass through.
  exit 0
fi

# ---------------------------------------------------------------------------
# Permanently blocked patterns — no override path.
# These operations cannot be undone and should never run autonomously.
# ---------------------------------------------------------------------------

PERMANENTLY_BLOCKED=(
  "git push --force"
  "git push -f "
  "git push -f$"
  "push --force-with-lease"
  "git push --mirror"
  "git push --delete"
  "git push origin :refs/"
  "git branch -D"
  "git branch --delete --force"
)

for pattern in "${PERMANENTLY_BLOCKED[@]}"; do
  if printf '%s' "$command_text" | grep -qF "$pattern"; then
    printf 'git-guard: PERMANENTLY BLOCKED — "%s" matches pattern "%s"\n' \
      "$command_text" "$pattern" >&2
    printf 'Reason: force push, remote branch deletion, and mirror push cannot be undone.\n' >&2
    printf 'To proceed: operator must run this command manually outside the agent session.\n' >&2
    exit 2
  fi
done

# ---------------------------------------------------------------------------
# Operator-approval required — warn by default, block when ALETHEIA_GIT_GUARD_BLOCK=1.
# ---------------------------------------------------------------------------

APPROVAL_REQUIRED=(
  "git push origin main"
  "git push origin master"
  "git reset --hard"
  "git clean -f"
  "git clean -fd"
  "git checkout -- "
  "git restore --staged"
  "git rebase -i"
  "git filter-repo"
  "git filter-branch"
)

for pattern in "${APPROVAL_REQUIRED[@]}"; do
  if printf '%s' "$command_text" | grep -qF "$pattern"; then
    if [[ "$BLOCK_MODE" == "1" ]]; then
      printf 'git-guard: BLOCKED (operator approval required) — "%s" matches "%s"\n' \
        "$command_text" "$pattern" >&2
      printf 'Set ALETHEIA_GIT_GUARD_BLOCK=0 or use --override-push-guard flag to allow.\n' >&2
      exit 1
    else
      printf 'git-guard: WARNING — "%s" requires operator approval (pattern: "%s")\n' \
        "$command_text" "$pattern" >&2
      printf 'Set ALETHEIA_GIT_GUARD_BLOCK=1 to block this operation instead of warning.\n' >&2
      # Exit 0: warn mode does not block the operation.
      exit 0
    fi
  fi
done

exit 0
