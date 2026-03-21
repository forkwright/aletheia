# Dependency Budget

## Current State (as of 2026-03-19, v0.13.0)

| Metric | Count |
|--------|-------|
| Workspace crates | 19 |
| External packages in lockfile | 657 |
| Total packages in lockfile | 676 |
| Direct external deps of the `aletheia` binary | 42 |

The 1714-line `cargo tree --depth 1` output includes all workspace crates' own
direct dependencies; the 657 external packages figure (from `cargo metadata`)
is the authoritative count of third-party crates pulled in transitively.

## Target Budget

| Metric | Budget |
|--------|--------|
| External packages in lockfile | ≤ 700 |
| Direct external deps of the `aletheia` binary | ≤ 55 |

The budget is intentionally loose for the lockfile because the heavy
`embed-candle` feature gate already accounts for most of the count.
Keeping the binary's direct dep count below 55 preserves clarity in
`Cargo.toml` and makes security audit surface tractable.

## Heavy dependencies and feature gates

The following crates contribute significantly to build time and binary size.
All are feature-gated so that default `--no-default-features` builds or test
builds can opt out.

| Crate | Feature gate | Location | Notes |
|-------|-------------|----------|-------|
| `candle-core` v0.9.2 | `embed-candle` | `aletheia-mneme` | Local ML embedding computation |
| `candle-nn` v0.9.2 | `embed-candle` | `aletheia-mneme` | Neural-net layers for embedding |
| `candle-transformers` v0.9.2 | `embed-candle` | `aletheia-mneme` | Transformer model inference |
| `tokenizers` v0.22 | `embed-candle` | `aletheia-mneme` | HuggingFace tokenizer (Rust binding) |
| `hf-hub` | `embed-candle` | `aletheia-mneme` | Model download from HuggingFace Hub |
| `ndarray` v0.17 | `embed-candle` | `aletheia-mneme` | N-dimensional arrays for vectors |

### Feature-gate chain

```
aletheia[embed-candle]
  └── aletheia-mneme[embed-candle]
        ├── candle-core
        ├── candle-nn
        ├── candle-transformers
        ├── tokenizers
        ├── hf-hub
        └── ndarray
```

The `embed-candle` feature is enabled in the `default` feature set of the
`aletheia` binary (`Cargo.toml` comment: "WARNING: embed-candle must remain
in defaults"). CI test runs that do not need embedding can pass
`--no-default-features --features tui,recall,storage-fjall` to skip the
candle compilation, which saves substantial build time.

## CI enforcement

No automated dep-count gate exists in CI. To add one, the
`rust.yml` or `nightly.yml` workflow can include a step such as:

```yaml
- name: Check dependency budget
  run: |
    count=$(cargo metadata --format-version 1 | python3 -c "
    import json,sys
    d=json.load(sys.stdin)
    ext=[p for p in d['packages'] if p['id'] not in set(d.get('workspace_members',[]))]
    print(len(ext))
    ")
    echo "External packages: $count"
    if [ "$count" -gt 700 ]; then
      echo "::error::Dependency budget exceeded ($count > 700)"
      exit 1
    fi
```

## Adding a new dependency

Before adding a crate:

1. Check if the functionality is already available from an existing dep.
2. Prefer crates already in the lockfile (zero marginal cost).
3. For heavy / compile-time-expensive crates, gate behind an optional feature.
4. Update this file's counts after `cargo metadata` confirms the new total.
