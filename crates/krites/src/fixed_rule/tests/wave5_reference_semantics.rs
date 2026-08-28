//! Reference-semantics property tests for the wave-5 sovereign rewrites.
//!
//! These compare each fixed rule's *observable* output against an
//! independent reference computed straight from the algorithm's textbook
//! definition inside the test itself — never against the derived
//! implementation's internals. The 19 zero-call-site algorithms in this
//! wave landed sovereign-by-default, so those tests exercise the sovereign
//! implementations directly. `PageRank` is one of wave 5's "live 3"
//! (RETIREMENT-PLAN.md §5) and is still landed **dual** — the tests below
//! query it through `PageRank(...)` without selecting a feature, so they
//! run against whichever shell is compiled: the CozoDB-derived one by
//! default, and the sovereign `pagerank_native.rs` shell under
//! `--features krites_sovereign_pagerank`. Both shells delegate to the same
//! already-sovereign numeric core (`fixed_rule::csr::page_rank`), so this
//! is a genuine equivalence check on the option-parsing glue, not a
//! restatement of the core's own math.
#![cfg(test)]
#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "test data with known structure, and reference algorithms operating on dense small graphs"
)]
#![expect(
    clippy::as_conversions,
    clippy::cast_possible_wrap,
    reason = "test: small proptest graph sizes fit target numeric ranges"
)]
#![expect(
    clippy::mutable_key_type,
    reason = "DataValue implements Hash via canonical byte representation — safe as BTreeMap/BTreeSet key"
)]

use std::collections::{BTreeMap, BTreeSet};

use proptest::prelude::*;

use crate::DbInstance;
use crate::data::value::DataValue;

fn weighted_edges(n: usize, extra: &[(usize, usize, f64)]) -> Vec<(usize, usize, f64)> {
    let mut edges = vec![];
    let mut seen_pairs: BTreeSet<(usize, usize)> = BTreeSet::new();
    let mut push_new_pair =
        |edges: &mut Vec<(usize, usize, f64)>, src: usize, dst: usize, w: f64| {
            let pair = (src.min(dst), src.max(dst));
            // WHY: a second edge on an already-used unordered pair (e.g. the
            // reverse direction) hits krites#kruskal-parallel-edge-overwrite —
            // MinimumSpanningForestKruskal's priority queue keys candidate
            // edges by the directed (from, to) index pair, so an undirected
            // duplicate silently replaces rather than compares by weight.
            // Tracked upstream; this generator sidesteps it rather than
            // encoding the derived bug into a "reference" test.
            if seen_pairs.insert(pair) {
                edges.push((src, dst, w));
            }
        };
    for i in 0..n.saturating_sub(1) {
        push_new_pair(&mut edges, i, i + 1, 1.0);
    }
    for &(src, dst, w) in extra {
        if src < n && dst < n && src != dst && w > 0.0 && w.is_finite() {
            push_new_pair(&mut edges, src, dst, w);
        }
    }
    edges
}

fn edge_list_literal(edges: &[(usize, usize, f64)]) -> String {
    edges
        .iter()
        .map(|(s, d, w)| format!("[{s}, {d}, {w}]"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Independent Bellman-Ford single-source shortest paths (non-negative
/// weights). O(V*E), deliberately not sharing any code with the Dijkstra
/// implementation under test.
fn bellman_ford(n: usize, edges: &[(usize, usize, f64)], source: usize) -> Vec<f64> {
    let mut dist = vec![f64::INFINITY; n];
    dist[source] = 0.0;
    for _ in 0..n {
        let mut changed = false;
        for &(u, v, w) in edges {
            if dist[u].is_finite() && dist[u] + w < dist[v] {
                dist[v] = dist[u] + w;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    dist
}

fn arb_weighted_edge(max_node: usize) -> impl Strategy<Value = (usize, usize, f64)> {
    (0..max_node, 0..max_node, 0.1f64..10.0f64)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(25))]

    /// `ShortestPathDijkstra`'s per-target cost must agree with an
    /// independent Bellman-Ford reference computed from the same edge list.
    #[test]
    fn dijkstra_agrees_with_bellman_ford(
        n in 3usize..8,
        extra in proptest::collection::vec(arb_weighted_edge(8), 0..6),
    ) {
        let edges = weighted_edges(n, &extra);
        let reference = bellman_ford(n, &edges, 0);

        let query = format!(
            "edges[src, dst, cost] <- [{}]\n\
             start[] <- [[0]]\n\
             ?[from, to, cost, path] <~ ShortestPathDijkstra(edges[], start[])",
            edge_list_literal(&edges)
        );
        let db = DbInstance::default();
        let res = db.run_default(&query).expect("Dijkstra query should execute").rows;

        let mut observed: BTreeMap<i64, f64> = BTreeMap::new();
        for row in &res {
            let to = row[1].get_int().expect("target should be an int");
            let cost = row[2].get_float().expect("cost should be a float");
            observed.insert(to, cost);
        }

        for (target, &expected) in reference.iter().enumerate() {
            let actual = observed.get(&(target as i64)).copied();
            if expected.is_finite() {
                let actual = actual.expect("reachable target should have a Dijkstra row");
                prop_assert!(
                    (actual - expected).abs() < 1e-6,
                    "node {target}: dijkstra={actual}, bellman-ford={expected}"
                );
            } else if let Some(actual) = actual {
                prop_assert!(
                    !actual.is_finite(),
                    "node {target}: bellman-ford says unreachable but dijkstra reports {actual}"
                );
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(20))]

    /// Prim's and Kruskal's minimum spanning tree/forest must have exactly
    /// the same total weight on any connected weighted graph, regardless of
    /// which of the two independent construction strategies is used.
    #[test]
    fn prim_and_kruskal_agree_on_mst_weight(
        n in 3usize..7,
        extra in proptest::collection::vec(arb_weighted_edge(7), 0..5),
    ) {
        let edges = weighted_edges(n, &extra);
        let literal = edge_list_literal(&edges);

        let db = DbInstance::default();
        let kruskal_res = db
            .run_default(&format!(
                "edges[src, dst, cost] <- [{literal}]\n\
                 ?[src, dst, cost] <~ MinimumSpanningForestKruskal(edges[])"
            ))
            .expect("Kruskal query should execute")
            .rows;
        let prim_res = db
            .run_default(&format!(
                "edges[src, dst, cost] <- [{literal}]\n\
                 ?[src, dst, cost] <~ MinimumSpanningTreePrim(edges[])"
            ))
            .expect("Prim query should execute")
            .rows;

        let sum = |rows: &[Vec<DataValue>]| -> f64 {
            rows.iter()
                .map(|r| r[2].get_float().expect("MST edge cost should be a float"))
                .sum()
        };
        let kruskal_weight = sum(&kruskal_res);
        let prim_weight = sum(&prim_res);

        prop_assert_eq!(
            kruskal_res.len(),
            prim_res.len(),
            "Prim and Kruskal should select the same number of MST edges"
        );
        prop_assert!(
            (kruskal_weight - prim_weight).abs() < 1e-6,
            "Kruskal MST weight {kruskal_weight} != Prim MST weight {prim_weight}"
        );
    }
}

/// Brute-force mutual-reachability partition: two nodes share a component
/// iff each can reach the other by following directed edges. Independent of
/// (and asymptotically worse than) Tarjan/Kosaraju, used only as a ground
/// truth here.
fn brute_force_scc(n: usize, edges: &[(usize, usize)]) -> Vec<BTreeSet<usize>> {
    let mut adj: Vec<Vec<usize>> = vec![vec![]; n];
    for &(u, v) in edges {
        adj[u].push(v);
    }
    let reachable = |start: usize| -> BTreeSet<usize> {
        let mut seen = BTreeSet::from([start]);
        let mut stack = vec![start];
        while let Some(node) = stack.pop() {
            for &next in &adj[node] {
                if seen.insert(next) {
                    stack.push(next);
                }
            }
        }
        seen
    };
    let forward: Vec<BTreeSet<usize>> = (0..n).map(reachable).collect();

    let mut assigned = vec![false; n];
    let mut components = vec![];
    for node in 0..n {
        if assigned[node] {
            continue;
        }
        let mut group = BTreeSet::new();
        for other in 0..n {
            if forward[node].contains(&other) && forward[other].contains(&node) {
                group.insert(other);
                assigned[other] = true;
            }
        }
        components.push(group);
    }
    components
}

fn arb_edge_pair(max_node: usize) -> impl Strategy<Value = (usize, usize)> {
    (0..max_node, 0..max_node)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(25))]

    /// `StronglyConnectedComponents`' partition must match a brute-force
    /// mutual-reachability computation on the same directed graph.
    #[test]
    fn scc_agrees_with_brute_force_reachability(
        n in 2usize..7,
        raw_edges in proptest::collection::vec(arb_edge_pair(7), 1..10),
    ) {
        let edges: Vec<(usize, usize)> = raw_edges.into_iter().filter(|&(s, d)| s < n && d < n).collect();
        prop_assume!(!edges.is_empty());

        let literal = edges
            .iter()
            .map(|(s, d)| format!("[{s}, {d}]"))
            .collect::<Vec<_>>()
            .join(", ");
        let query = format!(
            "edges[src, dst] <- [{literal}]\n\
             ?[node, component] <~ StronglyConnectedComponents(edges[])"
        );
        let db = DbInstance::default();
        let res = db.run_default(&query).expect("SCC query should execute").rows;

        let mut observed: BTreeMap<i64, DataValue> = BTreeMap::new();
        for row in &res {
            let node = row[0].get_int().expect("node should be an int");
            observed.insert(node, row[1].clone());
        }

        // WHY: with no `nodes[]` relation supplied, only nodes that appear
        // as a source or target actually receive a component row — a node
        // absent from every edge is its own trivial brute-force SCC but is
        // simply not queried, so it must not be compared here.
        let present_nodes: BTreeSet<usize> = edges.iter().flat_map(|&(s, d)| [s, d]).collect();
        let reference: Vec<BTreeSet<usize>> = brute_force_scc(n, &edges)
            .into_iter()
            .map(|group| group.intersection(&present_nodes).copied().collect::<BTreeSet<_>>())
            .filter(|group: &BTreeSet<usize>| !group.is_empty())
            .collect();
        for group in &reference {
            let labels: BTreeSet<Option<&DataValue>> =
                group.iter().map(|n| observed.get(&(*n as i64))).collect();
            prop_assert_eq!(
                labels.len(),
                1,
                "nodes {:?} form one brute-force SCC but got labels {:?}",
                group,
                labels
            );
        }

        // Distinct brute-force components must never share a component label.
        for (i, group_a) in reference.iter().enumerate() {
            for group_b in reference.iter().skip(i + 1) {
                let (Some(a_node), Some(b_node)) = (group_a.iter().next(), group_b.iter().next())
                else {
                    continue;
                };
                prop_assert_ne!(
                    observed.get(&(*a_node as i64)),
                    observed.get(&(*b_node as i64)),
                    "distinct brute-force SCCs {:?} and {:?} got the same label",
                    group_a,
                    group_b
                );
            }
        }
    }
}

/// Bidirectional-chain-plus-extra-edges generator: every node keeps out-degree
/// >= 1, so `power_iteration_reference`'s per-node division by out-degree never
/// hits zero. `page_rank`'s dangling-node behavior (out-degree 0) is identical
/// on both shells since they share the numeric core, so it is out of scope for
/// an equivalence test between them.
fn connected_directed_edges(n: usize, extra: &[(usize, usize)]) -> Vec<(usize, usize)> {
    let mut edges = vec![];
    let mut seen: BTreeSet<(usize, usize)> = BTreeSet::new();
    let mut push_new_edge = |edges: &mut Vec<(usize, usize)>, src: usize, dst: usize| {
        // WHY: `edges[src, dst] <- [...]` is a Datalog relation, which has SET
        // semantics — a repeated (src, dst) tuple collapses to one row in the
        // engine's input, so `PageRank` sees a lower out-degree for that node
        // than a reference computed from this generator's raw (possibly
        // duplicate) edge list would assume. Dedup here on the DIRECTED pair
        // (unlike `weighted_edges`'s unordered dedup above, which sidesteps a
        // different bug) so the reference always sees the same edge set the
        // engine actually queries.
        if seen.insert((src, dst)) {
            edges.push((src, dst));
        }
    };
    for i in 0..n.saturating_sub(1) {
        push_new_edge(&mut edges, i, i + 1);
        push_new_edge(&mut edges, i + 1, i);
    }
    for &(src, dst) in extra {
        if src < n && dst < n && src != dst {
            push_new_edge(&mut edges, src, dst);
        }
    }
    edges
}

/// Independent power-iteration reference, coded straight from Page et al.
/// (1999) rather than calling `fixed_rule::csr::page_rank` — this is the same
/// numeric core both the derived and sovereign shells delegate to, so calling
/// it here would test the core against itself and prove nothing about the
/// shells being replaced.
#[expect(
    clippy::cast_precision_loss,
    reason = "test: small proptest node/degree counts fit f64 exactly"
)]
fn power_iteration_reference(
    n: usize,
    edges: &[(usize, usize)],
    damping: f64,
    tolerance: f64,
    max_iterations: usize,
) -> Vec<f64> {
    let mut out_degree = vec![0usize; n];
    let mut in_neighbors: Vec<Vec<usize>> = vec![vec![]; n];
    for &(src, dst) in edges {
        out_degree[src] += 1;
        in_neighbors[dst].push(src);
    }

    let initial = 1.0 / n as f64;
    let base = (1.0 - damping) / n as f64;
    let mut scores = vec![initial; n];
    let mut out_scores: Vec<f64> = (0..n).map(|i| initial / out_degree[i] as f64).collect();

    for _ in 0..max_iterations {
        let mut new_scores = vec![0.0; n];
        let mut error = 0.0_f64;
        for node in 0..n {
            let incoming: f64 = in_neighbors[node].iter().map(|&src| out_scores[src]).sum();
            new_scores[node] = base + damping * incoming;
            error += (new_scores[node] - scores[node]).abs();
        }
        scores = new_scores;
        out_scores = (0..n).map(|i| scores[i] / out_degree[i] as f64).collect();
        if error < tolerance {
            break;
        }
    }
    scores
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(25))]

    /// `PageRank`'s per-node converged score must agree with an independent
    /// power-iteration reference on the same directed graph — RETIREMENT-PLAN.md
    /// wave 5's own gate for "the live 3" ("PageRank convergence and
    /// sum-to-one"; sum-to-one is covered separately by
    /// `proptest_algos::pagerank_scores_sum_to_one`, which this test leaves
    /// alone rather than duplicating). Runs against whichever shell is
    /// compiled — see the module doc comment.
    #[test]
    fn pagerank_agrees_with_power_iteration_reference(
        n in 3usize..8,
        extra in proptest::collection::vec(arb_edge_pair(8), 0..5),
    ) {
        let edges = connected_directed_edges(n, &extra);
        let literal = edges
            .iter()
            .map(|(s, d)| format!("[{s}, {d}]"))
            .collect::<Vec<_>>()
            .join(", ");

        let damping = 0.85_f64;
        let tolerance = 0.0001_f64;
        let max_iterations = 100_usize;
        let reference = power_iteration_reference(n, &edges, damping, tolerance, max_iterations);

        let query = format!(
            "edges[src, dst] <- [{literal}]\n\
             ?[node, rank] <~ PageRank(edges[], iterations: {max_iterations})"
        );
        let db = DbInstance::default();
        let res = db.run_default(&query).expect("PageRank query should execute").rows;

        let mut observed: BTreeMap<i64, f64> = BTreeMap::new();
        for row in &res {
            let node = row[0].get_int().expect("node should be an int");
            let rank = row[1].get_float().expect("rank should be a float");
            observed.insert(node, rank);
        }

        prop_assert_eq!(observed.len(), n, "PageRank should return one row per node");
        for (node, &expected) in reference.iter().enumerate() {
            let actual = *observed
                .get(&(node as i64))
                .expect("every node should have a PageRank row");
            // WHY 1e-3, not 1e-6 like the Dijkstra comparison above: the fixed
            // rule's numeric core iterates in f32 (crate::fixed_rule::csr::page_rank),
            // while this reference iterates in f64 -- rounding accumulates over
            // up to 100 iterations, and 1e-3 is well inside PageRank's own
            // convergence tolerance (0.0001 per-iteration delta, summed over N nodes).
            prop_assert!(
                (actual - expected).abs() < 1e-3,
                "node {node}: PageRank={actual}, power-iteration reference={expected}"
            );
        }
    }
}
