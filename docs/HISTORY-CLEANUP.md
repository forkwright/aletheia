# Git history cleanup plan

Items to address via `git filter-repo`:

1. Rewrite author/committer emails to GitHub noreply address
2. Strip all `Co-authored-by: Claude` trailers
3. Remove "Reverse-engineered from Claude Code's OAuth" from commit messages
4. Verify prosoche/config.yaml (deleted file) no longer in history
5. Verify Summus references removed from commit messages

This is a one-time destructive operation requiring force push.
All forks and local clones will need to re-clone after execution.
