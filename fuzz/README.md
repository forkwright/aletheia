# Aletheia Fuzz Testing

Fuzz targets for Aletheia's critical parsing and serialization surfaces, built on [cargo-fuzz](https://rust-fuzz.github.io/book/cargo-fuzz.html) and [libFuzzer](https://llvm.org/docs/LibFuzzer.html).

## Targets

| Target | What it tests |
|--------|--------------|
| `fuzz_tool_dispatch` | Tool input JSON parsing, ContentBlock deserialization, ToolName validation, loop detection |
| `fuzz_config_parsing` | Runtime config from JSON and TOML, section validation, individual config struct parsing |
| `fuzz_knowledge_roundtrip` | Fact serialization, timestamp parsing, FactType classification, knowledge store insert/read |

## Prerequisites

```bash
# Nightly toolchain required for libfuzzer instrumentation
rustup install nightly

# Install cargo-fuzz
cargo install cargo-fuzz
```

## Running

```bash
cd fuzz/

# Run a single target indefinitely (Ctrl+C to stop)
cargo fuzz run fuzz_tool_dispatch

# Run with a time limit (300 seconds)
cargo fuzz run fuzz_tool_dispatch -- -max_total_time=300

# Run all targets sequentially (5 minutes each)
for target in $(cargo fuzz list); do
  cargo fuzz run "$target" -- -max_total_time=300
done

# List available targets
cargo fuzz list
```

## Corpus

Seed inputs live in `corpus/<target_name>/`. Each target has hand-crafted seeds covering:

- Valid inputs (happy path)
- Malformed inputs (truncated, wrong types, missing fields)
- Boundary values (empty, oversized, null bytes)
- Binary payloads (invalid UTF-8)
- Adversarial structures (deep nesting, duplicate keys)

libFuzzer extends the corpus automatically during runs. Generated corpus entries are gitignored via the parent `.gitignore`.

## Triaging crashes

When a target finds a crash, libFuzzer writes the input to `artifacts/<target_name>/`:

```bash
# Reproduce
cargo fuzz run fuzz_tool_dispatch artifacts/fuzz_tool_dispatch/crash-<hash>

# Minimize to smallest reproducing input
cargo fuzz tmin fuzz_tool_dispatch artifacts/fuzz_tool_dispatch/crash-<hash>

# Generate coverage report
cargo fuzz coverage fuzz_tool_dispatch
```

## Architecture

This directory is a standalone Cargo workspace to avoid interfering with the parent workspace build. Target crates are referenced via relative path dependencies. Shared configuration (deny.toml, rustfmt.toml) is symlinked from the repository root.
