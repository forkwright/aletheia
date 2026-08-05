# Upstream snapshot — pinned oracle for the verbatim-drift metric

`cozo-core-src/` is a **verbatim, unmodified** copy of `cozo-core/src` from
[cozodb/cozo](https://github.com/cozodb/cozo) at commit
`481af058abac9444ea8c9c52c78f096ed4b5bfc4` (2024-12-04, the newest commit on
`main` touching `cozo-core/src` — upstream has been dormant since). Copyright
the CozoDB authors, licensed **MPL-2.0** (license text: `../LICENSE-MPL-2.0`,
one directory up).

Only `.rs` and `.pest` files were copied — the file types the drift metric
compares — at their original relative paths. Nothing else from the upstream
tree (docs, non-Rust scripts, build files) is present.

NOTE: this is a plain commit pin, not a tag — `481af05` postdates the
`v0.7.6` tag by a year and carries no tag of its own. Do not resolve the pin
from `cozo-core/Cargo.toml`'s `version = "0.7.6"` string: upstream never
bumped that string after tagging, so it still reads `0.7.6` at `481af05` and
for the rest of `main`'s (dormant) history. A prior snapshot of this file
pinned the `v0.7.6` tag on exactly that reasoning — 21 files under
`cozo-core/src` differ between the tag and `481af05` (`storage/newrocks.rs`
exists only at the later commit), so the two pins are not interchangeable.

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
edit these files — re-vendoring means re-cloning at the pinned commit and
replacing the tree wholesale, so the snapshot stays byte-identical to
upstream.

NOTE on lint exposure: `crates/krites/.kanon-lint-ignore` has no entry for
this directory, and none is needed today — `kanon lint --summary --rust`
is not in `.kanon-ci.toml`'s `[pipeline] stages` at all (repo-wide baseline
debt, unrelated to this crate; see that file's header), so nothing in
`kanon gate` walks Rust files for lint violations right now, vendored or
not. That exemption is incidental, not structural: basanos's file walker
(`crates/basanos/src/walker.rs`) discovers lint targets by directory walk
with a fixed `SKIP_DIRS` list (`target`, `node_modules`, `.git`, …) that
does not include vendor/snapshot directories, and there is no per-repo
directory-level lint-exclusion mechanism — only the per-file, per-rule
`RULE/name:path` entries in `.kanon-lint-ignore`. When RUST lint is
re-added to `.kanon-ci.toml` (tracked in the phase plan cited there), this
~65k-line, byte-exact-by-design copy will be swept in with it unless a
directory-level exclusion exists by then; editing these files to satisfy
lint would defeat their purpose. Tracked at forkwright/aletheia#6631.

## Updating the pin

Re-running the metric against a newer upstream state means re-vendoring
deliberately, not incrementally: clone `cozodb/cozo` at the new commit,
replace `cozo-core-src/` wholesale, update the commit above (a tag only if
upstream actually cut one at that commit — see the NOTE above), and
re-run `scripts/check-krites-verbatim-drift.py --calibrate` — a new
upstream snapshot invalidates the calibrated threshold until it is
re-derived against the new reference.
