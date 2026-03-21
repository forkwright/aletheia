# build-performance

Build timing analysis for `cargo build --release`. Profiled with `--timings` on 2026-03-19.
Machine: x86_64 Linux, 180 logical CPUs (Verda). Toolchain: rustc 1.94.0.

Baseline: 4m 33s (273.5s), 614 units, max concurrency 69.

---

## Bottleneck crates

| Rank | Crate | Time | Notes |
|------|-------|------|-------|
| 1 | `aletheia` (bin) | 64.6s | Final link + codegen; serial with codegen-units=1 |
| 2 | `aletheia-mneme` | 63.4s | Knowledge engine; largest crate by LOC (~110K) |
| 3 | `candle-transformers` | 44.3s | Local ML inference - unavoidable; feature-gated |
| 4 | `candle-core` | 33.6s | Tensor ops - unavoidable; feature-gated |
| 5 | `onig_sys` (build script) | 32.4s | C regex library pulled in by tokenizers+syntect |
| 6 | `ring` (build script) | 28.9s | C/asm cryptographic library |
| 7 | `aletheia-pylon` | 18.5s | API server; heavy axum + utoipa codegen |
| 8 | `tokenizers` | 14.5s | HuggingFace tokenizer library |
| 9 | `theatron-tui` (bin) | 14.4s | TUI binary |
| 10 | `aletheia-diaporeia` | 13.6s | MCP adapter |

---

## Quick wins implemented

### 1. Replace `onig` with `fancy-regex` in tokenizers and syntect

**Saves: ~32s** (eliminates `onig_sys` C build)

`tokenizers` (HuggingFace) and `syntect` both default to the `onig` backend, which requires
compiling `onig_sys` - a C foreign-function interface to the Oniguruma regex library. This C
build takes 32.4s and cannot be parallelised with Rust compilation.

Both crates support `fancy-regex` as a pure-Rust alternative. `fancy-regex` covers all
lookahead/lookbehind patterns required by BERT tokenizers and syntax highlighting grammars used
in this codebase.

Changes:
- `crates/mneme/Cargo.toml`: `tokenizers` feature `onig` → `fancy-regex`
- `crates/theatron/tui/Cargo.toml`: `syntect` explicit `default-fancy` instead of implicit `default-onig`

### 2. Remove unused `reqwest` `blocking` feature from `aletheia` binary

**Saves: marginal link time** (eliminates dead code in final binary)

`crates/aletheia/Cargo.toml` declared `reqwest = { features = ["blocking"] }` but no code in the
crate uses `reqwest::blocking`. The blocking HTTP client spawns an internal thread pool and
increases binary size. Removed.

---

## Remaining bottlenecks (no quick win)

### candle-core + candle-transformers (78s combined)

These are the local ML embedding crates. They compile large amounts of numeric kernel code.
No trimming possible without dropping the `embed-candle` feature, which is intentionally on by
default (see WARNING in `crates/aletheia/Cargo.toml` line 16). The feature guard exists because
it was accidentally removed three times (#1263, #1326, #1378).

Mitigation: CI caches these via `Swatinem/rust-cache`. Incremental local builds skip them
after the first compile.

### ring build script (28.9s)

`ring` compiles C and assembly for cryptographic primitives used by `rustls`. Replacing `ring`
with `aws-lc-rs` as the rustls backend is feasible (both are supported by rustls 0.23) but
requires testing correctness on all target platforms. Left for a dedicated PR.

### aletheia-mneme (63.4s)

Largest workspace crate at ~110K lines. The storage-fjall and mneme-engine features pull in
dozens of additional dependencies. No structural quick wins; this reflects real code mass.

### aletheia (binary link, 64.6s)

Final link with LTO=thin and codegen-units=1. These settings are correct for release: thin LTO
provides meaningful size/speed improvements. Single codegen unit maximises inter-procedural
optimisation. The link time is dominated by the candle and mneme objects.

---

## Profile settings (current)

```toml
[profile.dev]
opt-level = 1
codegen-units = 256        # fast incremental builds

[profile.dev.package."*"]
opt-level = 2              # optimise deps, keep local code fast

[profile.release]
lto = "thin"
codegen-units = 1          # maximum optimisation
strip = "symbols"
```

These settings match the standard in `standards/RUST.md`. No changes warranted.

---

## How to re-profile

```bash
cargo build --release --timings
# Output: target/cargo-timings/cargo-timing.html
```

Open the HTML file in a browser. The waterfall chart shows the critical path.
Sort the table by "Total" to find the longest-compiling units.
