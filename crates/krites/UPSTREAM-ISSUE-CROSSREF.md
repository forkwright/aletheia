# Upstream issue cross-reference (#6799)

A one-time, hand-run snapshot cross-referencing `cozodb/cozo`'s issue tracker
against the 141 `derived` rows in [`PROVENANCE.toml`](PROVENANCE.toml), taken
alongside adding `upstream_last_commit_date`/`upstream_status` to that
ledger's `[meta]`. Not CI-gated and not re-derived automatically — see
"Method" for how to re-run it.

## Upstream snapshot facts

- Pin: `481af058abac9444ea8c9c52c78f096ed4b5bfc4` (`crates/krites/PROVENANCE.toml`
  `[meta].upstream_ref`)
- That commit **is** upstream's current default-branch (`main`) HEAD — verified via
  `gh api repos/cozodb/cozo/commits/main` — so the pin carries zero commits of
  drift; `repos/cozodb/cozo.pushed_at` is also `2024-12-04T12:49:06Z`. Upstream is
  not archived, just dormant: no push since the pin.
- Issues pulled `2026-09-06` via `gh issue list --repo cozodb/cozo --state all
  --limit 300`: **157 total, 43 open, 114 closed**.

## Method

1. Pull every issue (title + body) with `gh issue list --repo cozodb/cozo
   --state open --json number,title,body,labels,createdAt,url` (open issues
   only carry live risk; a closed one is either fixed upstream — irrelevant,
   our pin predates or postdates the fix depending on date — or wontfix).
2. Regex-extract literal `cozo-core/src/...rs` and bare `....rs` path mentions
   from each issue's title+body.
3. Keep only the mentions that are actual `cozo-core/src/` paths appearing as
   an `upstream_path` among the 141 `derived` rows (71 unique upstream files).
   A bare `foo.rs` mention is kept only when the surrounding issue is
   unambiguously about `cozo-core` itself (see "Excluded" below for the ones
   that looked like matches and were not).
4. For each surviving match, confirm on our own tree whether the cited logic
   is actually present (grep), not just "the filename lines up" — a filename
   match alone is not evidence a derived row inherited anything.

Re-run: `gh issue list --repo cozodb/cozo --state open --limit 300 --json
number,title,body,createdAt,url`, then match bodies against the
`upstream_path` column of `PROVENANCE.toml`'s `derived` rows.

## Confirmed match: cozodb/cozo#312 (open, filed 2026-07-13 — after our pin)

**"Validity: an integral float in a validity timestamp is silently coerced to
microseconds — the write path stamps rows at 1970 (silent data corruption)"**
<https://github.com/cozodb/cozo/issues/312>

Filed by the maintainer of a *different* hard fork of CozoDB from the same
fork point (`481af05`, 2024-12-04 — our exact pin), reproduced against "a
clean checkout of upstream `481af05`" (our exact commit, not their fork). The
report names four affected call sites, three of which it states are
"verbatim upstream code": `data/value.rs` (`Num::get_int`), `data/relation.rs`,
`data/functions.rs`, plus `parse/query.rs` and `runtime/tests.rs` as
secondary mentions. `Num::get_int` accepts any whole-numbered float
(`f.round() == *f`) and casts it straight to `i64`, so a `Validity` timestamp
supplied in seconds (what `now()`/`parse_timestamp()` return) is silently
reinterpreted as microseconds — 1e6 too small — landing writes at
1970-01-01. Ordinary reads look fine; the corruption only surfaces under time
travel.

Cross-reference against our ledger:

| Upstream path (issue #312) | krites derived row | verbatim_pct | status |
|---|---|---:|---|
| `data/value.rs` | `src/data/value.rs` | 60.3% | derived |
| `data/relation.rs` | `src/data/relation.rs` | 54.2% | derived |
| `data/functions.rs` | `src/data/functions/mod.rs` (+ 11 split siblings) | 2.1% (mod.rs) | derived |
| `parse/query.rs` | `src/parse/query/mod.rs` (+ 4 split siblings) | 0.0% (mod.rs) | derived |
| `runtime/tests.rs` | `src/runtime/tests/mod.rs` (+ 4 split siblings) | 0.0% (mod.rs) | derived |

**Confirmed present on our tree, not just filename-adjacent**: `krites`'s own
`data/value.rs::get_int` (line 601) carries the identical
`if f.round() == *f { ... *f as i64 }` coercion the report describes. Krites
takes the same write path a `Validity` column would use, so the same
silent-corruption class is plausible here. This doc records the finding; it
does **not** fix it — fixing `Num`'s float-to-int coercion, auditing every
`Validity` call site, and proving no live consumer relies on the (buggy)
seconds-as-microseconds behavior is a design decision and more than a
cross-reference doc should decide un-reviewed. **Follow-up: file a krites
issue against `data/value.rs`'s `get_int` before this is acted on.**

## Other path mentions found, and why they don't add rows here

- **cozodb/cozo#272**, "Unconditional use of optional `rayon` dependency"
  (open, 2024-07-09, predates our pin) — cites `cozo-core/src/query/eval.rs:200`
  (`.par_iter()` used without gating on the `rayon` feature), which maps to
  our `src/query/eval.rs` (derived, 42.3%). This is a *build-configuration*
  complaint (breaks a `rayon`-less build), not a correctness bug, and krites
  builds `rayon` in unconditionally (see `Cargo.toml`) — not applicable to us
  as filed, noted for completeness rather than as a finding.
- **cozodb/cozo#200** and **#208** cite bare `lib.rs` — the crate root, which
  every file transitively touches; #200 is about a wasm/web `export_relations`
  call signature and #208 a Rust borrow-checker error, neither of which
  concerns krites (no wasm target, no shared call site). Excluded as
  filename-only noise, not a real match.
- **cozodb/cozo#307** ("Benchmarks don't compile") cites `pokec.rs`,
  `wiki_pagerank.rs`, `time_travel.rs` — these are upstream **benchmark**
  fixtures under `cozo-core/benches/`, not `src/`; no `PROVENANCE.toml` row
  derives from a benchmark file. Excluded.
- **cozodb/cozo#298** ("cargo update breaks due to newer rayon version") cites
  `.../src/input/edgelist.rs` and `.../src/iter/mod.rs` — these paths belong
  to a transitive dependency in the reporter's own backtrace (petgraph/rayon
  internals), not `cozo-core`. Excluded.
- **cozodb/cozo#306** ("Sled backend: fix broken `del()` ...") cites
  `sled.rs` — a real `cozo-core/src/storage/sled.rs`, but the Sled backend was
  never restored in krites at all (RETIREMENT-PLAN.md: fjall is the only
  storage backend, tracked as one of the seven undocumented drops in #6865).
  No `PROVENANCE.toml` row exists for it because there is nothing here to
  affect — recorded as confirmation the drop is inert, not a finding.
- **cozodb/cozo#256** ("Binary embeddings support") superficially matches
  `.rs` via `pgvecto.rs` — that is a third-party Postgres extension's *name*,
  not a file path. Regex false positive, excluded.

## What this doc is not

Not a security audit, not exhaustive (open-issue bodies only; a real bug
without an explicit `cozo-core/src/...rs` citation in the issue text is
invisible to this method), and not gated by any script — a future re-run is
manual, per "Method" above. Treat the #312 finding as a lead requiring its
own issue and fix, not as something this snapshot resolves.
