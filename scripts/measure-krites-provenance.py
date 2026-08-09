#!/usr/bin/env python3
"""Fetch pinned upstream cozo-core sources and (re)generate PROVENANCE.toml + NOTICE.md."""

from __future__ import annotations

import pathlib
import sys
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
    parse_ledger,
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
    "fixed_rule/algos/all_pairs_shortest_path.rs": "fixed_rule/algos/all_pairs_shortest_path.rs",
    "fixed_rule/algos/all_pairs_shortest_path_native.rs": None,
    "fixed_rule/algos/astar.rs": "fixed_rule/algos/astar.rs",
    "fixed_rule/algos/astar_native.rs": None,
    "fixed_rule/algos/bfs.rs": "fixed_rule/algos/bfs.rs",
    "fixed_rule/algos/bfs_native.rs": None,
    "fixed_rule/algos/degree_centrality.rs": "fixed_rule/algos/degree_centrality.rs",
    "fixed_rule/algos/degree_centrality_native.rs": None,
    "fixed_rule/algos/dfs.rs": "fixed_rule/algos/dfs.rs",
    "fixed_rule/algos/dfs_native.rs": None,
    "fixed_rule/algos/kcore.rs": None,
    "fixed_rule/algos/kruskal.rs": "fixed_rule/algos/kruskal.rs",
    "fixed_rule/algos/kruskal_native.rs": None,
    "fixed_rule/algos/label_propagation.rs": "fixed_rule/algos/label_propagation.rs",
    "fixed_rule/algos/label_propagation_native.rs": None,
    "fixed_rule/algos/louvain.rs": "fixed_rule/algos/louvain.rs",
    "fixed_rule/algos/mod.rs": "fixed_rule/algos/mod.rs",
    "fixed_rule/algos/pagerank.rs": "fixed_rule/algos/pagerank.rs",
    "fixed_rule/algos/prim.rs": "fixed_rule/algos/prim.rs",
    "fixed_rule/algos/prim_native.rs": None,
    "fixed_rule/algos/random_walk.rs": "fixed_rule/algos/random_walk.rs",
    "fixed_rule/algos/random_walk_native.rs": None,
    "fixed_rule/algos/shortest_path_bfs.rs": "fixed_rule/algos/shortest_path_bfs.rs",
    "fixed_rule/algos/shortest_path_bfs_native.rs": None,
    "fixed_rule/algos/shortest_path_dijkstra.rs": "fixed_rule/algos/shortest_path_dijkstra.rs",
    "fixed_rule/algos/shortest_path_dijkstra_native.rs": None,
    "fixed_rule/algos/strongly_connected_components.rs": "fixed_rule/algos/strongly_connected_components.rs",
    "fixed_rule/algos/strongly_connected_components_native.rs": None,
    "fixed_rule/algos/top_sort.rs": "fixed_rule/algos/top_sort.rs",
    "fixed_rule/algos/top_sort_native.rs": None,
    "fixed_rule/algos/triangles.rs": "fixed_rule/algos/triangles.rs",
    "fixed_rule/algos/triangles_native.rs": None,
    "fixed_rule/algos/yen.rs": "fixed_rule/algos/yen.rs",
    "fixed_rule/algos/yen_native.rs": None,
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
    "fixed_rule/utilities/constant.rs": "fixed_rule/utilities/constant.rs",
    "fixed_rule/utilities/constant_native.rs": None,
    "fixed_rule/utilities/mod.rs": "fixed_rule/utilities/mod.rs",
    "fixed_rule/utilities/reorder_sort.rs": "fixed_rule/utilities/reorder_sort.rs",
    "fixed_rule/utilities/reorder_sort_native.rs": None,
    "fixed_rule/utilities/rrf.rs": None,
    "fts/README.md": "fts/README.md",
    "fts/ast.rs": "fts/ast.rs",
    "fts/config.rs": "fts/mod.rs",
    "fts/error.rs": None,
    "fts/indexing.rs": "fts/indexing.rs",
    "fts/mod.rs": "fts/mod.rs",
    "fts/tokenizer/alphanum_only.rs": "fts/tokenizer/alphanum_only.rs",
    "fts/tokenizer/ascii_folding_filter/fold_table.rs": "fts/tokenizer/ascii_folding_filter.rs",
    "fts/tokenizer/ascii_folding_filter/fold_table/fold_digits_symbols.rs": "fts/tokenizer/ascii_folding_filter.rs",
    "fts/tokenizer/ascii_folding_filter/fold_table/fold_letters_a_m.rs": "fts/tokenizer/ascii_folding_filter.rs",
    "fts/tokenizer/ascii_folding_filter/fold_table/fold_letters_n_z.rs": "fts/tokenizer/ascii_folding_filter.rs",
    # wave2a/ascii-folding-table: regenerated from UCD + CLDR Latin-ASCII, not
    # transcribed from cozo-core -- no upstream lineage.
    "fts/tokenizer/ascii_folding_filter/fold_table/fold_table_sovereign/generate.py": None,
    "fts/tokenizer/ascii_folding_filter/fold_table/fold_table_sovereign/mod.rs": None,
    "fts/tokenizer/ascii_folding_filter/fold_table/fold_table_sovereign/table.rs": None,
    "fts/tokenizer/ascii_folding_filter/mod.rs": "fts/tokenizer/ascii_folding_filter.rs",
    # wave2a/ascii-folding-table: the full-BMP-sweep conformance test proving
    # fold_table_sovereign/ equivalent to fold_table/ -- no upstream lineage.
    "fts/tokenizer/ascii_folding_filter/tests/bmp_equivalence.rs": None,
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
    "fts/tokenizer/stop_word_filter/derived/gen_stopwords.py": "fts/tokenizer/stop_word_filter/gen_stopwords.py",
    "fts/tokenizer/stop_word_filter/derived/mod.rs": "fts/tokenizer/stop_word_filter/mod.rs",
    "fts/tokenizer/stop_word_filter/derived/stopwords/af_da.rs": "fts/tokenizer/stop_word_filter/stopwords.rs",
    "fts/tokenizer/stop_word_filter/derived/stopwords/el_ja.rs": "fts/tokenizer/stop_word_filter/stopwords.rs",
    "fts/tokenizer/stop_word_filter/derived/stopwords/ko_ro.rs": "fts/tokenizer/stop_word_filter/stopwords.rs",
    "fts/tokenizer/stop_word_filter/derived/stopwords/mod.rs": "fts/tokenizer/stop_word_filter/stopwords.rs",
    "fts/tokenizer/stop_word_filter/derived/stopwords/nl_de.rs": "fts/tokenizer/stop_word_filter/stopwords.rs",
    "fts/tokenizer/stop_word_filter/derived/stopwords/ru_ur.rs": "fts/tokenizer/stop_word_filter/stopwords.rs",
    "fts/tokenizer/stop_word_filter/derived/stopwords/vi_zu.rs": "fts/tokenizer/stop_word_filter/stopwords.rs",
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
    "query/stored/extractors.rs": "query/stored.rs",
    "query/stored/mod.rs": "query/stored.rs",
    "query/stored/mutation.rs": "query/stored.rs",
    "query/stored/validation.rs": "query/stored.rs",
    "query/stratify.rs": "query/stratify.rs",
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
    "runtime/mod.rs": "runtime/mod.rs",
    "runtime/relation/handles.rs": "runtime/relation.rs",
    "runtime/relation/index_create.rs": "runtime/relation.rs",
    "runtime/relation/index_management.rs": "runtime/relation.rs",
    "runtime/relation/mod.rs": "runtime/relation.rs",
    "runtime/relation/relation_crud.rs": "runtime/relation.rs",
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
DUAL_SOAK_WINDOW: dict[str, int] = {
    # wave2a/ascii-folding-table: land-dark PR lands at commit count 2808.
    # +30 commits is PLAN.md's own Q3 recommended window for low-blast-radius
    # waves (2a, 2b, 5, 7) -- this is a LOW-risk, pure-data wave with a
    # full-BMP-sweep conformance gate already proving equivalence
    # (tests/bmp_equivalence.rs), so there is no soak-observation need beyond
    # CI turning green on the sovereign feature.
    "fts/tokenizer/ascii_folding_filter/fold_table.rs": 2838,
    "fts/tokenizer/ascii_folding_filter/fold_table/fold_digits_symbols.rs": 2838,
    "fts/tokenizer/ascii_folding_filter/fold_table/fold_letters_a_m.rs": 2838,
    "fts/tokenizer/ascii_folding_filter/fold_table/fold_letters_n_z.rs": 2838,
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
    Best-effort: a missing or unparsable prior ledger yields no
    preservation, which is correct for the ledger's first-ever run."""
    if not path.exists():
        return {}
    try:
        _, rows = parse_ledger(path.read_text())
    except Exception:  # noqa: BLE001 — any parse failure means "nothing to preserve"
        return {}
    return {
        r["path"]: (r["status"], r["soak_expires_at_commit_count"])
        for r in rows
        if r["status"] in ("dual", "sovereign")
    }


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

    graduated = load_graduated_status(LEDGER_PATH)

    rows: list[dict] = []
    for rel in local_files:
        upstream_rel = UPSTREAM_MAP[rel]
        local_text = (KRITES_SRC / rel).read_text(errors="replace")
        if upstream_rel is None:
            rows.append(
                {
                    "path": rel,
                    "upstream_path": "none",
                    "verbatim_pct": 0.0,
                    "status": "sovereign",
                    "soak_expires_at_commit_count": 0,
                }
            )
            continue
        upstream_text = fetch_upstream(upstream_rel)
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
        rows.append(
            {
                "path": rel,
                "upstream_path": upstream_rel,
                "verbatim_pct": verbatim_pct(local_text, upstream_text),
                "status": status,
                "soak_expires_at_commit_count": soak,
            }
        )

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
