//! Graph algorithm implementations as fixed rules.
//!
//! Each module below is landed **dual**: the derived implementation stays
//! the default, and a fresh sovereign rewrite sits behind the
//! `krites_sovereign_algos` feature (PLAN.md §2 "land dark"). `KCore` has
//! no derived counterpart — it was written sovereign from day one and is
//! not part of this scheme.
pub(crate) mod kcore;

#[cfg(not(feature = "krites_sovereign_algos"))]
pub(crate) mod all_pairs_shortest_path;
#[cfg(feature = "krites_sovereign_algos")]
#[path = "all_pairs_shortest_path_native.rs"]
pub(crate) mod all_pairs_shortest_path;

#[cfg(not(feature = "krites_sovereign_algos"))]
pub(crate) mod astar;
#[cfg(feature = "krites_sovereign_algos")]
#[path = "astar_native.rs"]
pub(crate) mod astar;

#[cfg(not(feature = "krites_sovereign_algos"))]
pub(crate) mod bfs;
#[cfg(feature = "krites_sovereign_algos")]
#[path = "bfs_native.rs"]
pub(crate) mod bfs;

#[cfg(not(feature = "krites_sovereign_algos"))]
pub(crate) mod degree_centrality;
#[cfg(feature = "krites_sovereign_algos")]
#[path = "degree_centrality_native.rs"]
pub(crate) mod degree_centrality;

#[cfg(not(feature = "krites_sovereign_algos"))]
pub(crate) mod dfs;
#[cfg(feature = "krites_sovereign_algos")]
#[path = "dfs_native.rs"]
pub(crate) mod dfs;

#[cfg(not(feature = "krites_sovereign_algos"))]
pub(crate) mod kruskal;
#[cfg(feature = "krites_sovereign_algos")]
#[path = "kruskal_native.rs"]
pub(crate) mod kruskal;

#[cfg(not(feature = "krites_sovereign_algos"))]
pub(crate) mod label_propagation;
#[cfg(feature = "krites_sovereign_algos")]
#[path = "label_propagation_native.rs"]
pub(crate) mod label_propagation;

pub(crate) mod louvain;
pub(crate) mod pagerank;

#[cfg(not(feature = "krites_sovereign_algos"))]
pub(crate) mod prim;
#[cfg(feature = "krites_sovereign_algos")]
#[path = "prim_native.rs"]
pub(crate) mod prim;

#[cfg(not(feature = "krites_sovereign_algos"))]
pub(crate) mod random_walk;
#[cfg(feature = "krites_sovereign_algos")]
#[path = "random_walk_native.rs"]
pub(crate) mod random_walk;

#[cfg(not(feature = "krites_sovereign_algos"))]
pub(crate) mod shortest_path_bfs;
#[cfg(feature = "krites_sovereign_algos")]
#[path = "shortest_path_bfs_native.rs"]
pub(crate) mod shortest_path_bfs;

#[cfg(not(feature = "krites_sovereign_algos"))]
pub(crate) mod shortest_path_dijkstra;
#[cfg(feature = "krites_sovereign_algos")]
#[path = "shortest_path_dijkstra_native.rs"]
pub(crate) mod shortest_path_dijkstra;

#[cfg(not(feature = "krites_sovereign_algos"))]
pub(crate) mod strongly_connected_components;
#[cfg(feature = "krites_sovereign_algos")]
#[path = "strongly_connected_components_native.rs"]
pub(crate) mod strongly_connected_components;

#[cfg(not(feature = "krites_sovereign_algos"))]
pub(crate) mod top_sort;
#[cfg(feature = "krites_sovereign_algos")]
#[path = "top_sort_native.rs"]
pub(crate) mod top_sort;

#[cfg(not(feature = "krites_sovereign_algos"))]
pub(crate) mod triangles;
#[cfg(feature = "krites_sovereign_algos")]
#[path = "triangles_native.rs"]
pub(crate) mod triangles;

#[cfg(not(feature = "krites_sovereign_algos"))]
pub(crate) mod yen;
#[cfg(feature = "krites_sovereign_algos")]
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
