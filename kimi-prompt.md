# Task: Fix issue #2834
Read the issue: `gh issue view 2834 --json body`
Follow the fix instructions in the issue body.
cargo check --workspace
git add -A && git commit -m "fix: address #2834 — sbom
Closes #2834
Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>"
git push origin fix/wave-14
gh pr create --title "fix: #2834 sbom" --body "Closes #2834"
