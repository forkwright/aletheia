# Upstream snapshot — pinned oracle for the verbatim-drift metric

`cozo-core-src/` is a **verbatim, unmodified** copy of `cozo-core/src` from
[cozodb/cozo](https://github.com/cozodb/cozo), tag `v0.7.6`, commit
`870f1e733f8e8aa2eb882c5653fc097981b95256`. Copyright the CozoDB authors,
licensed **MPL-2.0** (license text: `../LICENSE-MPL-2.0`, one directory up).

Only `.rs` and `.pest` files were copied — the file types the drift metric
compares — at their original relative paths. Nothing else from the upstream
tree (docs, non-Rust scripts, build files) is present.

## Why this exists

`scripts/check-krites-verbatim-drift.py` needs a fixed reference to compute
token-shingle Jaccard similarity against. A live fetch (clone-on-CI) would
make the metric's output depend on GitHub being reachable and on upstream
`main` not having moved since the metric was calibrated — either failure
mode turns a provenance measurement into a flaky one. Pinning a snapshot
in-tree makes the comparison deterministic and reproducible offline.

## What this is not

Not part of the `krites` crate's build: no `Cargo.toml`, no `mod` path
reaches into this directory, `cargo build`/`fmt`/`clippy` do not see it.
It exists solely as comparison data for the drift-metric script. Do not
edit these files — re-vendoring means re-cloning the tag and replacing the
tree wholesale, so the snapshot stays byte-identical to upstream.

## Updating the pin

Re-running the metric against a newer upstream state means re-vendoring
deliberately, not incrementally: clone `cozodb/cozo` at the new tag,
replace `cozo-core-src/` wholesale, update the tag/commit above, and
re-run `scripts/check-krites-verbatim-drift.py --calibrate` — a new
upstream snapshot invalidates the calibrated threshold until it is
re-derived against the new reference.
