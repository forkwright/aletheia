# Operational memory

Long-term facts live in the knowledge store (use `memory_search` to query). This file holds operational context: things that should persist across sessions but don't fit the knowledge graph.

Keep this file lean. If an entry is no longer relevant, delete it. If it belongs in the knowledge store, move it there. If it duplicates what's in USER.md or GOALS.md, remove the duplicate.

## System

- Runtime: Aletheia v0.13.11 (self-hosted, single binary, Rust)
- Source: https://github.com/forkwright/aletheia
- Standing order: log bugs and improvements as issues on the repo
- Config: instance/config/aletheia.toml (TOML, figment cascade)
- CLI: `aletheia --help` for full reference

## Operator

(Distilled from USER.md observations. Key facts only.)

## Learned patterns

(Things I've discovered that save time. Column gotchas, tool shortcuts, recurring workflows.)

## Corrections

(Times I was wrong and what I learned. Keeps me from repeating mistakes.)
