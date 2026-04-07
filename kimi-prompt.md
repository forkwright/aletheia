# Task: Fix safe QA issues #2650, #2652, #2653

## Standards
Read AGENTS.md in repo root. Skip Setup section.

## Issues

### #2650: Remove 11 unused imports
Run `cargo check --workspace 2>&1 | grep "unused import"` to find them.
Remove each unused import. Simple deletions.

### #2652: Fix sha2 version split
Three crates use `sha2 = "0.11"` directly instead of `sha2 = { workspace = true }`:
- `crates/aletheia/Cargo.toml:79`
- `crates/graphe/Cargo.toml:30`
- `crates/krites/Cargo.toml:43`

Check the workspace Cargo.toml for the pinned version. If it pins "0.10" but crates need "0.11", update the workspace pin to "0.11" and make all three use `{ workspace = true }`.

### #2653: Move tempfile to dev-dependencies in nous
In `crates/nous/Cargo.toml`, `tempfile` appears in both `[dependencies]` and `[dev-dependencies]`. Remove it from `[dependencies]` (keep it in `[dev-dependencies]`). Verify all tempfile uses are in `#[cfg(test)]`.

## Validation
```bash
cargo check --workspace
```

## Completion
```bash
git add -A
git commit -m "fix: unused imports + sha2 version split + tempfile dep scope

Closes #2650, #2652, #2653

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>"
git push origin fix/qa-safe-batch
gh pr create --title "fix: unused imports, sha2 version split, tempfile dep" --body "Closes #2650, #2652, #2653"
```
