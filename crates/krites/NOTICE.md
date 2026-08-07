# Third-party notice — krites

`krites` is substantially derived from **CozoDB** (`cozo-core`), copyright the CozoDB authors, licensed under the **Mozilla Public License 2.0**. A copy of that license sits beside this file at [LICENSE-MPL-2.0](LICENSE-MPL-2.0); upstream is <https://github.com/cozodb/cozo>.

## What is derived

This table is rendered from [`PROVENANCE.toml`](PROVENANCE.toml) — the file-level provenance ledger — never hand-edited. `verbatim_pct` is the share of each file's non-blank lines that a line-level diff (Python `difflib.SequenceMatcher`, order-sensitive) matches against the upstream file at the pinned commit; it is measured per file, not assumed from a subsystem average.

- Upstream: <https://github.com/cozodb/cozo>, pinned at `481af058abac9444ea8c9c52c78f096ed4b5bfc4`
- 210 files under `src/`: 177 derived, 24 sovereign, 9 dual
- Mean verbatim match across the 177 derived files: 49.7% (unweighted average of the per-file `verbatim_pct` column below)

| File | Upstream | Verbatim | Status |
|---|---|---:|---|
| `src/async_surface.rs` | — | 0.0% | sovereign |
| `src/counterfactual.rs` | — | 0.0% | sovereign |
| `src/counterfactual_tests.rs` | — | 0.0% | sovereign |
| `src/data/aggr/boolean.rs` | `data/aggr.rs` | 76.6% | derived |
| `src/data/aggr/misc.rs` | `data/aggr.rs` | 78.7% | derived |
| `src/data/aggr/mod.rs` | `data/aggr.rs` | 62.4% | derived |
| `src/data/aggr/numeric.rs` | `data/aggr.rs` | 62.2% | derived |
| `src/data/error.rs` | — | 0.0% | sovereign |
| `src/data/expr/expr_impl.rs` | `data/expr.rs` | 65.5% | derived |
| `src/data/expr/mod.rs` | `data/expr.rs` | 57.0% | derived |
| `src/data/expr/op.rs` | `data/expr.rs` | 87.5% | derived |
| `src/data/functions/aggregate.rs` | `data/functions.rs` | 15.1% | derived |
| `src/data/functions/bits.rs` | `data/functions.rs` | 52.1% | derived |
| `src/data/functions/collections.rs` | `data/functions.rs` | 23.0% | derived |
| `src/data/functions/math/arithmetic.rs` | `data/functions.rs` | 43.2% | derived |
| `src/data/functions/math/mod.rs` | `data/functions.rs` | 34.6% | derived |
| `src/data/functions/math/transcendental.rs` | `data/functions.rs` | 14.3% | derived |
| `src/data/functions/mod.rs` | `data/functions.rs` | 5.8% | derived |
| `src/data/functions/string.rs` | `data/functions.rs` | 37.0% | derived |
| `src/data/functions/temporal.rs` | `data/functions.rs` | 23.9% | derived |
| `src/data/functions/trig.rs` | `data/functions.rs` | 36.9% | derived |
| `src/data/functions/utility.rs` | `data/functions.rs` | 38.4% | derived |
| `src/data/functions/vector.rs` | `data/functions.rs` | 35.1% | derived |
| `src/data/json.rs` | `data/json.rs` | 53.7% | derived |
| `src/data/memcmp.rs` | `data/memcmp.rs` | 60.2% | derived |
| `src/data/mod.rs` | `data/mod.rs` | 39.1% | derived |
| `src/data/program/atoms.rs` | `data/program.rs` | 73.4% | derived |
| `src/data/program/fixed_rule.rs` | `data/program.rs` | 66.5% | derived |
| `src/data/program/input.rs` | `data/program.rs` | 75.2% | derived |
| `src/data/program/magic.rs` | `data/program.rs` | 84.6% | derived |
| `src/data/program/mod.rs` | `data/program.rs` | 0.0% | derived |
| `src/data/program/search/atom_impl.rs` | `data/program.rs` | 83.7% | derived |
| `src/data/program/search/hnsw_normalize.rs` | `data/program.rs` | 57.0% | derived |
| `src/data/program/search/lsh_fts.rs` | `data/program.rs` | 56.9% | derived |
| `src/data/program/search/mod.rs` | `data/program.rs` | 83.0% | derived |
| `src/data/program/types.rs` | `data/program.rs` | 82.7% | derived |
| `src/data/relation.rs` | `data/relation.rs` | 57.3% | derived |
| `src/data/symb.rs` | `data/symb.rs` | 71.1% | derived |
| `src/data/tests/aggrs.rs` | `data/tests/aggrs.rs` | 35.4% | derived |
| `src/data/tests/exprs.rs` | `data/tests/exprs.rs` | 51.9% | derived |
| `src/data/tests/functions/arithmetic.rs` | `data/tests/functions.rs` | 42.3% | derived |
| `src/data/tests/functions/collections.rs` | `data/tests/functions.rs` | 52.8% | derived |
| `src/data/tests/functions/mod.rs` | `data/tests/functions.rs` | 0.0% | derived |
| `src/data/tests/functions/string_ops.rs` | `data/tests/functions.rs` | 58.0% | derived |
| `src/data/tests/functions/type_conversion.rs` | `data/tests/functions.rs` | 35.6% | derived |
| `src/data/tests/json.rs` | `data/tests/json.rs` | 9.6% | derived |
| `src/data/tests/memcmp.rs` | `data/tests/memcmp.rs` | 56.6% | derived |
| `src/data/tests/mod.rs` | `data/tests/mod.rs` | 77.8% | derived |
| `src/data/tests/proptest_memcmp.rs` | — | 0.0% | sovereign |
| `src/data/tests/validity.rs` | `data/tests/validity.rs` | 65.0% | derived |
| `src/data/tests/values.rs` | `data/tests/values.rs` | 25.6% | derived |
| `src/data/tuple.rs` | `data/tuple.rs` | 55.9% | derived |
| `src/data/value.rs` | `data/value.rs` | 68.3% | derived |
| `src/datalog.pest` | `cozoscript.pest` | 99.6% | derived |
| `src/error.rs` | — | 0.0% | sovereign |
| `src/fixed_rule/algos/all_pairs_shortest_path.rs` | `fixed_rule/algos/all_pairs_shortest_path.rs` | 39.6% | derived |
| `src/fixed_rule/algos/astar.rs` | `fixed_rule/algos/astar.rs` | 49.1% | derived |
| `src/fixed_rule/algos/bfs.rs` | `fixed_rule/algos/bfs.rs` | 61.7% | derived |
| `src/fixed_rule/algos/degree_centrality.rs` | `fixed_rule/algos/degree_centrality.rs` | 55.3% | derived |
| `src/fixed_rule/algos/dfs.rs` | `fixed_rule/algos/dfs.rs` | 57.8% | derived |
| `src/fixed_rule/algos/kcore.rs` | — | 0.0% | sovereign |
| `src/fixed_rule/algos/kruskal.rs` | `fixed_rule/algos/kruskal.rs` | 45.3% | derived |
| `src/fixed_rule/algos/label_propagation.rs` | `fixed_rule/algos/label_propagation.rs` | 44.0% | derived |
| `src/fixed_rule/algos/louvain.rs` | `fixed_rule/algos/louvain.rs` | 41.4% | derived |
| `src/fixed_rule/algos/mod.rs` | `fixed_rule/algos/mod.rs` | 91.9% | derived |
| `src/fixed_rule/algos/pagerank.rs` | `fixed_rule/algos/pagerank.rs` | 45.7% | derived |
| `src/fixed_rule/algos/prim.rs` | `fixed_rule/algos/prim.rs` | 49.3% | derived |
| `src/fixed_rule/algos/random_walk.rs` | `fixed_rule/algos/random_walk.rs` | 40.8% | derived |
| `src/fixed_rule/algos/shortest_path_bfs.rs` | `fixed_rule/algos/shortest_path_bfs.rs` | 68.1% | derived |
| `src/fixed_rule/algos/shortest_path_dijkstra.rs` | `fixed_rule/algos/shortest_path_dijkstra.rs` | 56.4% | derived |
| `src/fixed_rule/algos/strongly_connected_components.rs` | `fixed_rule/algos/strongly_connected_components.rs` | 45.0% | derived |
| `src/fixed_rule/algos/top_sort.rs` | `fixed_rule/algos/top_sort.rs` | 44.1% | derived |
| `src/fixed_rule/algos/triangles.rs` | `fixed_rule/algos/triangles.rs` | 47.3% | derived |
| `src/fixed_rule/algos/yen.rs` | `fixed_rule/algos/yen.rs` | 56.1% | derived |
| `src/fixed_rule/csr/mod.rs` | — | 0.0% | sovereign |
| `src/fixed_rule/csr/page_rank.rs` | — | 0.0% | sovereign |
| `src/fixed_rule/error.rs` | — | 0.0% | sovereign |
| `src/fixed_rule/mod.rs` | `fixed_rule/mod.rs` | 63.9% | derived |
| `src/fixed_rule/tests/centrality_spanning.rs` | — | 0.0% | sovereign |
| `src/fixed_rule/tests/connectivity_misc.rs` | — | 0.0% | sovereign |
| `src/fixed_rule/tests/mod.rs` | — | 0.0% | sovereign |
| `src/fixed_rule/tests/path_algorithms.rs` | — | 0.0% | sovereign |
| `src/fixed_rule/tests/proptest_algos.rs` | — | 0.0% | sovereign |
| `src/fixed_rule/utilities/constant.rs` | `fixed_rule/utilities/constant.rs` | 39.9% | derived |
| `src/fixed_rule/utilities/mod.rs` | `fixed_rule/utilities/mod.rs` | 57.1% | derived |
| `src/fixed_rule/utilities/reorder_sort.rs` | `fixed_rule/utilities/reorder_sort.rs` | 70.0% | derived |
| `src/fixed_rule/utilities/rrf.rs` | — | 0.0% | sovereign |
| `src/fts/README.md` | `fts/README.md` | 100.0% | derived |
| `src/fts/ast.rs` | `fts/ast.rs` | 79.6% | derived |
| `src/fts/config.rs` | `fts/mod.rs` | 41.2% | derived |
| `src/fts/error.rs` | — | 0.0% | sovereign |
| `src/fts/indexing.rs` | `fts/indexing.rs` | 35.8% | derived |
| `src/fts/mod.rs` | `fts/mod.rs` | 47.3% | derived |
| `src/fts/tokenizer/alphanum_only.rs` | `fts/tokenizer/alphanum_only.rs` | 74.2% | derived |
| `src/fts/tokenizer/ascii_folding_filter/fold_table.rs` | `fts/tokenizer/ascii_folding_filter.rs` | 3.7% | derived |
| `src/fts/tokenizer/ascii_folding_filter/fold_table/fold_digits_symbols.rs` | `fts/tokenizer/ascii_folding_filter.rs` | 99.1% | derived |
| `src/fts/tokenizer/ascii_folding_filter/fold_table/fold_letters_a_m.rs` | `fts/tokenizer/ascii_folding_filter.rs` | 99.5% | derived |
| `src/fts/tokenizer/ascii_folding_filter/fold_table/fold_letters_n_z.rs` | `fts/tokenizer/ascii_folding_filter.rs` | 99.5% | derived |
| `src/fts/tokenizer/ascii_folding_filter/mod.rs` | `fts/tokenizer/ascii_folding_filter.rs` | 78.8% | derived |
| `src/fts/tokenizer/ascii_folding_filter/tests/foldings_a_i.rs` | `fts/tokenizer/ascii_folding_filter.rs` | 0.4% | derived |
| `src/fts/tokenizer/ascii_folding_filter/tests/foldings_j_s.rs` | `fts/tokenizer/ascii_folding_filter.rs` | 0.5% | derived |
| `src/fts/tokenizer/ascii_folding_filter/tests/foldings_num_sym.rs` | `fts/tokenizer/ascii_folding_filter.rs` | 0.4% | derived |
| `src/fts/tokenizer/ascii_folding_filter/tests/foldings_t_z.rs` | `fts/tokenizer/ascii_folding_filter.rs` | 0.7% | derived |
| `src/fts/tokenizer/ascii_folding_filter/tests/mod.rs` | `fts/tokenizer/ascii_folding_filter.rs` | 7.0% | derived |
| `src/fts/tokenizer/empty_tokenizer.rs` | `fts/tokenizer/empty_tokenizer.rs` | 88.6% | derived |
| `src/fts/tokenizer/lower_caser.rs` | `fts/tokenizer/lower_caser.rs` | 81.7% | derived |
| `src/fts/tokenizer/mod.rs` | `fts/tokenizer/mod.rs` | 68.4% | derived |
| `src/fts/tokenizer/ngram_tokenizer.rs` | `fts/tokenizer/ngram_tokenizer.rs` | 78.4% | derived |
| `src/fts/tokenizer/raw_tokenizer.rs` | `fts/tokenizer/raw_tokenizer.rs` | 83.1% | derived |
| `src/fts/tokenizer/remove_long.rs` | `fts/tokenizer/remove_long.rs` | 81.4% | derived |
| `src/fts/tokenizer/simple_tokenizer.rs` | `fts/tokenizer/simple_tokenizer.rs` | 80.3% | derived |
| `src/fts/tokenizer/split_compound_words.rs` | `fts/tokenizer/split_compound_words.rs` | 87.8% | derived |
| `src/fts/tokenizer/stemmer.rs` | `fts/tokenizer/stemmer.rs` | 89.0% | derived |
| `src/fts/tokenizer/stop_word_filter/gen_stopwords.py` | `fts/tokenizer/stop_word_filter/gen_stopwords.py` | 88.2% | derived |
| `src/fts/tokenizer/stop_word_filter/mod.rs` | `fts/tokenizer/stop_word_filter/mod.rs` | 71.8% | derived |
| `src/fts/tokenizer/stop_word_filter/stopwords/af_da.rs` | `fts/tokenizer/stop_word_filter/stopwords.rs` | 3.3% | derived |
| `src/fts/tokenizer/stop_word_filter/stopwords/el_ja.rs` | `fts/tokenizer/stop_word_filter/stopwords.rs` | 3.2% | derived |
| `src/fts/tokenizer/stop_word_filter/stopwords/ko_ro.rs` | `fts/tokenizer/stop_word_filter/stopwords.rs` | 3.7% | derived |
| `src/fts/tokenizer/stop_word_filter/stopwords/mod.rs` | `fts/tokenizer/stop_word_filter/stopwords.rs` | 0.0% | derived |
| `src/fts/tokenizer/stop_word_filter/stopwords/nl_de.rs` | `fts/tokenizer/stop_word_filter/stopwords.rs` | 2.5% | derived |
| `src/fts/tokenizer/stop_word_filter/stopwords/ru_ur.rs` | `fts/tokenizer/stop_word_filter/stopwords.rs` | 4.2% | derived |
| `src/fts/tokenizer/stop_word_filter/stopwords/vi_zu.rs` | `fts/tokenizer/stop_word_filter/stopwords.rs` | 4.6% | derived |
| `src/fts/tokenizer/tokenized_string.rs` | `fts/tokenizer/tokenized_string.rs` | 79.2% | derived |
| `src/fts/tokenizer/tokenizer_impl.rs` | `fts/tokenizer/tokenizer_impl.rs` | 81.9% | derived |
| `src/fts/tokenizer/whitespace_tokenizer.rs` | `fts/tokenizer/whitespace_tokenizer.rs` | 80.3% | derived |
| `src/hot_reload.rs` | — | 0.0% | sovereign |
| `src/lib.rs` | `lib.rs` | 10.2% | derived |
| `src/parse/error.rs` | — | 0.0% | sovereign |
| `src/parse/expr/bytecode.rs` | `parse/expr.rs` | 36.4% | derived |
| `src/parse/expr/mod.rs` | `parse/expr.rs` | 16.5% | derived |
| `src/parse/expr/strings.rs` | `parse/expr.rs` | 11.3% | derived |
| `src/parse/fts.rs` | `parse/fts.rs` | 28.8% | derived |
| `src/parse/imperative.rs` | `parse/imperative.rs` | 25.6% | derived |
| `src/parse/mod.rs` | `parse/mod.rs` | 38.3% | derived |
| `src/parse/query/atoms.rs` | `parse/query.rs` | 26.3% | derived |
| `src/parse/query/fixed_rules.rs` | `parse/query.rs` | 15.1% | derived |
| `src/parse/query/mod.rs` | `parse/query.rs` | 0.0% | derived |
| `src/parse/query/options.rs` | `parse/query.rs` | 23.2% | derived |
| `src/parse/query/program.rs` | `parse/query.rs` | 24.2% | derived |
| `src/parse/schema.rs` | `parse/schema.rs` | 43.2% | derived |
| `src/parse/sys/index.rs` | `parse/sys.rs` | 3.9% | derived |
| `src/parse/sys/mod.rs` | `parse/sys.rs` | 56.6% | derived |
| `src/parse/sys/parse.rs` | `parse/sys.rs` | 8.1% | derived |
| `src/query/compile.rs` | `query/compile.rs` | 84.8% | derived |
| `src/query/error.rs` | — | 0.0% | sovereign |
| `src/query/eval.rs` | `query/eval.rs` | 50.7% | derived |
| `src/query/graph.rs` | `query/graph.rs` | 63.9% | derived |
| `src/query/logical.rs` | `query/logical.rs` | 81.1% | derived |
| `src/query/magic.rs` | `query/magic.rs` | 80.5% | derived |
| `src/query/mod.rs` | `query/mod.rs` | 25.6% | derived |
| `src/query/ra/filter.rs` | `query/ra.rs` | 72.0% | derived |
| `src/query/ra/inline_fixed.rs` | `query/ra.rs` | 75.2% | derived |
| `src/query/ra/join.rs` | `query/ra.rs` | 69.8% | derived |
| `src/query/ra/mod.rs` | `query/ra.rs` | 78.6% | derived |
| `src/query/ra/project.rs` | `query/ra.rs` | 72.9% | derived |
| `src/query/ra/search.rs` | `query/ra.rs` | 26.4% | derived |
| `src/query/ra/sort.rs` | `query/ra.rs` | 53.3% | derived |
| `src/query/ra/stored.rs` | `query/ra.rs` | 67.0% | derived |
| `src/query/ra/temp_store.rs` | `query/ra.rs` | 62.3% | derived |
| `src/query/reorder.rs` | `query/reorder.rs` | 77.1% | derived |
| `src/query/sort.rs` | `query/sort.rs` | 70.6% | derived |
| `src/query/stored/extractors.rs` | `query/stored.rs` | 68.6% | derived |
| `src/query/stored/mod.rs` | `query/stored.rs` | 0.0% | derived |
| `src/query/stored/mutation.rs` | `query/stored.rs` | 71.6% | derived |
| `src/query/stored/validation.rs` | `query/stored.rs` | 73.5% | derived |
| `src/query/stratify.rs` | `query/stratify.rs` | 71.2% | derived |
| `src/query_cache.rs` | — | 0.0% | sovereign |
| `src/runtime/callback.rs` | `runtime/callback.rs` | 65.8% | derived |
| `src/runtime/db.rs` | `runtime/db.rs` | 45.5% | derived |
| `src/runtime/error.rs` | — | 0.0% | sovereign |
| `src/runtime/exec.rs` | `runtime/db.rs` | 77.8% | derived |
| `src/runtime/hnsw/adaptive.rs` | `runtime/hnsw.rs` | 3.3% | derived |
| `src/runtime/hnsw/graph.rs` | `runtime/hnsw.rs` | 50.0% | derived |
| `src/runtime/hnsw/mod.rs` | `runtime/hnsw.rs` | 2.8% | derived |
| `src/runtime/hnsw/put.rs` | `runtime/hnsw.rs` | 33.6% | derived |
| `src/runtime/hnsw/remove.rs` | `runtime/hnsw.rs` | 53.7% | derived |
| `src/runtime/hnsw/search.rs` | `runtime/hnsw.rs` | 26.4% | derived |
| `src/runtime/hnsw/types.rs` | `runtime/hnsw.rs` | 29.4% | derived |
| `src/runtime/hnsw/visited_pool.rs` | `runtime/hnsw.rs` | 10.8% | derived |
| `src/runtime/hnsw_sovereign/adaptive.rs` | `runtime/hnsw.rs` | 3.7% | dual |
| `src/runtime/hnsw_sovereign/close_reopen_tests.rs` | `runtime/hnsw.rs` | 6.1% | dual |
| `src/runtime/hnsw_sovereign/graph.rs` | `runtime/hnsw.rs` | 25.6% | dual |
| `src/runtime/hnsw_sovereign/mod.rs` | `runtime/hnsw.rs` | 1.9% | dual |
| `src/runtime/hnsw_sovereign/put.rs` | `runtime/hnsw.rs` | 12.4% | dual |
| `src/runtime/hnsw_sovereign/remove.rs` | `runtime/hnsw.rs` | 21.4% | dual |
| `src/runtime/hnsw_sovereign/search.rs` | `runtime/hnsw.rs` | 11.6% | dual |
| `src/runtime/hnsw_sovereign/types.rs` | `runtime/hnsw.rs` | 13.1% | dual |
| `src/runtime/hnsw_sovereign/visited_pool.rs` | `runtime/hnsw.rs` | 14.1% | dual |
| `src/runtime/imperative.rs` | `runtime/imperative.rs` | 44.2% | derived |
| `src/runtime/minhash_lsh.rs` | `runtime/minhash_lsh.rs` | 59.6% | derived |
| `src/runtime/mod.rs` | `runtime/mod.rs` | 3.1% | derived |
| `src/runtime/relation/handles.rs` | `runtime/relation.rs` | 76.8% | derived |
| `src/runtime/relation/index_create.rs` | `runtime/relation.rs` | 73.9% | derived |
| `src/runtime/relation/index_management.rs` | `runtime/relation.rs` | 66.7% | derived |
| `src/runtime/relation/mod.rs` | `runtime/relation.rs` | 0.0% | derived |
| `src/runtime/relation/relation_crud.rs` | `runtime/relation.rs` | 61.0% | derived |
| `src/runtime/sys.rs` | `runtime/db.rs` | 72.7% | derived |
| `src/runtime/temp_store.rs` | `runtime/temp_store.rs` | 83.3% | derived |
| `src/runtime/tests/basic_queries.rs` | `runtime/tests.rs` | 38.5% | derived |
| `src/runtime/tests/imperative.rs` | `runtime/tests.rs` | 58.8% | derived |
| `src/runtime/tests/indexing.rs` | `runtime/tests.rs` | 52.5% | derived |
| `src/runtime/tests/mod.rs` | `runtime/tests.rs` | 0.0% | derived |
| `src/runtime/tests/triggers_callbacks.rs` | `runtime/tests.rs` | 23.5% | derived |
| `src/runtime/transact.rs` | `runtime/transact.rs` | 16.2% | derived |
| `src/storage/error.rs` | — | 0.0% | sovereign |
| `src/storage/fjall_backend.rs` | — | 0.0% | sovereign |
| `src/storage/mem.rs` | `storage/mem.rs` | 69.4% | derived |
| `src/storage/mod.rs` | `storage/mod.rs` | 64.4% | derived |
| `src/storage/temp.rs` | `storage/temp.rs` | 78.1% | derived |
| `src/utils.rs` | `utils.rs` | 71.4% | derived |

Aletheia's own additions are real and sit alongside the derived files — `async_surface`, `counterfactual`, `hot_reload`, `query_cache`, `storage/fjall_backend`, the CSR PageRank path, `kcore`, RRF, the fixed-rule test suite, and `data/tests/proptest_memcmp` — all `sovereign` in the table above. They do not change the provenance of the derived files they extend.

## What that requires

Under MPL §3.1 every file in this crate that is derived from `cozo-core`, **including our modifications to it**, stays governed by the MPL. That is file-level copyleft: it binds these files and reaches no further into aletheia.

Aletheia distributes the whole as a Larger Work under AGPL-3.0-or-later. MPL §3.3 permits exactly that, because CozoDB does not attach Exhibit B and so is not Incompatible With Secondary Licenses, and AGPL-3.0 is a Secondary License under §1.12. A recipient may therefore take the covered files under either license, at their option. The crate's `license` field records the combination.

## Why this notice exists

Upstream identifiers were renamed during the migration and no attribution was recorded, which left the crate carrying MPL-covered code with its notices removed — the one thing §3.1 does not permit, independent of which license the Larger Work ships under. Renaming symbols does not change authorship of the expression. This file restores the notice.

The related trap, since it is what produced the gap: `docs/HUBS.md` asks memory documentation to describe the current architecture as Krites/Datalog/Fjall rather than CozoDB. That is sound naming hygiene and it explicitly does not reach attribution. Provenance and licensing statements name CozoDB because they are claims about authorship, not about architecture.

## Anti-backsliding

`scripts/check-krites-provenance.py` runs in CI (wired into the repo's required `gate` check, not a side workflow) and fails the build if: any file under `crates/krites/src/` is missing from the ledger; this file drifts from what the ledger renders; the set of `derived` rows grows relative to the PR's base commit; a row's status skips the `derived` → `dual` → `sovereign` sequence; a `sovereign` row carries a nonzero `verbatim_pct`; a `dual` row's soak window has expired against the current commit count on `main`; or — when the offline upstream snapshot is present — a `derived` row's stored `verbatim_pct` no longer matches a fresh recomputation. The status-sequence and sovereign/verbatim_pct checks together make a direct `derived` → `sovereign` jump structurally impossible, not merely discouraged: neither check alone stops a bypass that clears the other (flip status alone leaves verbatim_pct as evidence; zero the field too and the sequence check still requires a `dual` commit in between).
