# Task: Fix issue #2844
Read the issue: `gh issue view 2844 --json body`
Follow the fix instructions in the issue body.
cargo check --workspace
git add -A && git commit -m "fix: address #2844 — wildcard-bug
Closes #2844
Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>"
git push origin fix/wave-1
gh pr create --title "fix: #2844 wildcard-bug" --body "Closes #2844"
