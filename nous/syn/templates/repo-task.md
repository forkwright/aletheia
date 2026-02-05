# Repo Task Template

## Git Workflow (CRITICAL)

1. **Check if repo exists locally:** `/mnt/ssd/moltbot/repos/{repo-name}`
2. **If not exists:** `gh repo clone forkwright/{repo-name} /mnt/ssd/moltbot/repos/{repo-name}`
3. **If exists:** `cd /mnt/ssd/moltbot/repos/{repo-name} && git fetch origin && git checkout develop && git pull`
4. **Create feature branch:** `git checkout -b feature/{issue-number}-{short-desc}`
5. **Make ONLY targeted changes** — do NOT commit unrelated files
6. **Stage only your changes:** `git add {specific-files}` — NEVER `git add .` or `git add -A`
7. **Commit with conventional format:** `git commit -m "feat(scope): description\n\nCloses #{issue}"`
8. **Push branch:** `git push -u origin feature/{issue-number}-{short-desc}`
9. **Create PR:** `gh pr create --base develop --title "feat(scope): description" --body "Closes #{issue}"`

## Before Creating PR

- [ ] Only changed files related to the issue
- [ ] All tests pass locally if possible
- [ ] No TODOs or placeholder code
- [ ] Follows project coding standards
- [ ] Commit message follows conventional commits

## NEVER DO

- `git add .` or `git add -A` (stages everything)
- Commit files you didn't intentionally modify
- Push to main/develop directly
- Create PR with 100+ changed files for a small feature
