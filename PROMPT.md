# Task: Push blanket lint suppressions to individual sites — storage module pilot

Skip the Setup section — you're already in a worktree.

## Context

krites (crates/krites/) is an embedded Datalog engine. Its lib.rs has module-level `#[expect(...)]` annotations that suppress clippy lints across entire modules. This hides the actual suppression sites.

This task: push suppressions from lib.rs down to individual functions/items in the `storage` module only (smallest, 1.7K LOC). This is a pilot for the full overhaul.

## What to do

1. Read `crates/krites/src/lib.rs` — find the `#[expect]` block for the `storage` module
2. Note which lints are suppressed (e.g., `clippy::as_conversions`, `clippy::indexing_slicing`, etc.)
3. Remove the blanket suppression for `storage` from lib.rs
4. Run `cargo clippy -p krites 2>&1` — collect all new warnings in `storage/` files
5. For each warning: add `#[expect(clippy::lint_name, reason = "...")]` on the specific function or impl block
6. The reason must explain WHY (e.g., `reason = "u32-to-usize widening is safe on 64-bit"`)

## Rules

- Do NOT fix the underlying issues — only add per-site suppressions with reasons
- Only touch files in `crates/krites/src/storage/`
- Run `cargo clippy -p krites` after — zero warnings
- Run `cargo test -p krites` — all tests pass
- Commit: `refactor(krites): localize lint suppressions in storage module`
- Gate-Passed: kanon 0.1.0

## Acceptance criteria

- Zero module-level `#[expect]` for the storage module in lib.rs
- Every individual `#[expect]` has a `reason = "..."` string
- `cargo clippy -p krites` — zero warnings
- `cargo test -p krites` — all pass
