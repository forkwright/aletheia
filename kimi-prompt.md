# Task: Fix issue #2832
Read the issue: `gh issue view 2832 --json body`
Follow the fix instructions in the issue body.
cargo check --workspace
git add -A && git commit -m "fix: address #2832 — regex-compile
Closes #2832
Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>"
git push origin fix/wave-2
gh pr create --title "fix: #2832 regex-compile" --body "Closes #2832"
