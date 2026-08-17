# Third-party notice — krites

`krites` is substantially derived from **CozoDB** (`cozo-core`), copyright the CozoDB authors, licensed under the **Mozilla Public License 2.0**. A copy of that license sits beside this file at [LICENSE-MPL-2.0](LICENSE-MPL-2.0); upstream is <https://github.com/cozodb/cozo>.

## What is derived

This table is rendered from [`PROVENANCE.toml`](PROVENANCE.toml) — the file-level provenance ledger — never hand-edited. `verbatim_pct` is the share of each file's non-blank lines that a line-level diff (Python `difflib.SequenceMatcher`, order-sensitive) matches against the upstream file at the pinned commit; it is measured per file, not assumed from a subsystem average.

A `sovereign` row's `verbatim_pct` is not always 0.0: when the row still has something to measure against — a completed `dual` soak (PLAN.md §2(c)), or a from-scratch rewrite with a natural predecessor — the ledger retains that predecessor as `replaced_upstream_path` (shown below as "cf. `path`") and keeps measuring against it. `upstream_path` itself stays `none` on every `sovereign` row either way: this is not an MPL lineage claim, only a retained comparison the anti-backsliding gate keeps honest. A row with no predecessor at all (`replaced_upstream_path` also `none`) has nothing to measure and its `verbatim_pct` is genuinely 0.0.

- Upstream: <https://github.com/cozodb/cozo>, pinned at `481af058abac9444ea8c9c52c78f096ed4b5bfc4`
- 210 files under `src/`: 142 derived, 68 sovereign, 0 dual
- Mean verbatim match across the 142 derived files: 44.1% (unweighted average of the per-file `verbatim_pct` column below)

| File | Upstream | Verbatim | Status |
|---|---|---:|---|
| `src/async_surface.rs` | — | 0.0% | sovereign |
| `src/counterfactual.rs` | — | 0.0% | sovereign |
| `src/counterfactual_tests.rs` | — | 0.0% | sovereign |
| `src/data/aggr/boolean.rs` | `data/aggr.rs` | 71.7% | derived |
| `src/data/aggr/misc.rs` | `data/aggr.rs` | 71.9% | derived |
| `src/data/aggr/mod.rs` | `data/aggr.rs` | 46.0% | derived |
| `src/data/aggr/numeric.rs` | `data/aggr.rs` | 56.9% | derived |
| `src/data/error.rs` | — | 0.0% | sovereign |
| `src/data/expr/expr_impl.rs` | `data/expr.rs` | 64.5% | derived |
| `src/data/expr/mod.rs` | `data/expr.rs` | 44.8% | derived |
| `src/data/expr/op.rs` | `data/expr.rs` | 84.1% | derived |
| `src/data/functions/aggregate.rs` | `data/functions.rs` | 12.7% | derived |
| `src/data/functions/bits.rs` | `data/functions.rs` | 32.7% | derived |
| `src/data/functions/collections.rs` | `data/functions.rs` | 8.1% | derived |
| `src/data/functions/math/arithmetic.rs` | `data/functions.rs` | 27.1% | derived |
| `src/data/functions/math/mod.rs` | `data/functions.rs` | 18.9% | derived |
| `src/data/functions/math/transcendental.rs` | `data/functions.rs` | 6.5% | derived |
| `src/data/functions/mod.rs` | `data/functions.rs` | 2.1% | derived |
| `src/data/functions/string.rs` | `data/functions.rs` | 11.9% | derived |
| `src/data/functions/temporal.rs` | `data/functions.rs` | 6.9% | derived |
| `src/data/functions/trig.rs` | `data/functions.rs` | 22.6% | derived |
| `src/data/functions/utility.rs` | `data/functions.rs` | 27.5% | derived |
| `src/data/functions/vector.rs` | `data/functions.rs` | 22.1% | derived |
| `src/data/json.rs` | `data/json.rs` | 48.8% | derived |
| `src/data/memcmp.rs` | `data/memcmp.rs` | 44.2% | derived |
| `src/data/mod.rs` | `data/mod.rs` | 17.4% | derived |
| `src/data/program/atoms.rs` | `data/program.rs` | 65.8% | derived |
| `src/data/program/fixed_rule.rs` | `data/program.rs` | 57.1% | derived |
| `src/data/program/input.rs` | `data/program.rs` | 70.4% | derived |
| `src/data/program/magic.rs` | `data/program.rs` | 82.2% | derived |
| `src/data/program/mod.rs` | `data/program.rs` | 0.0% | derived |
| `src/data/program/search/atom_impl.rs` | `data/program.rs` | 80.0% | derived |
| `src/data/program/search/hnsw_normalize.rs` | `data/program.rs` | 46.7% | derived |
| `src/data/program/search/lsh_fts.rs` | `data/program.rs` | 48.0% | derived |
| `src/data/program/search/mod.rs` | `data/program.rs` | 75.5% | derived |
| `src/data/program/types.rs` | `data/program.rs` | 80.2% | derived |
| `src/data/relation.rs` | `data/relation.rs` | 54.2% | derived |
| `src/data/symb.rs` | `data/symb.rs` | 66.0% | derived |
| `src/data/tests/aggrs.rs` | `data/tests/aggrs.rs` | 27.1% | derived |
| `src/data/tests/exprs.rs` | `data/tests/exprs.rs` | 22.2% | derived |
| `src/data/tests/functions/arithmetic.rs` | `data/tests/functions.rs` | 23.6% | derived |
| `src/data/tests/functions/collections.rs` | `data/tests/functions.rs` | 38.1% | derived |
| `src/data/tests/functions/mod.rs` | `data/tests/functions.rs` | 0.0% | derived |
| `src/data/tests/functions/string_ops.rs` | `data/tests/functions.rs` | 42.9% | derived |
| `src/data/tests/functions/type_conversion.rs` | `data/tests/functions.rs` | 11.1% | derived |
| `src/data/tests/functions/validity_units.rs` | — | 0.0% | sovereign |
| `src/data/tests/json.rs` | `data/tests/json.rs` | 7.7% | derived |
| `src/data/tests/memcmp.rs` | `data/tests/memcmp.rs` | 55.0% | derived |
| `src/data/tests/mod.rs` | `data/tests/mod.rs` | 55.6% | derived |
| `src/data/tests/proptest_memcmp.rs` | — | 0.0% | sovereign |
| `src/data/tests/validity.rs` | `data/tests/validity.rs` | 41.7% | derived |
| `src/data/tests/values.rs` | `data/tests/values.rs` | 14.0% | derived |
| `src/data/tuple.rs` | `data/tuple.rs` | 52.0% | derived |
| `src/data/value.rs` | `data/value.rs` | 60.3% | derived |
| `src/datalog.pest` | `cozoscript.pest` | 94.2% | derived |
| `src/error.rs` | — | 0.0% | sovereign |
| `src/fixed_rule/algos/all_pairs_shortest_path.rs` | cf. `fixed_rule/algos/all_pairs_shortest_path.rs` | 15.4% | sovereign |
| `src/fixed_rule/algos/astar.rs` | cf. `fixed_rule/algos/astar.rs` | 18.4% | sovereign |
| `src/fixed_rule/algos/bfs.rs` | cf. `fixed_rule/algos/bfs.rs` | 18.9% | sovereign |
| `src/fixed_rule/algos/degree_centrality.rs` | cf. `fixed_rule/algos/degree_centrality.rs` | 32.1% | sovereign |
| `src/fixed_rule/algos/dfs.rs` | cf. `fixed_rule/algos/dfs.rs` | 18.0% | sovereign |
| `src/fixed_rule/algos/kcore.rs` | — | 0.0% | sovereign |
| `src/fixed_rule/algos/kruskal.rs` | cf. `fixed_rule/algos/kruskal.rs` | 14.9% | sovereign |
| `src/fixed_rule/algos/label_propagation.rs` | cf. `fixed_rule/algos/label_propagation.rs` | 13.6% | sovereign |
| `src/fixed_rule/algos/louvain.rs` | `fixed_rule/algos/louvain.rs` | 29.2% | derived |
| `src/fixed_rule/algos/mod.rs` | cf. `fixed_rule/algos/mod.rs` | 82.9% | sovereign |
| `src/fixed_rule/algos/pagerank.rs` | `fixed_rule/algos/pagerank.rs` | 37.0% | derived |
| `src/fixed_rule/algos/prim.rs` | cf. `fixed_rule/algos/prim.rs` | 14.9% | sovereign |
| `src/fixed_rule/algos/random_walk.rs` | cf. `fixed_rule/algos/random_walk.rs` | 19.6% | sovereign |
| `src/fixed_rule/algos/shortest_path_bfs.rs` | cf. `fixed_rule/algos/shortest_path_bfs.rs` | 19.1% | sovereign |
| `src/fixed_rule/algos/shortest_path_dijkstra.rs` | cf. `fixed_rule/algos/shortest_path_dijkstra.rs` | 9.2% | sovereign |
| `src/fixed_rule/algos/strongly_connected_components.rs` | cf. `fixed_rule/algos/strongly_connected_components.rs` | 18.0% | sovereign |
| `src/fixed_rule/algos/top_sort.rs` | cf. `fixed_rule/algos/top_sort.rs` | 27.4% | sovereign |
| `src/fixed_rule/algos/triangles.rs` | cf. `fixed_rule/algos/triangles.rs` | 23.5% | sovereign |
| `src/fixed_rule/algos/yen.rs` | cf. `fixed_rule/algos/yen.rs` | 11.8% | sovereign |
| `src/fixed_rule/csr/mod.rs` | — | 0.0% | sovereign |
| `src/fixed_rule/csr/page_rank.rs` | — | 0.0% | sovereign |
| `src/fixed_rule/error.rs` | — | 0.0% | sovereign |
| `src/fixed_rule/mod.rs` | `fixed_rule/mod.rs` | 57.5% | derived |
| `src/fixed_rule/tests/centrality_spanning.rs` | — | 0.0% | sovereign |
| `src/fixed_rule/tests/connectivity_misc.rs` | — | 0.0% | sovereign |
| `src/fixed_rule/tests/mod.rs` | — | 0.0% | sovereign |
| `src/fixed_rule/tests/path_algorithms.rs` | — | 0.0% | sovereign |
| `src/fixed_rule/tests/proptest_algos.rs` | — | 0.0% | sovereign |
| `src/fixed_rule/tests/wave5_reference_semantics.rs` | — | 0.0% | sovereign |
| `src/fixed_rule/utilities/constant.rs` | cf. `fixed_rule/utilities/constant.rs` | 17.2% | sovereign |
| `src/fixed_rule/utilities/mod.rs` | cf. `fixed_rule/utilities/mod.rs` | 0.0% | sovereign |
| `src/fixed_rule/utilities/reorder_sort.rs` | cf. `fixed_rule/utilities/reorder_sort.rs` | 17.5% | sovereign |
| `src/fixed_rule/utilities/rrf.rs` | — | 0.0% | sovereign |
| `src/fts/README.md` | `fts/README.md` | 100.0% | derived |
| `src/fts/ast.rs` | `fts/ast.rs` | 71.3% | derived |
| `src/fts/config.rs` | `fts/mod.rs` | 34.1% | derived |
| `src/fts/error.rs` | — | 0.0% | sovereign |
| `src/fts/indexing.rs` | `fts/indexing.rs` | 31.4% | derived |
| `src/fts/mod.rs` | `fts/mod.rs` | 30.1% | derived |
| `src/fts/tokenizer/alphanum_only.rs` | `fts/tokenizer/alphanum_only.rs` | 67.7% | derived |
| `src/fts/tokenizer/ascii_folding_filter/fold_table.rs` | cf. `fts/tokenizer/ascii_folding_filter.rs` | 0.0% | sovereign |
| `src/fts/tokenizer/ascii_folding_filter/fold_table/fold_table_sovereign/generate.py` | — | 0.0% | sovereign |
| `src/fts/tokenizer/ascii_folding_filter/fold_table/fold_table_sovereign/mod.rs` | cf. `fts/tokenizer/ascii_folding_filter.rs` | 0.0% | sovereign |
| `src/fts/tokenizer/ascii_folding_filter/fold_table/fold_table_sovereign/table.rs` | cf. `fts/tokenizer/ascii_folding_filter.rs` | 0.0% | sovereign |
| `src/fts/tokenizer/ascii_folding_filter/mod.rs` | `fts/tokenizer/ascii_folding_filter.rs` | 75.0% | derived |
| `src/fts/tokenizer/ascii_folding_filter/tests/foldings_a_i.rs` | `fts/tokenizer/ascii_folding_filter.rs` | 60.6% | derived |
| `src/fts/tokenizer/ascii_folding_filter/tests/foldings_j_s.rs` | `fts/tokenizer/ascii_folding_filter.rs` | 56.6% | derived |
| `src/fts/tokenizer/ascii_folding_filter/tests/foldings_num_sym.rs` | `fts/tokenizer/ascii_folding_filter.rs` | 77.0% | derived |
| `src/fts/tokenizer/ascii_folding_filter/tests/foldings_t_z.rs` | `fts/tokenizer/ascii_folding_filter.rs` | 57.9% | derived |
| `src/fts/tokenizer/ascii_folding_filter/tests/mod.rs` | `fts/tokenizer/ascii_folding_filter.rs` | 60.5% | derived |
| `src/fts/tokenizer/empty_tokenizer.rs` | `fts/tokenizer/empty_tokenizer.rs` | 85.7% | derived |
| `src/fts/tokenizer/lower_caser.rs` | `fts/tokenizer/lower_caser.rs` | 74.6% | derived |
| `src/fts/tokenizer/mod.rs` | `fts/tokenizer/mod.rs` | 52.6% | derived |
| `src/fts/tokenizer/ngram_tokenizer.rs` | `fts/tokenizer/ngram_tokenizer.rs` | 64.1% | derived |
| `src/fts/tokenizer/raw_tokenizer.rs` | `fts/tokenizer/raw_tokenizer.rs` | 74.6% | derived |
| `src/fts/tokenizer/remove_long.rs` | `fts/tokenizer/remove_long.rs` | 72.9% | derived |
| `src/fts/tokenizer/simple_tokenizer.rs` | `fts/tokenizer/simple_tokenizer.rs` | 73.7% | derived |
| `src/fts/tokenizer/split_compound_words.rs` | `fts/tokenizer/split_compound_words.rs` | 86.8% | derived |
| `src/fts/tokenizer/stemmer.rs` | `fts/tokenizer/stemmer.rs` | 89.0% | derived |
| `src/fts/tokenizer/stop_word_filter/mod.rs` | cf. `fts/tokenizer/stop_word_filter/mod.rs` | 0.0% | sovereign |
| `src/fts/tokenizer/stop_word_filter/sovereign/NOTICE.md` | — | 0.0% | sovereign |
| `src/fts/tokenizer/stop_word_filter/sovereign/gen_stopwords.py` | cf. `fts/tokenizer/stop_word_filter/gen_stopwords.py` | 0.0% | sovereign |
| `src/fts/tokenizer/stop_word_filter/sovereign/mod.rs` | cf. `fts/tokenizer/stop_word_filter/mod.rs` | 15.5% | sovereign |
| `src/fts/tokenizer/stop_word_filter/sovereign/stopwords.rs` | cf. `fts/tokenizer/stop_word_filter/stopwords.rs` | 76.6% | sovereign |
| `src/fts/tokenizer/tokenized_string.rs` | `fts/tokenizer/tokenized_string.rs` | 76.4% | derived |
| `src/fts/tokenizer/tokenizer_impl.rs` | `fts/tokenizer/tokenizer_impl.rs` | 74.1% | derived |
| `src/fts/tokenizer/whitespace_tokenizer.rs` | `fts/tokenizer/whitespace_tokenizer.rs` | 73.7% | derived |
| `src/hot_reload.rs` | — | 0.0% | sovereign |
| `src/lib.rs` | `lib.rs` | 0.9% | derived |
| `src/parse/error.rs` | — | 0.0% | sovereign |
| `src/parse/expr/bytecode.rs` | `parse/expr.rs` | 29.5% | derived |
| `src/parse/expr/mod.rs` | `parse/expr.rs` | 24.1% | derived |
| `src/parse/expr/strings.rs` | `parse/expr.rs` | 0.0% | derived |
| `src/parse/fts.rs` | `parse/fts.rs` | 25.0% | derived |
| `src/parse/imperative.rs` | `parse/imperative.rs` | 21.1% | derived |
| `src/parse/mod.rs` | `parse/mod.rs` | 24.8% | derived |
| `src/parse/query/atoms.rs` | `parse/query.rs` | 33.0% | derived |
| `src/parse/query/fixed_rules.rs` | `parse/query.rs` | 10.3% | derived |
| `src/parse/query/mod.rs` | `parse/query.rs` | 0.0% | derived |
| `src/parse/query/options.rs` | `parse/query.rs` | 27.5% | derived |
| `src/parse/query/program.rs` | `parse/query.rs` | 16.9% | derived |
| `src/parse/schema.rs` | `parse/schema.rs` | 33.6% | derived |
| `src/parse/sys/index.rs` | `parse/sys.rs` | 18.0% | derived |
| `src/parse/sys/mod.rs` | `parse/sys.rs` | 18.9% | derived |
| `src/parse/sys/parse.rs` | `parse/sys.rs` | 9.9% | derived |
| `src/query/compile.rs` | `query/compile.rs` | 74.6% | derived |
| `src/query/context.rs` | — | 0.0% | sovereign |
| `src/query/error.rs` | — | 0.0% | sovereign |
| `src/query/eval.rs` | `query/eval.rs` | 42.3% | derived |
| `src/query/graph.rs` | `query/graph.rs` | 56.7% | derived |
| `src/query/logical.rs` | `query/logical.rs` | 77.3% | derived |
| `src/query/magic.rs` | `query/magic.rs` | 76.2% | derived |
| `src/query/mod.rs` | `query/mod.rs` | 17.1% | derived |
| `src/query/ra/filter.rs` | `query/ra.rs` | 64.5% | derived |
| `src/query/ra/inline_fixed.rs` | `query/ra.rs` | 69.8% | derived |
| `src/query/ra/join.rs` | `query/ra.rs` | 64.2% | derived |
| `src/query/ra/mod.rs` | `query/ra.rs` | 73.7% | derived |
| `src/query/ra/project.rs` | `query/ra.rs` | 64.4% | derived |
| `src/query/ra/search.rs` | `query/ra.rs` | 22.7% | derived |
| `src/query/ra/sort.rs` | `query/ra.rs` | 38.7% | derived |
| `src/query/ra/stored.rs` | `query/ra.rs` | 61.4% | derived |
| `src/query/ra/temp_store.rs` | `query/ra.rs` | 67.3% | derived |
| `src/query/reorder.rs` | `query/reorder.rs` | 72.3% | derived |
| `src/query/sort.rs` | `query/sort.rs` | 46.9% | derived |
| `src/query/stratify.rs` | `query/stratify.rs` | 69.7% | derived |
| `src/query/tests/mod.rs` | — | 0.0% | sovereign |
| `src/query/tests/reference_semantics.rs` | — | 0.0% | sovereign |
| `src/query_cache.rs` | — | 0.0% | sovereign |
| `src/runtime/callback.rs` | `runtime/callback.rs` | 55.3% | derived |
| `src/runtime/db.rs` | `runtime/db.rs` | 22.7% | derived |
| `src/runtime/error.rs` | — | 0.0% | sovereign |
| `src/runtime/exec.rs` | `runtime/db.rs` | 66.6% | derived |
| `src/runtime/hnsw/adaptive.rs` | `runtime/hnsw.rs` | 0.0% | derived |
| `src/runtime/hnsw/graph.rs` | `runtime/hnsw.rs` | 46.1% | derived |
| `src/runtime/hnsw/mod.rs` | `runtime/hnsw.rs` | 0.0% | derived |
| `src/runtime/hnsw/put.rs` | `runtime/hnsw.rs` | 27.5% | derived |
| `src/runtime/hnsw/remove.rs` | `runtime/hnsw.rs` | 49.3% | derived |
| `src/runtime/hnsw/search.rs` | `runtime/hnsw.rs` | 19.7% | derived |
| `src/runtime/hnsw/types.rs` | `runtime/hnsw.rs` | 11.9% | derived |
| `src/runtime/hnsw/visited_pool.rs` | `runtime/hnsw.rs` | 3.3% | derived |
| `src/runtime/hnsw_sovereign/adaptive.rs` | cf. `runtime/hnsw.rs` | 0.0% | sovereign |
| `src/runtime/hnsw_sovereign/close_reopen_tests.rs` | cf. `runtime/hnsw.rs` | 1.6% | sovereign |
| `src/runtime/hnsw_sovereign/graph.rs` | cf. `runtime/hnsw.rs` | 13.6% | sovereign |
| `src/runtime/hnsw_sovereign/mod.rs` | cf. `runtime/hnsw.rs` | 0.0% | sovereign |
| `src/runtime/hnsw_sovereign/put.rs` | cf. `runtime/hnsw.rs` | 9.6% | sovereign |
| `src/runtime/hnsw_sovereign/remove.rs` | cf. `runtime/hnsw.rs` | 8.3% | sovereign |
| `src/runtime/hnsw_sovereign/search.rs` | cf. `runtime/hnsw.rs` | 15.8% | sovereign |
| `src/runtime/hnsw_sovereign/types.rs` | cf. `runtime/hnsw.rs` | 3.4% | sovereign |
| `src/runtime/imperative.rs` | `runtime/imperative.rs` | 33.9% | derived |
| `src/runtime/minhash_lsh.rs` | `runtime/minhash_lsh.rs` | 53.0% | derived |
| `src/runtime/mod.rs` | `runtime/mod.rs` | 0.0% | derived |
| `src/runtime/query_context_impl.rs` | — | 0.0% | sovereign |
| `src/runtime/relation/extractors.rs` | `query/stored.rs` | 56.9% | derived |
| `src/runtime/relation/handles.rs` | `runtime/relation.rs` | 68.7% | derived |
| `src/runtime/relation/index_create.rs` | `runtime/relation.rs` | 61.3% | derived |
| `src/runtime/relation/index_management.rs` | `runtime/relation.rs` | 57.0% | derived |
| `src/runtime/relation/mod.rs` | `runtime/relation.rs` | 0.0% | derived |
| `src/runtime/relation/mutation.rs` | `query/stored.rs` | 63.7% | derived |
| `src/runtime/relation/relation_crud.rs` | `runtime/relation.rs` | 48.0% | derived |
| `src/runtime/relation/validation.rs` | `query/stored.rs` | 73.3% | derived |
| `src/runtime/sys.rs` | `runtime/db.rs` | 55.1% | derived |
| `src/runtime/temp_store.rs` | `runtime/temp_store.rs` | 81.0% | derived |
| `src/runtime/tests/basic_queries.rs` | `runtime/tests.rs` | 12.3% | derived |
| `src/runtime/tests/imperative.rs` | `runtime/tests.rs` | 44.4% | derived |
| `src/runtime/tests/indexing.rs` | `runtime/tests.rs` | 29.0% | derived |
| `src/runtime/tests/mod.rs` | `runtime/tests.rs` | 0.0% | derived |
| `src/runtime/tests/triggers_callbacks.rs` | `runtime/tests.rs` | 7.6% | derived |
| `src/runtime/transact.rs` | `runtime/transact.rs` | 11.4% | derived |
| `src/storage/error.rs` | — | 0.0% | sovereign |
| `src/storage/fjall_backend.rs` | — | 0.0% | sovereign |
| `src/storage/mem.rs` | cf. `storage/mem.rs` | 22.6% | sovereign |
| `src/storage/mod.rs` | `storage/mod.rs` | 39.3% | derived |
| `src/storage/temp.rs` | cf. `storage/temp.rs` | 33.2% | sovereign |
| `src/utils.rs` | `utils.rs` | 42.9% | derived |

Aletheia's own additions are real and sit alongside the derived files — `async_surface`, `counterfactual`, `hot_reload`, `query_cache`, `storage/fjall_backend`, the CSR PageRank path, `kcore`, RRF, the fixed-rule test suite, and `data/tests/proptest_memcmp` — all `sovereign` in the table above. They do not change the provenance of the derived files they extend.

## Reading `verbatim_pct`: what it can and cannot prove

`verbatim_pct` is evidence of textual overlap, not a verdict on origin. Two files that independently implement the same algorithm against the same crate vocabulary (`DataValue`, `BTreeMap`, the `FixedRule` trait, `poison.check()?`) converge on real line-for-line similarity that has nothing to do with copying — and at the file sizes in this crate, that convergence is large enough to overlap with an actual transliteration.

aletheia#6656 measured this directly against `fixed_rule/algos/*_native.rs` — every one nominally `sovereign` (`upstream_path = "none"`, no lineage claimed). Scored against the same-algorithm upstream file each was written to replace, verbatim_pct ranges 14.9% (`kruskal_native.rs`) to 32.1% (`degree_centrality_native.rs`); scored against an algorithm it has no relationship to at all, `bfs_native.rs` vs. `degree_centrality.rs` still measures 7.4% from shared idiom alone. `dfs_native.rs` — confirmed by that audit to be a statement-for-statement transliteration with renamed identifiers — measures 26.6% against its real source: inside the same band as files with no such finding, and lower than `degree_centrality_native.rs`'s 32.1%, which reads on manual inspection as an independent rewrite (different data structures, different variable names, an added citation to Freeman 1978) despite scoring higher. The metric alone cannot separate the two.

Treat any `verbatim_pct` figure — for a `derived`/`dual` row against its recorded `upstream_path`, or for an ad hoc comparison run against a `sovereign` row for review — as a triage signal that earns a manual read at any nontrivial value, never as proof of either originality or copying by itself, and never as a substitute for reading the file.

`scripts/check-krites-verbatim-drift.py` is a separate, purpose-built answer to this same gap — a token-shingle Jaccard metric that discards punctuation-only, `use`, and attribute lines before comparing, precisely so shared idiom stops reading as evidence. It runs report-only in CI today (not yet promoted to a gate; see its module docstring for the promotion criteria) and is the tool to reach for when `verbatim_pct` alone is not enough to settle a review.

## A second vendored source: stop word lists

`fts/tokenizer/stop_word_filter`'s word lists are not CozoDB's expression, even in the rows above marked `derived`/`dual` against a CozoDB `upstream_path`: they are the [stopwords-iso](https://github.com/stopwords-iso/stopwords-iso/) project's data (copyright Gene Diaz, MIT license), which CozoDB itself vendored rather than authored. Krites vendors the same corpus a second time — CozoDB is a sibling vendor here, not the copyright source. The `upstream_path` column names CozoDB because that is where this crate's copy was copied from mechanically, which is a real and correctly-tracked lineage fact for the `derived`/`dual` rows in that module; it does not make CozoDB the author of the word data, and does not substitute for the MIT notice that data separately requires. That notice — attribution plus the full license text — lives at `src/fts/tokenizer/stop_word_filter/sovereign/NOTICE.md` and [`LICENSE-MIT-stopwords-iso`](LICENSE-MIT-stopwords-iso), independent of this file and of this module's CozoDB-retirement status.

## What that requires

Under MPL §3.1 every file in this crate that is derived from `cozo-core`, **including our modifications to it**, stays governed by the MPL. That is file-level copyleft: it binds these files and reaches no further into aletheia.

Aletheia distributes the whole as a Larger Work under AGPL-3.0-or-later. MPL §3.3 permits exactly that, because CozoDB does not attach Exhibit B and so is not Incompatible With Secondary Licenses, and AGPL-3.0 is a Secondary License under §1.12. A recipient may therefore take the covered files under either license, at their option. The crate's `license` field records the combination.

## Why this notice exists

Upstream identifiers were renamed during the migration and no attribution was recorded, which left the crate carrying MPL-covered code with its notices removed — the one thing §3.1 does not permit, independent of which license the Larger Work ships under. Renaming symbols does not change authorship of the expression. This file restores the notice.

The related trap, since it is what produced the gap: `docs/HUBS.md` asks memory documentation to describe the current architecture as Krites/Datalog/Fjall rather than CozoDB. That is sound naming hygiene and it explicitly does not reach attribution. Provenance and licensing statements name CozoDB because they are claims about authorship, not about architecture.

## Anti-backsliding

`scripts/check-krites-provenance.py` runs in CI (wired into the repo's required `gate` check, not a side workflow) and fails the build if: any file under `crates/krites/src/` is missing from the ledger; this file drifts from what the ledger renders; the set of `derived` rows grows relative to the PR's base commit; a row's status skips the `derived` → `dual` → `sovereign` sequence; a `dual` → `sovereign` transition drops or rewrites the `replaced_upstream_path` it carried forward from that row's own `upstream_path`; a `sovereign` row with no retained predecessor (`replaced_upstream_path == 'none'`) carries a nonzero `verbatim_pct`; a `dual` row's soak window has expired against the current commit count on `main`; or — when the offline upstream snapshot is present — a `derived`/`dual` row's stored `verbatim_pct` no longer matches a fresh recomputation against `upstream_path`, **or a `sovereign` row's stored `verbatim_pct` no longer matches a fresh recomputation against its retained `replaced_upstream_path`**. That last clause is what makes a `sovereign` claim keep proving itself instead of being measured once and trusted forever — the original gap this file's own existence (see "Why this notice exists" above) was written to close, and that a transliterated file could still slip past a status flip that quietly zeroed its evidence (aletheia#6656). The status-sequence and sovereign/verbatim_pct checks together make a direct `derived` → `sovereign` jump structurally impossible, not merely discouraged: neither check alone stops a bypass that clears the other (flip status alone leaves verbatim_pct as evidence; zero the field too and the sequence check still requires a `dual` commit in between).

One more clause closes a gap the recompute check could not reach on its own (aletheia#6797): a `sovereign` row with `replaced_upstream_path == 'none'` had nothing for the recompute check to run against, and nothing verified that `'none'` meant "genuinely nothing to compare against" rather than "nobody mapped it yet" — the two looked identical, which is how the crate's highest-risk rewrite (`runtime/hnsw_sovereign/*`, 2912 lines) sat completely unmeasured while smaller rewrites beside it were all measured. CI now fails the build if any such row is absent from `krites_provenance_lib.py`'s `NO_PREDECESSOR_REASONS` — an explicit, individually-verified declaration of why the row genuinely has nothing to compare against — or if that map holds a stale entry for a row that no longer qualifies.
