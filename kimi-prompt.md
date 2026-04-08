# Task: Fix issue #2791
Read the issue: `gh issue view 2791 --json body`
Follow the fix instructions in the issue body.
cargo check --workspace
git add -A && git commit -m "fix: address #2791 — deps-audit
Closes #2791
Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>"
git push origin fix/wave-16
gh pr create --title "fix: #2791 deps-audit" --body "Closes #2791"
