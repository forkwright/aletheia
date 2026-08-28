//! Graph algorithm implementations as fixed rules.
//!
//! Every module here is sovereign except `pagerank`, which is still landed
//! **dual**: the derived implementation stays the default, and a fresh
//! sovereign rewrite sits behind the `krites_sovereign_pagerank` feature
//! (RETIREMENT-PLAN.md wave 5, "the live 3" — PageRank has a live episteme
//! consumer via embedded Datalog, unlike the 19 zero-call-site algorithms
//! that already went through this same land-dark/soak/delete cycle). The
//! `_native.rs` filenames the other 19 were authored under, and the
//! `#[path]` attributes that reached them, were retired once their soak
//! completed; `pagerank_native.rs` carries the same suffix for the same
//! reason and will drop it the same way.
pub(crate) mod kcore;

pub(crate) mod all_pairs_shortest_path;
pub(crate) mod astar;
pub(crate) mod bfs;
pub(crate) mod degree_centrality;
pub(crate) mod dfs;
pub(crate) mod kruskal;
pub(crate) mod label_propagation;
pub(crate) mod louvain;

#[cfg(not(feature = "krites_sovereign_pagerank"))]
pub(crate) mod pagerank;
#[cfg(feature = "krites_sovereign_pagerank")]
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
