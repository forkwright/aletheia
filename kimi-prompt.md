# Task: Inline 2 small dependency crates

## Standards
Read AGENTS.md. Skip Setup.

## Issues

### #2672: Replace owo-colors and supports-color with inline ANSI
- Only used in `crates/eval/` for colored test output
- Read issue: `gh issue view 2672 --json body`
- Replace with direct ANSI escape codes (\x1b[31m for red, etc.)
- Remove owo-colors and supports-color from eval's Cargo.toml

### #2673: Replace `open` crate with inline platform command
- Used in theatron for "open URL in browser"
- Read issue: `gh issue view 2673 --json body`
- Replace with `std::process::Command::new("xdg-open")` on Linux, `open` on macOS
- Remove `open` from Cargo.toml

## Validation
```bash
cargo check --workspace
cargo check -p theatron-desktop
```

## Completion
git add -A
git commit -m "chore: inline owo-colors and open crate replacements

Closes #2672, #2673

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>"
git push origin chore/inline-small-deps
gh pr create --title "chore: inline small dep replacements" --body "Closes #2672, #2673"
