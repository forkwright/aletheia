//! Graph algorithm implementations as fixed rules.
//!
//! Every module here is sovereign. The `#[path]` attributes point at the
//! `_native.rs` filenames the sovereign rewrites were authored under while a
//! derived counterpart still occupied the plain name; the counterparts are
//! retired, so the suffix is now only a filename.
pub(crate) mod kcore;

#[path = "all_pairs_shortest_path_native.rs"]
pub(crate) mod all_pairs_shortest_path;

#[path = "astar_native.rs"]
pub(crate) mod astar;

#[path = "bfs_native.rs"]
pub(crate) mod bfs;

#[path = "degree_centrality_native.rs"]
pub(crate) mod degree_centrality;

#[path = "dfs_native.rs"]
pub(crate) mod dfs;

#[path = "kruskal_native.rs"]
pub(crate) mod kruskal;

#[path = "label_propagation_native.rs"]
pub(crate) mod label_propagation;

pub(crate) mod louvain;
pub(crate) mod pagerank;

#[path = "prim_native.rs"]
pub(crate) mod prim;

#[path = "random_walk_native.rs"]
pub(crate) mod random_walk;

#[path = "shortest_path_bfs_native.rs"]
pub(crate) mod shortest_path_bfs;

#[path = "shortest_path_dijkstra_native.rs"]
pub(crate) mod shortest_path_dijkstra;

#[path = "strongly_connected_components_native.rs"]
pub(crate) mod strongly_connected_components;

#[path = "top_sort_native.rs"]
pub(crate) mod top_sort;

#[path = "triangles_native.rs"]
pub(crate) mod triangles;

#[path = "yen_native.rs"]
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
