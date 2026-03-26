# Feature flags

Compile-time feature flags controlling optional functionality. All flags are set on the `aletheia` binary crate in `crates/aletheia/Cargo.toml`.

## Default features

These are enabled by `cargo build` with no extra flags:

| Flag | Purpose |
|------|---------|
| `tui` | Terminal dashboard (ratatui). Adds `theatron-tui` dependency. Disable with `--no-default-features` for headless builds. |
| `recall` | Knowledge pipeline: KnowledgeStore, vector search, extraction persistence. Enables `mneme-engine` and `storage-fjall` in downstream crates. |
| `storage-fjall` | fjall LSM-tree storage backend for mneme. Pure Rust, LZ4 compression. |
| `embed-candle` | Local ML embeddings via candle (BAAI/bge-small-en-v1.5, 384 dims). Pure Rust, no C++ deps. |

## Optional features

Enable with `--features <flag>`:

| Flag | Crate | Purpose |
|------|-------|---------|
| `mcp` | `aletheia` | MCP server (Model Context Protocol) via diaporeia. Adds context-window cost and auth friction; disabled by default for local deployments. |
| `computer-use` | `organon` | Computer use tool: screen capture, action dispatch, Landlock sandbox. Requires Linux kernel 5.13+ with Landlock support. |
| `local-llm` | `hermeneus` | OpenAI-compatible local LLM provider support (vLLM, etc.). |
| `tls` | `aletheia` | TLS termination for the pylon HTTP gateway. |
| `migrate-qdrant` | `aletheia` | Qdrant migration support for importing memories from an external Qdrant instance. |

## Test tiers

See [test-tiers.md](test-tiers.md) for details.

| Flag | Purpose |
|------|---------|
| `test-core` | Enables storage and engine tests across the workspace. Propagated to mneme, nous, pylon, and other crates. |
| `test-full` | Superset of `test-core`. Adds ML embedding tests (requires model download). |

## Build examples

```bash
cargo build --release                                  # default features (tui, recall, storage-fjall, embed-candle)
cargo build --release --no-default-features --features tls  # headless server with TLS, no TUI or ML
cargo build --release --features mcp                   # default + MCP server
cargo build --release --features "mcp,local-llm"       # default + MCP + local LLM
cargo test --workspace --features test-core            # run storage/engine tests
cargo test --workspace --features test-full            # run all tests including ML
```
