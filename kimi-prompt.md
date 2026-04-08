# Task: Fix issue #2838
Read the issue: `gh issue view 2838 --json body`
Follow the fix instructions in the issue body.
cargo check --workspace
git add -A && git commit -m "fix: address #2838 — default-features
Closes #2838
Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>"
git push origin fix/wave-5
gh pr create --title "fix: #2838 default-features" --body "Closes #2838"
