# Task: Fix formatting issues (#2773)

## Standards
Read AGENTS.md. Skip Setup.

## What to fix
Read: `gh issue view 2773 --json body`

Fix consecutive blank lines, trailing blank lines. These are mechanical — just delete the extra blank lines.

DO NOT change any code logic, only whitespace formatting.

## Validation
cargo check --workspace

## Completion
git add -A && git commit -m "chore: fix formatting — consecutive blank lines, trailing blanks

Part of #2773

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>"
git push origin chore/formatting-fixes
gh pr create --title "chore: formatting fixes" --body "Part of #2773"
