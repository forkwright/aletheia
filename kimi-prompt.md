# Task: Fix 5 issues sequentially

Read AGENTS.md. Skip Setup.

Fix these 5 issues ONE AT A TIME. After each fix, commit separately with conventional commit format.

## 1. #2845: 6 wildcard matches on internal/cross-crate enums hide new variants
Read: `gh issue view 2845 --json body`
Replace wildcard `_` catch-all match arms with explicit variants on internal enums.

## 2. #2840: clone hotspots — config.id cloned ~15x in pipeline
Read: `gh issue view 2840 --json body`
Replace repeated `.clone()` calls with references or Arc where appropriate.

## 3. #2820: nous metrics tests mirror episteme pattern — 5 more assertion-free tests
Read: `gh issue view 2820 --json body`
Add meaningful assertions to the identified tests that currently have no assertions.

## 4. #2818: krites debug/exploratory tests left in codebase — zero assertion tests
Read: `gh issue view 2818 --json body`
Add assertions to exploratory tests or convert them to proper tests with expectations.

## 5. #2655: dormant features and dead config — local-llm, custom_commands, dianoia metrics
Read: `gh issue view 2655 --json body`
Remove dead feature flags and unused config from the identified locations.

After ALL fixes:
```bash
cargo check --workspace
git push origin fix/kimi-wave3
gh pr create --title "fix: cleanup wave 3 (#2845, #2840, #2820, #2818, #2655)" --body "Closes #2845, #2840, #2820, #2818, #2655"
```
