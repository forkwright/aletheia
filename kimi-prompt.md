# Task: Fix issue #2767
Read the issue: `gh issue view 2767 --json body`
Follow the fix instructions in the issue body.
cargo check --workspace
git add -A && git commit -m "fix: address #2767 — non-exhaustive
Closes #2767
Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>"
git push origin fix/wave-20
gh pr create --title "fix: #2767 non-exhaustive" --body "Closes #2767"
