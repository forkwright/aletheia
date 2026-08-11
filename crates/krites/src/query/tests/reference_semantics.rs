//! Reference-semantics tests for magic-set rewriting and semi-naive evaluation.
//!
//! `query/` is the largest untested surface in the crate: roughly 12.7k derived
//! lines behind a handful of in-module tests, covered only indirectly through
//! end-to-end scripts. Nothing pinned magic-set rewriting or semi-naive
//! fixpoint semantics directly, which is how a rewrite there loses correctness
//! silently.
//!
//! Two independent oracles, because either alone is insufficient:
//!
//! - **Differential.** The same program is run with magic-set rewriting on and
//!   with `:disable_magic_rewrite true`. The rewrite is an optimization, so the
//!   two answers must be identical for every input. This targets the rewrite
//!   precisely, but it cannot see a fault shared by both paths — semi-naive
//!   evaluation runs underneath either one.
//! - **Reference.** The engine's answer is compared against a transitive
//!   closure computed from its textbook definition inside this file, never
//!   against the engine's own internals. This is what catches a fault the two
//!   engine paths share, and it validates whichever implementation is compiled
//!   in rather than a particular one.
#![cfg(test)]
#![expect(
    clippy::indexing_slicing,
    reason = "test data of known shape, and reference algorithms over dense small graphs"
)]

use std::collections::BTreeSet;

use proptest::prelude::*;

use crate::DbInstance;
use crate::data::value::DataValue;

/// Transitive closure from the definition: the least fixpoint of "extend a
/// known path by one edge". Deliberately the naive O(n^3)-ish formulation --
/// this is the oracle, so it is written to be obviously correct rather than
/// fast, and it shares no code with the engine.
fn reference_closure(edges: &[(i64, i64)]) -> BTreeSet<(i64, i64)> {
    let mut reach: BTreeSet<(i64, i64)> = edges.iter().copied().collect();
    loop {
        let mut grown = reach.clone();
        for &(a, m) in &reach {
            for &(m2, b) in edges {
                if m == m2 {
                    grown.insert((a, b));
                }
            }
        }
        if grown.len() == reach.len() {
            return reach;
        }
        reach = grown;
    }
}

fn edge_literal(edges: &[(i64, i64)]) -> String {
    let items: Vec<String> = edges.iter().map(|(a, b)| format!("[{a}, {b}]")).collect();
    format!("[{}]", items.join(", "))
}

/// Run a script and collect its rows as integer pairs.
fn run_pairs(script: &str) -> BTreeSet<(i64, i64)> {
    let db = DbInstance::default();
    let rows = db
        .run_default(script)
        .unwrap_or_else(|e| panic!("script should run:\n{script}\nerror: {e}"))
        .rows;
    rows.iter()
        .map(|r| {
            assert_eq!(r.len(), 2, "expected arity-2 rows, got {r:?}");
            (int_of(&r[0]), int_of(&r[1]))
        })
        .collect()
}

fn run_singles(script: &str) -> BTreeSet<i64> {
    let db = DbInstance::default();
    let rows = db
        .run_default(script)
        .unwrap_or_else(|e| panic!("script should run:\n{script}\nerror: {e}"))
        .rows;
    rows.iter()
        .map(|r| {
            assert_eq!(r.len(), 1, "expected arity-1 rows, got {r:?}");
            int_of(&r[0])
        })
        .collect()
}

/// WHY `get_int_strict`: the permissive `get_int` accepts a whole-numbered float,
/// so a regression that returned 3.0 where 3 is required would be absorbed by
/// the oracle instead of caught by it. An oracle that launders a type change is
/// not an oracle.
fn int_of(v: &DataValue) -> i64 {
    v.get_int_strict()
        .unwrap_or_else(|| panic!("expected an integer result value, got {v:?}"))
}

/// Linear recursion: the shape semi-naive evaluation is built for -- each epoch
/// joins only the previous epoch's delta against the base relation.
fn tc_linear(edges: &[(i64, i64)], disable_magic: bool) -> String {
    format!(
        "edge[] <- {lit}\n\
         reach[a, b] := edge[a, b]\n\
         reach[a, b] := reach[a, m], edge[m, b]\n\
         ?[a, b] := reach[a, b]\n\
         {opt}",
        lit = edge_literal(edges),
        opt = magic_option(disable_magic),
    )
}

/// Non-linear (doubly) recursion: both body atoms are the recursive relation,
/// so each epoch must join delta against the full accumulated set on BOTH
/// sides. Getting only one side is a classic semi-naive bug that still
/// terminates and still returns a subset that looks plausible.
fn tc_nonlinear(edges: &[(i64, i64)], disable_magic: bool) -> String {
    format!(
        "edge[] <- {lit}\n\
         reach[a, b] := edge[a, b]\n\
         reach[a, b] := reach[a, m], reach[m, b]\n\
         ?[a, b] := reach[a, b]\n\
         {opt}",
        lit = edge_literal(edges),
        opt = magic_option(disable_magic),
    )
}

/// A bound argument in the entry rule is what makes the magic-set rewrite fire
/// at all: without a constant to propagate, there is no sideways information to
/// pass and the rewrite has nothing to restrict.
fn reach_from_const(edges: &[(i64, i64)], src: i64, disable_magic: bool) -> String {
    format!(
        "edge[] <- {lit}\n\
         reach[a, b] := edge[a, b]\n\
         reach[a, b] := reach[a, m], edge[m, b]\n\
         ?[b] := reach[{src}, b]\n\
         {opt}",
        lit = edge_literal(edges),
        opt = magic_option(disable_magic),
    )
}

fn magic_option(disable: bool) -> &'static str {
    if disable {
        ":disable_magic_rewrite true"
    } else {
        ""
    }
}

/// Small dense-ish graphs: node ids drawn from a narrow range so edges actually
/// chain into multi-hop paths. A wide id range would produce mostly-disjoint
/// edges, and every recursive program would agree trivially on an answer that
/// never exercised a second epoch.
fn edge_set() -> impl Strategy<Value = Vec<(i64, i64)>> {
    prop::collection::vec((0i64..6, 0i64..6), 1..12).prop_map(|mut v| {
        v.sort_unstable();
        v.dedup();
        v
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    /// Linear recursion against the definition.
    #[test]
    fn linear_transitive_closure_matches_the_definition(edges in edge_set()) {
        let got = run_pairs(&tc_linear(&edges, false));
        prop_assert_eq!(got, reference_closure(&edges));
    }

    /// Non-linear recursion must reach the SAME fixpoint as linear recursion.
    /// Both compute transitive closure; only the evaluation shape differs.
    #[test]
    fn nonlinear_transitive_closure_matches_the_definition(edges in edge_set()) {
        let got = run_pairs(&tc_nonlinear(&edges, false));
        prop_assert_eq!(got, reference_closure(&edges));
    }

    /// The magic-set rewrite is an optimization: switching it off must not
    /// change one row. Runs the shape where the rewrite actually fires.
    #[test]
    fn magic_rewrite_preserves_answers_on_bound_query(edges in edge_set(), src in 0i64..6) {
        let with = run_singles(&reach_from_const(&edges, src, false));
        let without = run_singles(&reach_from_const(&edges, src, true));
        prop_assert_eq!(with, without);
    }

    /// ...and the answer both paths agree on is the right one. Without this,
    /// the differential above passes on any fault the two paths share.
    #[test]
    fn bound_query_matches_the_definition(edges in edge_set(), src in 0i64..6) {
        let got = run_singles(&reach_from_const(&edges, src, false));
        let expected: BTreeSet<i64> = reference_closure(&edges)
            .into_iter()
            .filter_map(|(a, b)| (a == src).then_some(b))
            .collect();
        prop_assert_eq!(got, expected);
    }

    /// The rewrite must also be answer-preserving on the unbound query, where
    /// it has no constant to propagate and should degenerate to a no-op.
    #[test]
    fn magic_rewrite_preserves_answers_on_unbound_query(edges in edge_set()) {
        let with = run_pairs(&tc_linear(&edges, false));
        let without = run_pairs(&tc_linear(&edges, true));
        prop_assert_eq!(with, without);
    }
}

/// A cycle is where a fixpoint that fails to detect saturation hangs, and where
/// one that stops an epoch early silently returns a subset. Fixed rather than
/// generated so the case is always exercised.
#[test]
fn cyclic_graph_reaches_a_complete_fixpoint() {
    let edges = [(0, 1), (1, 2), (2, 0)];
    let got = run_pairs(&tc_linear(&edges, false));
    // Every node reaches every node, itself included, in a 3-cycle.
    let expected: BTreeSet<(i64, i64)> = (0..3).flat_map(|a| (0..3).map(move |b| (a, b))).collect();
    assert_eq!(got, expected, "3-cycle closure must be total");
    assert_eq!(got, reference_closure(&edges));
}

/// Self-loops and duplicate edges must not change the closure, and must not
/// make the fixpoint iterate forever.
#[test]
fn self_loops_and_duplicates_do_not_change_the_closure() {
    let edges = [(0, 0), (0, 1), (0, 1), (1, 2)];
    let got = run_pairs(&tc_linear(&edges, false));
    assert_eq!(got, reference_closure(&edges));
}

/// A rule whose recursive atom can never be satisfied must terminate with the
/// base facts alone -- an epoch loop that re-runs a rule producing nothing new
/// without noticing is how a fixpoint fails to converge.
#[test]
fn non_productive_recursion_terminates_at_the_base_facts() {
    let edges = [(0, 1), (2, 3)];
    let got = run_pairs(&tc_linear(&edges, false));
    assert_eq!(got, reference_closure(&edges));
    assert_eq!(got, edges.iter().copied().collect::<BTreeSet<_>>());
}
