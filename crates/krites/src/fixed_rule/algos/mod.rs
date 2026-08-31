//! Graph algorithm implementations as fixed rules.
//!
//! Every module here is sovereign. `pagerank` was the last to go through
//! the land-dark/soak/delete cycle (RETIREMENT-PLAN.md wave 5, "the live
//! 3" — `PageRank` has a live episteme consumer via embedded Datalog,
//! unlike the 19 zero-call-site algorithms that preceded it): the
//! CozoDB-derived shell soaked as `dual` and was deleted in #7042, and the
//! sovereign shell in `pagerank_native.rs` is now the only implementation.
//! The `_native.rs` filenames the other 19 were authored under, and the
//! `#[path]` attributes that reached them, were dropped once their derived
//! siblings were gone; `pagerank_native.rs` carries the suffix for the
//! same historical reason and drops it the same way, in a follow-up rename
//! kept out of the retirement diff so the ledger's path-keyed maps move in
//! a change that changes nothing else.
pub(crate) mod kcore;

pub(crate) mod all_pairs_shortest_path;
pub(crate) mod astar;
pub(crate) mod bfs;
pub(crate) mod degree_centrality;
pub(crate) mod dfs;
pub(crate) mod kruskal;
pub(crate) mod label_propagation;
pub(crate) mod louvain;

#[path = "pagerank_native.rs"]
pub(crate) mod pagerank;

pub(crate) mod prim;
pub(crate) mod random_walk;
pub(crate) mod shortest_path_bfs;
pub(crate) mod shortest_path_dijkstra;
pub(crate) mod strongly_connected_components;
pub(crate) mod top_sort;
pub(crate) mod triangles;
pub(crate) mod yen;

pub(crate) use all_pairs_shortest_path::{BetweennessCentrality, ClosenessCentrality};
pub(crate) use astar::ShortestPathAStar;
pub(crate) use bfs::Bfs;
pub(crate) use degree_centrality::DegreeCentrality;
pub(crate) use dfs::Dfs;
pub(crate) use kcore::KCore;
pub(crate) use kruskal::MinimumSpanningForestKruskal;
pub(crate) use label_propagation::LabelPropagation;
pub(crate) use louvain::CommunityDetectionLouvain;
pub(crate) use pagerank::PageRank;
pub(crate) use prim::MinimumSpanningTreePrim;
pub(crate) use random_walk::RandomWalk;
pub(crate) use shortest_path_bfs::ShortestPathBFS;
pub(crate) use shortest_path_dijkstra::ShortestPathDijkstra;
pub(crate) use strongly_connected_components::StronglyConnectedComponent;
pub(crate) use top_sort::TopSort;
pub(crate) use triangles::ClusteringCoefficients;
pub(crate) use yen::KShortestPathYen;
