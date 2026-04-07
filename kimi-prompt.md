# Task: Fix stale model constant (#2777)
Read: `gh issue view 2777 --json body`
Replace hardcoded dated model snapshot with the SONNET alias constant from koina defaults.
cargo check --workspace
git add -A && git commit -m "fix: use model alias constant instead of dated snapshot
Closes #2777
Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>" && git push origin fix/stale-model && gh pr create --title "fix: stale model constant" --body "Closes #2777"
