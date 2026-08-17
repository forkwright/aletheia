#!/usr/bin/env python3
"""Fetch pinned upstream cozo-core sources and (re)generate PROVENANCE.toml + NOTICE.md."""

from __future__ import annotations

import pathlib
import sys
import tomllib
import urllib.error
import urllib.request

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

from krites_provenance_lib import (  # noqa: E402
    KRITES_SRC,
    LEDGER_PATH,
    NOTICE_PATH,
    UPSTREAM_SNAPSHOT_DIR,
    dump_ledger,
    iter_src_files,
    render_notice,
    verbatim_pct,
)

UPSTREAM_REPO = "https://github.com/cozodb/cozo"
# INVARIANT: pin an exact commit, never a moving ref (main/HEAD) — the ledger's
# verbatim_pct must be reproducible from this file alone.
UPSTREAM_REF = "481af058abac9444ea8c9c52c78f096ed4b5bfc4"
RAW_BASE = f"https://raw.githubusercontent.com/cozodb/cozo/{UPSTREAM_REF}/cozo-core/src"

# Explicit, individually-verified path map: local (relative to crates/krites/src/)
# -> upstream cozo-core/src/ path, or None for sovereign (no upstream lineage).
#
# WARNING: do not derive this by directory-name pattern matching alone — several
# directories look like splits but are not (fixed_rule/algos/kcore.rs has no
# upstream counterpart despite fixed_rule/algos/degree_centrality.rs sitting beside
# it; fixed_rule/csr/page_rank.rs is a from-scratch CSR implementation, not a split
# of algos/pagerank.rs). Every grouping below was verified by cross-referencing
# public fn/struct/enum names between the local fragment set and the upstream file
# it is claimed to split.
UPSTREAM_MAP: dict[str, str | None] = {
    "async_surface.rs": None,
    "counterfactual.rs": None,
    "counterfactual_tests.rs": None,
    "data/aggr/boolean.rs": "data/aggr.rs",
    "data/aggr/misc.rs": "data/aggr.rs",
    "data/aggr/mod.rs": "data/aggr.rs",
    "data/aggr/numeric.rs": "data/aggr.rs",
    "data/error.rs": None,
    "data/expr/expr_impl.rs": "data/expr.rs",
    "data/expr/mod.rs": "data/expr.rs",
    "data/expr/op.rs": "data/expr.rs",
    "data/functions/aggregate.rs": "data/functions.rs",
    "data/functions/bits.rs": "data/functions.rs",
    "data/functions/collections.rs": "data/functions.rs",
    "data/functions/math/arithmetic.rs": "data/functions.rs",
    "data/functions/math/mod.rs": "data/functions.rs",
    "data/functions/math/transcendental.rs": "data/functions.rs",
    "data/functions/mod.rs": "data/functions.rs",
    "data/functions/string.rs": "data/functions.rs",
    "data/functions/temporal.rs": "data/functions.rs",
    "data/functions/trig.rs": "data/functions.rs",
    "data/functions/utility.rs": "data/functions.rs",
    "data/functions/vector.rs": "data/functions.rs",
    "data/json.rs": "data/json.rs",
    "data/memcmp.rs": "data/memcmp.rs",
    "data/mod.rs": "data/mod.rs",
    "data/program/atoms.rs": "data/program.rs",
    "data/program/fixed_rule.rs": "data/program.rs",
    "data/program/input.rs": "data/program.rs",
    "data/program/magic.rs": "data/program.rs",
    "data/program/mod.rs": "data/program.rs",
    "data/program/search/atom_impl.rs": "data/program.rs",
    "data/program/search/hnsw_normalize.rs": "data/program.rs",
    "data/program/search/lsh_fts.rs": "data/program.rs",
    "data/program/search/mod.rs": "data/program.rs",
    "data/program/types.rs": "data/program.rs",
    "data/relation.rs": "data/relation.rs",
    "data/symb.rs": "data/symb.rs",
    "data/tests/aggrs.rs": "data/tests/aggrs.rs",
    "data/tests/exprs.rs": "data/tests/exprs.rs",
    "data/tests/functions/arithmetic.rs": "data/tests/functions.rs",
    "data/tests/functions/collections.rs": "data/tests/functions.rs",
    "data/tests/functions/mod.rs": "data/tests/functions.rs",
    "data/tests/functions/string_ops.rs": "data/tests/functions.rs",
    "data/tests/functions/type_conversion.rs": "data/tests/functions.rs",
    "data/tests/functions/validity_units.rs": None,
    "data/tests/json.rs": "data/tests/json.rs",
    "data/tests/memcmp.rs": "data/tests/memcmp.rs",
    "data/tests/mod.rs": "data/tests/mod.rs",
    "data/tests/proptest_memcmp.rs": None,
    "data/tests/validity.rs": "data/tests/validity.rs",
    "data/tests/values.rs": "data/tests/values.rs",
    "data/tuple.rs": "data/tuple.rs",
    "data/value.rs": "data/value.rs",
    "datalog.pest": "cozoscript.pest",
    "error.rs": None,
    "fixed_rule/algos/all_pairs_shortest_path.rs": None,
    "fixed_rule/algos/astar.rs": None,
    "fixed_rule/algos/bfs.rs": None,
    "fixed_rule/algos/degree_centrality.rs": None,
    "fixed_rule/algos/dfs.rs": None,
    "fixed_rule/algos/kcore.rs": None,
    "fixed_rule/algos/kruskal.rs": None,
    "fixed_rule/algos/label_propagation.rs": None,
    "fixed_rule/algos/louvain.rs": "fixed_rule/algos/louvain.rs",
    "fixed_rule/algos/mod.rs": "fixed_rule/algos/mod.rs",
    "fixed_rule/algos/pagerank.rs": "fixed_rule/algos/pagerank.rs",
    "fixed_rule/algos/prim.rs": None,
    "fixed_rule/algos/random_walk.rs": None,
    "fixed_rule/algos/shortest_path_bfs.rs": None,
    "fixed_rule/algos/shortest_path_dijkstra.rs": None,
    "fixed_rule/algos/strongly_connected_components.rs": None,
    "fixed_rule/algos/top_sort.rs": None,
    "fixed_rule/algos/triangles.rs": None,
    "fixed_rule/algos/yen.rs": None,
    "fixed_rule/csr/mod.rs": None,
    "fixed_rule/csr/page_rank.rs": None,
    "fixed_rule/error.rs": None,
    "fixed_rule/mod.rs": "fixed_rule/mod.rs",
    "fixed_rule/tests/centrality_spanning.rs": None,
    "fixed_rule/tests/connectivity_misc.rs": None,
    "fixed_rule/tests/mod.rs": None,
    "fixed_rule/tests/path_algorithms.rs": None,
    "fixed_rule/tests/proptest_algos.rs": None,
    "fixed_rule/tests/wave5_reference_semantics.rs": None,
    "fixed_rule/utilities/constant.rs": None,
    "fixed_rule/utilities/mod.rs": "fixed_rule/utilities/mod.rs",
    "fixed_rule/utilities/reorder_sort.rs": None,
    "fixed_rule/utilities/rrf.rs": None,
    "fts/README.md": "fts/README.md",
    "fts/ast.rs": "fts/ast.rs",
    "fts/config.rs": "fts/mod.rs",
    "fts/error.rs": None,
    "fts/indexing.rs": "fts/indexing.rs",
    "fts/mod.rs": "fts/mod.rs",
    "fts/tokenizer/alphanum_only.rs": "fts/tokenizer/alphanum_only.rs",
    "fts/tokenizer/ascii_folding_filter/fold_table.rs": "fts/tokenizer/ascii_folding_filter.rs",
    # wave2a/ascii-folding-table: regenerated from UCD + CLDR Latin-ASCII, not
    # transcribed from cozo-core -- no upstream lineage.
    "fts/tokenizer/ascii_folding_filter/fold_table/fold_table_sovereign/generate.py": None,
    "fts/tokenizer/ascii_folding_filter/fold_table/fold_table_sovereign/mod.rs": None,
    "fts/tokenizer/ascii_folding_filter/fold_table/fold_table_sovereign/table.rs": None,
    "fts/tokenizer/ascii_folding_filter/mod.rs": "fts/tokenizer/ascii_folding_filter.rs",
    # wave2a/ascii-folding-table: the full-BMP-sweep conformance test proving
    # fold_table_sovereign/ equivalent to fold_table/ -- no upstream lineage.
    "fts/tokenizer/ascii_folding_filter/tests/foldings_a_i.rs": "fts/tokenizer/ascii_folding_filter.rs",
    "fts/tokenizer/ascii_folding_filter/tests/foldings_j_s.rs": "fts/tokenizer/ascii_folding_filter.rs",
    "fts/tokenizer/ascii_folding_filter/tests/foldings_num_sym.rs": "fts/tokenizer/ascii_folding_filter.rs",
    "fts/tokenizer/ascii_folding_filter/tests/foldings_t_z.rs": "fts/tokenizer/ascii_folding_filter.rs",
    "fts/tokenizer/ascii_folding_filter/tests/mod.rs": "fts/tokenizer/ascii_folding_filter.rs",
    "fts/tokenizer/empty_tokenizer.rs": "fts/tokenizer/empty_tokenizer.rs",
    "fts/tokenizer/lower_caser.rs": "fts/tokenizer/lower_caser.rs",
    "fts/tokenizer/mod.rs": "fts/tokenizer/mod.rs",
    "fts/tokenizer/ngram_tokenizer.rs": "fts/tokenizer/ngram_tokenizer.rs",
    "fts/tokenizer/raw_tokenizer.rs": "fts/tokenizer/raw_tokenizer.rs",
    "fts/tokenizer/remove_long.rs": "fts/tokenizer/remove_long.rs",
    "fts/tokenizer/simple_tokenizer.rs": "fts/tokenizer/simple_tokenizer.rs",
    "fts/tokenizer/split_compound_words.rs": "fts/tokenizer/split_compound_words.rs",
    "fts/tokenizer/stemmer.rs": "fts/tokenizer/stemmer.rs",
    # wave2b/stopword-lists land-dark: the CozoDB-lineage copy moved to
    # derived/ unchanged (still tracks the same upstream cozo paths, now
    # status=dual — see krites_provenance_lib.py's status-preservation logic
    # below); sovereign/ is the freshly authored replacement (sovereign, no
    # upstream). mod.rs ITSELF kept its path (Rust module resolution
    # requires `stop_word_filter/mod.rs` to exist) but its content became a
    # cfg dispatcher — freshly authored, yet the PATH carries derived
    # lineage on origin/main, so check_status_sequence correctly refuses a
    # direct derived -> sovereign jump here too: this row rides the same
    # dual soak as its derived/ siblings and graduates with them.
    "fts/tokenizer/stop_word_filter/mod.rs": "fts/tokenizer/stop_word_filter/mod.rs",
    "fts/tokenizer/stop_word_filter/sovereign/NOTICE.md": None,
    "fts/tokenizer/stop_word_filter/sovereign/gen_stopwords.py": None,
    "fts/tokenizer/stop_word_filter/sovereign/mod.rs": None,
    "fts/tokenizer/stop_word_filter/sovereign/stopwords.rs": None,
    "fts/tokenizer/tokenized_string.rs": "fts/tokenizer/tokenized_string.rs",
    "fts/tokenizer/tokenizer_impl.rs": "fts/tokenizer/tokenizer_impl.rs",
    "fts/tokenizer/whitespace_tokenizer.rs": "fts/tokenizer/whitespace_tokenizer.rs",
    "hot_reload.rs": None,
    "lib.rs": "lib.rs",
    "parse/error.rs": None,
    "parse/expr/bytecode.rs": "parse/expr.rs",
    "parse/expr/mod.rs": "parse/expr.rs",
    "parse/expr/strings.rs": "parse/expr.rs",
    "parse/fts.rs": "parse/fts.rs",
    "parse/imperative.rs": "parse/imperative.rs",
    "parse/mod.rs": "parse/mod.rs",
    "parse/query/atoms.rs": "parse/query.rs",
    "parse/query/fixed_rules.rs": "parse/query.rs",
    "parse/query/mod.rs": "parse/query.rs",
    "parse/query/options.rs": "parse/query.rs",
    "parse/query/program.rs": "parse/query.rs",
    "parse/schema.rs": "parse/schema.rs",
    "parse/sys/index.rs": "parse/sys.rs",
    "parse/sys/mod.rs": "parse/sys.rs",
    "parse/sys/parse.rs": "parse/sys.rs",
    "query/compile.rs": "query/compile.rs",
    "query/context.rs": None,
    "query/error.rs": None,
    "query/eval.rs": "query/eval.rs",
    "query/graph.rs": "query/graph.rs",
    "query/logical.rs": "query/logical.rs",
    "query/magic.rs": "query/magic.rs",
    "query/mod.rs": "query/mod.rs",
    "query/ra/filter.rs": "query/ra.rs",
    "query/ra/inline_fixed.rs": "query/ra.rs",
    "query/ra/join.rs": "query/ra.rs",
    "query/ra/mod.rs": "query/ra.rs",
    "query/ra/project.rs": "query/ra.rs",
    "query/ra/search.rs": "query/ra.rs",
    "query/ra/sort.rs": "query/ra.rs",
    "query/ra/stored.rs": "query/ra.rs",
    "query/ra/temp_store.rs": "query/ra.rs",
    "query/reorder.rs": "query/reorder.rs",
    "query/sort.rs": "query/sort.rs",
    "query/stratify.rs": "query/stratify.rs",
    "query/tests/mod.rs": None,
    "query/tests/reference_semantics.rs": None,
    "query_cache.rs": None,
    "runtime/callback.rs": "runtime/callback.rs",
    "runtime/db.rs": "runtime/db.rs",
    "runtime/error.rs": None,
    "runtime/exec.rs": "runtime/db.rs",
    "runtime/hnsw/adaptive.rs": "runtime/hnsw.rs",
    "runtime/hnsw/graph.rs": "runtime/hnsw.rs",
    "runtime/hnsw/mod.rs": "runtime/hnsw.rs",
    "runtime/hnsw/put.rs": "runtime/hnsw.rs",
    "runtime/hnsw/remove.rs": "runtime/hnsw.rs",
    "runtime/hnsw/search.rs": "runtime/hnsw.rs",
    "runtime/hnsw/types.rs": "runtime/hnsw.rs",
    "runtime/hnsw/visited_pool.rs": "runtime/hnsw.rs",
    "runtime/imperative.rs": "runtime/imperative.rs",
    "runtime/minhash_lsh.rs": "runtime/minhash_lsh.rs",
    "runtime/query_context_impl.rs": None,
    "runtime/mod.rs": "runtime/mod.rs",
    "runtime/relation/extractors.rs": "query/stored.rs",
    "runtime/relation/handles.rs": "runtime/relation.rs",
    "runtime/relation/index_create.rs": "runtime/relation.rs",
    "runtime/relation/index_management.rs": "runtime/relation.rs",
    "runtime/relation/mod.rs": "runtime/relation.rs",
    "runtime/relation/mutation.rs": "query/stored.rs",
    "runtime/relation/relation_crud.rs": "runtime/relation.rs",
    "runtime/relation/validation.rs": "query/stored.rs",
    "runtime/sys.rs": "runtime/db.rs",
    "runtime/temp_store.rs": "runtime/temp_store.rs",
    "runtime/tests/basic_queries.rs": "runtime/tests.rs",
    "runtime/tests/imperative.rs": "runtime/tests.rs",
    "runtime/tests/indexing.rs": "runtime/tests.rs",
    "runtime/tests/mod.rs": "runtime/tests.rs",
    "runtime/tests/triggers_callbacks.rs": "runtime/tests.rs",
    "runtime/transact.rs": "runtime/transact.rs",
    "storage/error.rs": None,
    "storage/fjall_backend.rs": None,
    "storage/mem.rs": "storage/mem.rs",
    "storage/mod.rs": "storage/mod.rs",
    # The fresh HNSW reimplementation. None declares no CozoDB ancestor; the file each
    # one replaces is retained in SOVEREIGN_VERIFY_MAP so the row is still measured.
    "runtime/hnsw_sovereign/adaptive.rs": None,
    "runtime/hnsw_sovereign/close_reopen_tests.rs": None,
    "runtime/hnsw_sovereign/graph.rs": None,
    "runtime/hnsw_sovereign/mod.rs": None,
    "runtime/hnsw_sovereign/put.rs": None,
    "runtime/hnsw_sovereign/remove.rs": None,
    "runtime/hnsw_sovereign/search.rs": None,
    "runtime/hnsw_sovereign/types.rs": None,
    # WHY this row carries a real predecessor and a high figure: the file is
    # vendored stopwords-iso data (MIT), and its own header records that the
    # word content is unchanged from the copy it replaced -- token-multiset
    # identical, 21,707 literals across 58 languages. A stop-word list cannot
    # be rewritten to be more original without being wrong. Recording the
    # predecessor makes the row state that; asserting none, as it did, claimed
    # there was nothing to measure against a file it is 94% identical to.
    "fts/tokenizer/stop_word_filter/sovereign/stopwords.rs": "fts/tokenizer/stop_word_filter/stopwords.rs",
    "storage/temp.rs": "storage/temp.rs",
    "utils.rs": "utils.rs",
}

# PLAN.md Sec.2 land-dark/soak/delete: paths still in UPSTREAM_MAP with a real
# upstream_path (so they measure as "derived" by the branch below) but that
# now have a sovereign replacement compiled in beside them, selected by a
# compile-time cfg. Maps path -> soak_expires_at_commit_count (an ABSOLUTE
# `git rev-list --count origin/main` target, per the ledger header note).
# check-krites-provenance.py's check_soak_expiry fails the build once main
# reaches that count without the pair having flipped to sovereign (dropping
# the derived file) or the window being extended by an explicit ledger edit.
# Explicit, individually-verified map (same discipline as UPSTREAM_MAP above,
# for the same reason): a local path whose UPSTREAM_MAP entry is None (no MPL
# lineage, ever) but that is nonetheless a from-scratch REPLACEMENT for a
# specific derived/dual file — a from-scratch rewrite that took over the
# replaced file's name, or a `*_sovereign/` directory swapped in via cfg
# beside its dual sibling (PLAN.md §2's land-dark pattern) — rather than a
# wholly independent addition. The value
# is the SAME upstream path the replaced file already carries in
# UPSTREAM_MAP; it becomes the generated row's replaced_upstream_path (never
# upstream_path — no lineage claim), and the row is measured against it for
# real instead of being hardcoded to verbatim_pct=0.0.
#
# A path with no natural predecessor at all (kcore.rs, hot_reload.rs,
# query_cache.rs, storage/fjall_backend.rs, async_surface.rs, ...) is
# correctly absent from this map — there is nothing to measure it against,
# and verbatim_pct stays genuinely 0.0.
#
# WHY this map exists: before it did, every UPSTREAM_MAP=None row — this
# includes every one of the 17 fixed-rule rewrites below — was hardcoded to
# verbatim_pct=0.0/upstream_path='none' unconditionally, regardless of how
# similar the file actually was to what it replaced. That is how a
# statement-for-statement transliteration entered the ledger measuring
# 18.0%-41.4% against the upstream file it replaced, certified at 0.0%
# because nothing ever ran the comparison (aletheia#6656).
#
# WARNING: when a new land-dark wave adds a `*_sovereign/` directory (e.g.
# `runtime/hnsw_sovereign/*.rs`, land-dark beside `runtime/hnsw/*.rs` against
# the same upstream `runtime/hnsw.rs`), add its entries here explicitly — do
# not derive them by directory-name pattern matching, for the same reason
# UPSTREAM_MAP's own warning above gives (a `*_sovereign` name does not by
# itself prove which upstream file, if any, is the right comparison). The
# `_native.rs` filename convention this map was written against is retired:
# once a derived file is deleted, its replacement takes the plain name, so
# nothing distinguishes a replacement from an original by filename alone —
# which is exactly why the mapping is explicit rather than inferred. A row that instead lands directly as
# `dual` first (real upstream_path in UPSTREAM_MAP, measured and soaking
# under CI the whole time) does not need an entry here at all: its
# replaced_upstream_path is set automatically when it later transitions to
# `sovereign` via krites-provenance-transition.py, which carries its
# dual-era upstream_path forward unchanged.
SOVEREIGN_VERIFY_MAP: dict[str, str] = {
    "fixed_rule/algos/all_pairs_shortest_path.rs": "fixed_rule/algos/all_pairs_shortest_path.rs",
    "fixed_rule/algos/astar.rs": "fixed_rule/algos/astar.rs",
    "fixed_rule/algos/bfs.rs": "fixed_rule/algos/bfs.rs",
    "fixed_rule/algos/degree_centrality.rs": "fixed_rule/algos/degree_centrality.rs",
    "fixed_rule/algos/dfs.rs": "fixed_rule/algos/dfs.rs",
    "fixed_rule/algos/kruskal.rs": "fixed_rule/algos/kruskal.rs",
    "fixed_rule/algos/label_propagation.rs": "fixed_rule/algos/label_propagation.rs",
    "fixed_rule/algos/prim.rs": "fixed_rule/algos/prim.rs",
    "fixed_rule/algos/random_walk.rs": "fixed_rule/algos/random_walk.rs",
    "fixed_rule/algos/shortest_path_bfs.rs": "fixed_rule/algos/shortest_path_bfs.rs",
    "fixed_rule/algos/shortest_path_dijkstra.rs": "fixed_rule/algos/shortest_path_dijkstra.rs",
    "fixed_rule/algos/strongly_connected_components.rs": "fixed_rule/algos/strongly_connected_components.rs",
    "fixed_rule/algos/top_sort.rs": "fixed_rule/algos/top_sort.rs",
    "fixed_rule/algos/triangles.rs": "fixed_rule/algos/triangles.rs",
    "fixed_rule/algos/yen.rs": "fixed_rule/algos/yen.rs",
    "fixed_rule/utilities/constant.rs": "fixed_rule/utilities/constant.rs",
    "fixed_rule/utilities/reorder_sort.rs": "fixed_rule/utilities/reorder_sort.rs",
    # WHY these eight belong here: every `*_native.rs` rewrite above records a
    # predecessor and is measured against it, while the largest and highest-risk
    # rewrite in the program recorded `replaced_upstream_path = "none"` and was
    # therefore measured against nothing -- check_verbatim_recompute skips rows
    # with no predecessor by construction. That made the one tree that ships to
    # production when `krites_sovereign_hnsw` flips the only tree with no
    # measurement at all.
    #
    # Each file's predecessor is the upstream path its derived sibling under
    # `runtime/hnsw/` already carries: upstream keeps HNSW in one file, which
    # krites split. close_reopen_tests.rs is included deliberately even though
    # it has no upstream analogue -- it asserts the behaviour of a rewrite that
    # is byte-compatible with upstream's encoding, and a row asserting "nothing
    # to compare against" is precisely the shape that evades the gate.
    "runtime/hnsw_sovereign/adaptive.rs": "runtime/hnsw.rs",
    "runtime/hnsw_sovereign/close_reopen_tests.rs": "runtime/hnsw.rs",
    "runtime/hnsw_sovereign/graph.rs": "runtime/hnsw.rs",
    "runtime/hnsw_sovereign/mod.rs": "runtime/hnsw.rs",
    "runtime/hnsw_sovereign/put.rs": "runtime/hnsw.rs",
    "runtime/hnsw_sovereign/remove.rs": "runtime/hnsw.rs",
    "runtime/hnsw_sovereign/search.rs": "runtime/hnsw.rs",
    "runtime/hnsw_sovereign/types.rs": "runtime/hnsw.rs",
    # #6797: the same audit that closed aletheia#6656 for hnsw_sovereign found two
    # more sovereign-with-none rows that DO have a real predecessor, once judged by
    # content rather than path shape.
    #
    # fold_table_sovereign/{mod.rs,table.rs}: fold_table.rs (this crate's own row,
    # already sovereign) already carries replaced_upstream_path =
    # "fts/tokenizer/ascii_folding_filter.rs" -- it is now a ten-line shim that
    # delegates to fold_table_sovereign, exactly the "upstream keeps it in one file,
    # krites split it further" shape the hnsw_sovereign group above documents.
    # mod.rs's own fold_non_ascii_char(c: char) -> Option<&'static str> is the same
    # function name and signature upstream's file implements inline; table.rs holds
    # the data that function looks up. generate.py (the UCD/CLDR codegen tool that
    # produced table.rs) is deliberately absent here -- unlike stop_word_filter's
    # gen_stopwords.py below, cozo-core's fold table was hand-authored with no
    # generator script of any kind, so there is nothing for a Python file to be
    # measured against.
    "fts/tokenizer/ascii_folding_filter/fold_table/fold_table_sovereign/mod.rs": "fts/tokenizer/ascii_folding_filter.rs",
    "fts/tokenizer/ascii_folding_filter/fold_table/fold_table_sovereign/table.rs": "fts/tokenizer/ascii_folding_filter.rs",
    # stop_word_filter/sovereign/mod.rs: a line-for-line reproduction of upstream's
    # own stop_word_filter/mod.rs -- same StopWordFilter struct, same for_lang match
    # arms (only reordered), same TokenFilter/TokenStream impl shape. Measures 15.5%,
    # squarely inside the aletheia#6656 review-worthy band (14.9%-32.1% across the
    # fixed_rule/algos/*_native.rs rewrites) -- exactly the sovereign-with-nothing-
    # to-compare-against shape this map exists to close.
    "fts/tokenizer/stop_word_filter/sovereign/mod.rs": "fts/tokenizer/stop_word_filter/mod.rs",
    # stop_word_filter/sovereign/gen_stopwords.py: cozo-core has its own
    # gen_stopwords.py at the same relative path, doing the same job (emit the Rust
    # stopword-list source from external data) -- a real, named predecessor, unlike
    # fold_table_sovereign/generate.py above, which has none. This crate's own
    # docstring says as much: it names itself the successor to "the sibling
    # `derived/gen_stopwords.py`" (the CozoDB-lineage copy, since retired) which was
    # itself a copy of this same upstream file.
    "fts/tokenizer/stop_word_filter/sovereign/gen_stopwords.py": "fts/tokenizer/stop_word_filter/gen_stopwords.py",
}

DUAL_SOAK_WINDOW: dict[str, int] = {
    # wave2a/ascii-folding-table: land-dark PR lands at commit count 2808.
    # +30 commits is PLAN.md's own Q3 recommended window for low-blast-radius
    # waves (2a, 2b, 5, 7) -- this is a LOW-risk, pure-data wave with a
    # full-BMP-sweep conformance gate already proving equivalence
    # (tests/bmp_equivalence.rs), so there is no soak-observation need beyond
    # CI turning green on the sovereign feature.
    "fts/tokenizer/ascii_folding_filter/fold_table.rs": 2838,
}

_upstream_cache: dict[str, str] = {}


def fetch_upstream(path: str) -> str:
    # WHY(P6): prefer the offline vendored snapshot (wave0/drift-metric,
    # crates/krites/upstream-snapshot/cozo-core-src/) when present, so
    # regeneration — and CI's check_verbatim_recompute — work without a
    # network fetch and without trusting a raw.githubusercontent.com read
    # done once and never re-verified. Falls back to the network fetch when
    # the snapshot hasn't landed yet; both paths are pinned to the same
    # UPSTREAM_REF, so the value is identical either way.
    if path not in _upstream_cache:
        snapshot_file = UPSTREAM_SNAPSHOT_DIR / path
        if snapshot_file.is_file():
            _upstream_cache[path] = snapshot_file.read_text(errors="replace")
        else:
            url = f"{RAW_BASE}/{path}"
            try:
                with urllib.request.urlopen(url, timeout=20) as resp:  # noqa: S310
                    _upstream_cache[path] = resp.read().decode("utf-8")
            except urllib.error.HTTPError as exc:
                raise SystemExit(f"fetch failed ({exc.code}): {url}") from exc
    return _upstream_cache[path]


def unparsable_ledger_message(path: pathlib.Path, exc: Exception) -> str:
    """WHY an unparsable prior ledger is fatal rather than 'no preservation'.

    Both readers below take two fields off the previous ledger, and both used to
    treat a TOML parse error as "nothing to preserve" -- the same best-effort
    shape that is correct for a MISSING file on the ledger's first-ever run. The
    two cases are not the same. A missing file means there is no prior state; an
    unparsable one means there IS prior state and we cannot read it, and every
    dual/sovereign row then gets recomputed from scratch as `derived` with its
    soak window zeroed.

    That is reachable by ordinary work, not by tampering: a merge conflict in
    PROVENANCE.toml leaves conflict markers in the file, and regenerating in that
    state silently demoted 5 sovereign rows and 1 dual row in one run. Only
    check_status_sequence caught it afterwards, by rejecting sovereign ->
    derived; the regenerator itself reported a normal write. Conflicts in this
    file are routine in a program whose whole shape is moving and rewriting
    files, so this fires often rather than never.
    """
    return (
        f"{path} could not be parsed, so no prior status could be preserved: {exc}\n"
        "Regenerating now would recompute every dual/sovereign row as 'derived' "
        "and zero its soak window. If this is a merge conflict, resolve the "
        "ledger first (take one side wholesale -- it is regenerated immediately "
        "after) and re-run; the derived artifacts are recomputed, never merged."
    )


def load_graduated_status(path: pathlib.Path) -> dict[str, tuple[str, int]]:
    """WHY: this script is the ledger's sole regenerator, and it used to
    hardcode every UPSTREAM_MAP-mapped row to 'derived' unconditionally —
    which silently reverts a PLAN.md §2 land-dark transition (derived ->
    dual) on the very next run, since nothing else ever re-asserts 'dual'.
    A row that has already graduated past 'derived' (dual or sovereign, per
    a prior hand-driven transition — see
    scripts/krites-provenance-transition.py) keeps that status and its
    soak_expires_at_commit_count across regeneration; only a row still
    sitting at 'derived' (or genuinely new) gets recomputed from scratch.
    Best-effort: a missing or genuinely-unparsable prior ledger yields no
    preservation, which is correct for the ledger's first-ever run.

    SAFETY(#6656): reads with bare tomllib, NOT parse_ledger/validate_rows.
    This function only ever consumes two fields (status,
    soak_expires_at_commit_count) — it has no business demanding the FULL
    current row schema validate before it can read them. The prior version
    routed through parse_ledger and swallowed every exception, status
    schema mismatch included, as "nothing to preserve" — which meant a
    ledger written before this field's own introduction (replaced_upstream_path)
    would fail validate_rows on its first post-migration read and silently
    drop EVERY dual/sovereign row's preservation, walking 31 real `dual`
    rows back to `derived` and erasing their soak windows on this script's
    very next run. Caught only by re-running immediately after the schema
    change and noticing the dual count collapse from 35 to 4 — a schema
    addition must not depend on every historical ledger already having it."""
    if not path.exists():
        return {}
    try:
        data = tomllib.loads(path.read_text())
    except tomllib.TOMLDecodeError as exc:
        raise SystemExit(unparsable_ledger_message(path, exc)) from exc
    graduated = {}
    for r in data.get("file", []):
        if not isinstance(r, dict):
            continue
        status = r.get("status")
        path_key = r.get("path")
        soak = r.get("soak_expires_at_commit_count")
        if status in ("dual", "sovereign") and isinstance(path_key, str) and isinstance(soak, int):
            graduated[path_key] = (status, soak)
    return graduated


def load_prior_paths(path: pathlib.Path) -> set[str]:
    """Every path the prior ledger recorded, for move detection.

    Read with bare tomllib for the same reason load_graduated_status is: this
    consumes a single field and has no business demanding the full current row
    schema validate before it can read it.
    """
    if not path.exists():
        return set()
    try:
        data = tomllib.loads(path.read_text())
    except tomllib.TOMLDecodeError as exc:
        raise SystemExit(unparsable_ledger_message(path, exc)) from exc
    return {
        r["path"]
        for r in data.get("file", [])
        if isinstance(r, dict) and isinstance(r.get("path"), str)
    }


def check_dual_survives_move(
    graduated: dict[str, tuple[str, int]],
    prior_paths: set[str],
    rows: list[dict[str, object]],
) -> None:
    """Refuse to write a ledger in which a moved file dropped its soak fuse.

    WHY: a `dual` row carries the only live fuse in the scheme -- the absolute
    commit count at which its derived copy gets deleted. Status preservation
    keys `graduated` on the ledger's recorded path and looks it up by the
    file's CURRENT path, so a moved dual file matches nothing, falls through to
    DUAL_SOAK_WINDOW (path-keyed too), and is rewritten as `derived` with soak
    0. Measured: a dual row at soak 3108 came back `derived`/0 after a plain
    `git mv` plus the UPSTREAM_MAP rekey that any move already requires -- and
    every downstream check stayed green, because the resulting ledger is
    perfectly self-consistent. The retirement that row was scheduled for simply
    never happens, and nothing reports it.

    WHY this shape rather than failing on any vanished dual row: retirement
    legitimately deletes a dual file -- that IS `land-dark -> soak -> delete`
    completing, and it must stay possible. A deletion removes a row and adds
    none; a move removes one and adds another. Requiring both lets retirement
    through and stops the move. A wave that retires and adds files in one pass
    trips this too; that is a deliberate false positive, since it is exactly
    the case where a human should confirm which fuse belongs where.
    """
    new_paths = {r["path"] for r in rows}
    vanished_dual = sorted(
        p for p, (status, _) in graduated.items() if status == "dual" and p not in new_paths
    )
    appeared = sorted(new_paths - prior_paths)
    if not (vanished_dual and appeared):
        return
    raise SystemExit(
        "dual rows vanished while new rows appeared -- a moved `dual` file "
        "loses its soak fuse silently.\n"
        f"  gone (was dual): {', '.join(vanished_dual)}\n"
        f"  new:             {', '.join(appeared)}\n"
        "If these are the same file moved, re-record its status at the new "
        "path with scripts/krites-provenance-transition.py before regenerating, "
        "so the soak_expires_at_commit_count carries across. If the file was "
        "retired, retire it on its own -- do not add files in the same pass."
    )


def main() -> None:
    local_files = iter_src_files()
    mapped = set(UPSTREAM_MAP)
    missing_from_map = sorted(set(local_files) - mapped)
    stale_in_map = sorted(mapped - set(local_files))
    if missing_from_map:
        raise SystemExit(
            "UPSTREAM_MAP is missing rows for: " + ", ".join(missing_from_map)
        )
    if stale_in_map:
        raise SystemExit(
            "UPSTREAM_MAP has rows for files that no longer exist: " + ", ".join(stale_in_map)
        )
    # SAFETY(#6656): a SOVEREIGN_VERIFY_MAP entry only makes sense for a path
    # UPSTREAM_MAP maps to None -- a path with a real UPSTREAM_MAP entry
    # already carries a live lineage claim and is measured against IT, not a
    # second retained-replacement target. A key present in both maps is a
    # contradiction, not a preference to resolve silently.
    contradictory = sorted(k for k in SOVEREIGN_VERIFY_MAP if UPSTREAM_MAP.get(k) is not None)
    if contradictory:
        raise SystemExit(
            "SOVEREIGN_VERIFY_MAP has entries for paths UPSTREAM_MAP already maps to a real "
            "upstream_path (a row cannot carry both a live lineage claim and a retained "
            "replacement target): " + ", ".join(contradictory)
        )
    stale_verify = sorted(set(SOVEREIGN_VERIFY_MAP) - set(local_files))
    if stale_verify:
        raise SystemExit(
            "SOVEREIGN_VERIFY_MAP has rows for files that no longer exist: " + ", ".join(stale_verify)
        )

    graduated = load_graduated_status(LEDGER_PATH)
    prior_paths = load_prior_paths(LEDGER_PATH)

    rows: list[dict] = []
    for rel in local_files:
        upstream_rel = UPSTREAM_MAP[rel]
        local_text = (KRITES_SRC / rel).read_text(errors="replace")

        if upstream_rel is None:
            # No lineage claim, ever. SOVEREIGN_VERIFY_MAP optionally still
            # names a predecessor to measure against for real, rather than
            # hardcoding verbatim_pct=0.0 for every such row regardless of
            # how similar it actually is (aletheia#6656) -- a path absent
            # from SOVEREIGN_VERIFY_MAP has no predecessor at all, so 0.0
            # here is a genuine measurement, not a placeholder.
            verify_rel = SOVEREIGN_VERIFY_MAP.get(rel)
            if verify_rel is not None:
                measured_pct = verbatim_pct(local_text, fetch_upstream(verify_rel))
            else:
                measured_pct = 0.0
            rows.append(
                {
                    "path": rel,
                    "upstream_path": "none",
                    "replaced_upstream_path": verify_rel or "none",
                    "verbatim_pct": measured_pct,
                    "status": "sovereign",
                    "soak_expires_at_commit_count": 0,
                }
            )
            continue

        # A real lineage path. derived/dual/an-already-graduated-sovereign
        # row (PLAN.md §2(c): a prior in-place dual -> sovereign transition
        # via krites-provenance-transition.py) are all measured against it
        # here -- only which FIELD the number lands in differs by status.
        upstream_text = fetch_upstream(upstream_rel)
        measured_pct = verbatim_pct(local_text, upstream_text)
        # WHY both sources, in this order: they solve different halves and either alone loses data.
        # The ledger is authoritative for a transition that has ALREADY happened -- regenerating must
        # not silently walk a `dual` row back to `derived`, which is what the original unconditional
        # "derived" did and what would have quietly undone every land-dark wave on the next regen.
        # DUAL_SOAK_WINDOW seeds a transition the ledger has not recorded yet, which is the only case
        # preservation cannot cover: the first regen after a wave flips a file.
        preserved = graduated.get(rel)
        if preserved:
            status, soak = preserved
        else:
            soak = DUAL_SOAK_WINDOW.get(rel, 0)
            status = "dual" if soak else "derived"

        if status == "sovereign":
            # SAFETY(#6656): a graduated-sovereign row must NOT carry
            # upstream_path=upstream_rel forward -- validate_rows requires
            # upstream_path='none' on every sovereign row (no live lineage
            # claim survives the transition). Regenerating used to do
            # exactly that (this branch didn't exist), producing an invalid
            # ledger the moment anyone re-ran this script after a real
            # in-place dual -> sovereign transition. The measurement itself
            # is still real -- it lands in replaced_upstream_path instead.
            rows.append(
                {
                    "path": rel,
                    "upstream_path": "none",
                    "replaced_upstream_path": upstream_rel,
                    "verbatim_pct": measured_pct,
                    "status": "sovereign",
                    "soak_expires_at_commit_count": 0,
                }
            )
        else:
            rows.append(
                {
                    "path": rel,
                    "upstream_path": upstream_rel,
                    "replaced_upstream_path": "none",
                    "verbatim_pct": measured_pct,
                    "status": status,
                    "soak_expires_at_commit_count": soak,
                }
            )

    check_dual_survives_move(graduated, prior_paths, rows)

    meta = {"upstream_repo": UPSTREAM_REPO, "upstream_ref": UPSTREAM_REF}
    LEDGER_PATH.write_text(dump_ledger(meta, rows))
    NOTICE_PATH.write_text(render_notice(meta, rows))
    derived_ct = sum(1 for r in rows if r["status"] == "derived")
    sovereign_ct = sum(1 for r in rows if r["status"] == "sovereign")
    dual_ct = sum(1 for r in rows if r["status"] == "dual")
    print(
        f"wrote {LEDGER_PATH} ({len(rows)} rows: {derived_ct} derived, "
        f"{dual_ct} dual, {sovereign_ct} sovereign)"
    )
    print(f"wrote {NOTICE_PATH}")


if __name__ == "__main__":
    main()
