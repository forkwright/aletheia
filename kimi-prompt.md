# Task: Fix audit issues #2721, #2722, #2723

## Standards
Read AGENTS.md. Skip Setup.

## Issues

### #2721: Loop detection threshold=3 is too aggressive
Read: `gh issue view 2721 --json body`
The stuck detection fires after 3 repeated errors, which is too sensitive for
transient network issues. Raise the default to 5 or make it configurable
(it may already be in StuckConfig — check).

### #2722: Generic serialization errors erase file/field context
Read: `gh issue view 2722 --json body`
Serialization errors should include the file path or field name that failed.
Add context to the error variants.

### #2723: Validation errors display as empty; tool errors lack exit code
Read: `gh issue view 2723 --json body`
Check Display impls for validation errors — they may have empty messages.
Tool execution errors should include the process exit code.

## Validation
cargo check --workspace

## Completion
git add -A
git commit -m "fix: audit fixes — loop threshold, serialization context, validation display

Closes #2721, #2722, #2723

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>"
git push origin fix/audit-batch
gh pr create --title "fix: audit fixes batch" --body "Closes #2721, #2722, #2723"
