# fuzz

Fuzz testing workspace for Aletheia. Separate Cargo workspace that links against crate APIs to exercise parsing and serialization surfaces with arbitrary input.

## Structure

```
fuzz/
  Cargo.toml            # Standalone workspace, cargo-fuzz metadata
  fuzz_targets/         # One file per fuzz target binary
  corpus/               # Seed inputs, one directory per target
  deny.toml -> ../deny.toml
  rustfmt.toml -> ../rustfmt.toml
```

## Targets

| Target | Crates exercised | Surface |
|--------|-----------------|---------|
| `fuzz_tool_dispatch` | hermeneus, nous, koina | ContentBlock deserialization, ToolCall roundtrip, ToolName validation, LoopDetector |
| `fuzz_config_parsing` | taxis | AletheiaConfig JSON/TOML parsing, validate_section, individual config structs |
| `fuzz_knowledge_roundtrip` | mneme | Fact deserialization, timestamp parsing, FactType classification, KnowledgeStore insert/read |

## Commands

```bash
# Install cargo-fuzz (once)
cargo install cargo-fuzz

# Run a target (from fuzz/ directory)
cargo fuzz run fuzz_tool_dispatch
cargo fuzz run fuzz_config_parsing
cargo fuzz run fuzz_knowledge_roundtrip

# Run with time limit (seconds)
cargo fuzz run fuzz_tool_dispatch -- -max_total_time=300

# Run with specific corpus directory
cargo fuzz run fuzz_tool_dispatch corpus/fuzz_tool_dispatch

# List available targets
cargo fuzz list

# Minimize a crash artifact
cargo fuzz tmin fuzz_tool_dispatch artifacts/fuzz_tool_dispatch/<crash-file>

# Generate coverage report
cargo fuzz coverage fuzz_tool_dispatch
```

## Adding a target

1. Create `fuzz_targets/fuzz_<name>.rs` with `#![no_main]` and `fuzz_target!` macro
2. Add `[[bin]]` entry in `Cargo.toml` with `doc = false`
3. Create `corpus/fuzz_<name>/` with seed files
4. Add any new crate dependencies to `[dependencies]` in `Cargo.toml`

## Corpus conventions

- JSON seeds: `.json` extension, one per edge case
- Binary seeds: `.bin` extension for non-UTF-8 payloads
- Text seeds: `.txt` extension for plain-text parsing targets
- Name seeds descriptively: `seed_<what_it_tests>.<ext>`

## Key patterns

- **No panics expected**: all fuzz targets use fallible APIs (`let _ = ...`). A panic is a real bug.
- **Shared state**: `fuzz_knowledge_roundtrip` uses a `LazyLock<Arc<KnowledgeStore>>` shared across iterations for performance.
- **Deterministic derivation**: Fact fields (confidence, tier, type) are derived from fuzzer bytes, not random, to maintain reproducibility.

## Workspace relationship

This is a standalone workspace (`[workspace] members = ["."]`) to avoid interfering with the parent Cargo workspace. It references parent crates via relative `path` dependencies. Shared config (deny.toml, rustfmt.toml) is symlinked from the parent.
