# Task: Fix issue #2778
Read the issue: `gh issue view 2778 --json body`
Follow the fix instructions in the issue body.
cargo check --workspace
git add -A && git commit -m "fix: address #2778 — claude-md
Closes #2778
Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>"
git push origin fix/wave-17
gh pr create --title "fix: #2778 claude-md" --body "Closes #2778"
