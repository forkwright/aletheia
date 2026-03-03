//! A library that can be used as a building block for high-performant graph
//! algorithms.
//!
//! Graph provides implementations for directed and undirected graphs. Graphs
//! can be created programatically or read from custom input formats in a
//! type-safe way. The library uses [rayon](https://github.com/rayon-rs/rayon)
//! to parallelize all steps during graph creation.
//!
//! The implementation uses a Compressed-Sparse-Row (CSR) data structure which
//! is tailored for fast and concurrent access to the graph topology.
//!
//! **Note**: The development is mainly driven by
//! [Neo4j](https://github.com/neo4j/neo4j) developers. However, the library is
//! __not__ an official product of Neo4j.
//!
//! # What is a graph?
//!
//! A graph consists of nodes and edges where edges connect exactly two nodes. A
//! graph can be either directed, i.e., an edge has a source and a target node
//! or undirected where there is no such distinction.
//!
//! In a directed graph, each node `u` has outgoing and incoming neighbors. An
//! outgoing neighbor of node `u` is any node `v` for which an edge `(u, v)`
//! exists. An incoming neighbor of node `u` is any node `v` for which an edge
//! `(v, u)` exists.
//!
//! In an undirected graph there is no distinction between source and target
//! node. A neighbor of node `u` is any node `v` for which either an edge `(u,
//! v)` or `(v, u)` exists.
//!
//! # How to build a graph
//!
//! The library provides a builder that can be used to construct a graph from a
//! given list of edges.
//!
//! For example, to create a directed graph that uses `usize` as node
//! identifier, one can use the builder like so:
//!
//! ```
//! use aletheia_graph_builder::prelude::*;
//!
//! let graph: DirectedCsrGraph<usize> = GraphBuilder::new()
//!     .edges(vec![(0, 1), (0, 2), (1, 2), (1, 3), (2, 3)])
//!     .build();
//!
//! assert_eq!(graph.node_count(), 4);
//! assert_eq!(graph.edge_count(), 5);
//!
//! assert_eq!(graph.out_degree(1), 2);
//! assert_eq!(graph.in_degree(1), 1);
//!
//! assert_eq!(graph.out_neighbors(1).as_slice(), &[2, 3]);
//! assert_eq!(graph.in_neighbors(1).as_slice(), &[0]);
//! ```
//!
//! To build an undirected graph using `u32` as node identifer, we only need to
//! change the expected types:
//!
//! ```
//! use aletheia_graph_builder::prelude::*;
//!
//! let graph: UndirectedCsrGraph<u32> = GraphBuilder::new()
//!     .csr_layout(CsrLayout::Sorted)
//!     .edges(vec![(0, 1), (0, 2), (1, 2), (1, 3), (2, 3)])
//!     .build();
//!
//! assert_eq!(graph.node_count(), 4);
//! assert_eq!(graph.edge_count(), 5);
//!
//! assert_eq!(graph.degree(1), 3);
//!
//! assert_eq!(graph.neighbors(1).as_slice(), &[0, 2, 3]);
//! ```
//!
//! Edges can have attached values to represent weighted graphs:
//!
//! ```
//! use aletheia_graph_builder::prelude::*;
//!
//! let graph: UndirectedCsrGraph<u32, (), f32> = GraphBuilder::new()
//!     .csr_layout(CsrLayout::Sorted)
//!     .edges_with_values(vec![(0, 1, 0.5), (0, 2, 0.7), (1, 2, 0.25), (1, 3, 1.0), (2, 3, 0.33)])
//!     .build();
//!
//! assert_eq!(graph.node_count(), 4);
//! assert_eq!(graph.edge_count(), 5);
//!
//! assert_eq!(graph.degree(1), 3);
//!
//! assert_eq!(
//!     graph.neighbors_with_values(1).as_slice(),
//!     &[Target::new(0, 0.5), Target::new(2, 0.25), Target::new(3, 1.0)]
//! );
//! ```
//!
//! It is also possible to create a graph from a specific input format. In the
//! following example we use the `EdgeListInput` which is an input format where
//! each line of a file contains an edge of the graph.
//!
//! ```
//! use std::path::PathBuf;
//!
//! use aletheia_graph_builder::prelude::*;
//!
//! let path = [env!("CARGO_MANIFEST_DIR"), "resources", "example.el"]
//!     .iter()
//!     .collect::<PathBuf>();
//!
//! let graph: DirectedCsrGraph<usize> = GraphBuilder::new()
//!     .csr_layout(CsrLayout::Sorted)
//!     .file_format(EdgeListInput::default())
//!     .path(path)
//!     .build()
//!     .expect("loading failed");
//!
//! assert_eq!(graph.node_count(), 4);
//! assert_eq!(graph.edge_count(), 5);
//!
//! assert_eq!(graph.out_degree(1), 2);
//! assert_eq!(graph.in_degree(1), 1);
//!
//! assert_eq!(graph.out_neighbors(1).as_slice(), &[2, 3]);
//! assert_eq!(graph.in_neighbors(1).as_slice(), &[0]);
//! ```
//!
//! The `EdgeListInput` format also supports weighted edges. This can be
//! controlled by a single type parameter on the graph type. Note, that the edge
//! value type needs to implement [`crate::input::ParseValue`].
//!
//! ```
//! use std::path::PathBuf;
//!
//! use aletheia_graph_builder::prelude::*;
//!
//! let path = [env!("CARGO_MANIFEST_DIR"), "resources", "example.wel"]
//!     .iter()
//!     .collect::<PathBuf>();
//!
//! let graph: DirectedCsrGraph<usize, (), f32> = GraphBuilder::new()
//!     .csr_layout(CsrLayout::Sorted)
//!     .file_format(EdgeListInput::default())
//!     .path(path)
//!     .build()
//!     .expect("loading failed");
//!
//! assert_eq!(graph.node_count(), 4);
//! assert_eq!(graph.edge_count(), 5);
//!
//! assert_eq!(graph.out_degree(1), 2);
//! assert_eq!(graph.in_degree(1), 1);
//!
//! assert_eq!(
//!     graph.out_neighbors_with_values(1).as_slice(),
//!     &[Target::new(2, 0.25), Target::new(3, 1.0)]
//! );
//! assert_eq!(
//!     graph.in_neighbors_with_values(1).as_slice(),
//!     &[Target::new(0, 0.5)]
//! );
//! ```
//!
//! # Types of graphs
//!
//! The crate currently ships with two graph implementations:
//!
//! ## Compressed Sparse Row (CSR)
//!
//! [CSR](https://en.wikipedia.org/wiki/Sparse_matrix#Compressed_sparse_row_(CSR,_CRS_or_Yale_format))
//! is a data structure used for representing a sparse matrix. Since graphs can be modelled as adjacency
//! matrix and are typically very sparse, i.e., not all possible pairs of nodes are connected
//! by an edge, the CSR representation is very well suited for representing a real-world graph topology.
//!
//! In our current implementation, we use two arrays to model the edges. One array stores the adjacency
//! lists for all nodes consecutively which requires `O(edge_count)` space. The other array stores the
//! offset for each node in the first array where the corresponding adjacency list can be found which
//! requires `O(node_count)` space. The degree of a node can be inferred from the offset array.
//!
//! Our CSR implementation is immutable, i.e., once built, the topology of the graph cannot be altered as
//! it would require inserting target ids and shifting all elements to the right which is expensive and
//! invalidates all offsets coming afterwards. However, building the CSR data structure from a list of
//! edges is implement very efficiently using multi-threading.
//!
//! However, due to inlining the all adjacency lists in one `Vec`, access becomes very cache-friendly,
//! as there is a chance that the adjacency list of the next node is already cached. Also, reading the
//! graph from multiple threads is safe, as there will be never be a concurrent mutable access.
//!
//! One can use [`DirectedCsrGraph`] or [`UndirectedCsrGraph`] to build a CSR-based graph:
//!
//! ```
//! use aletheia_graph_builder::prelude::*;
//!
//! let graph: DirectedCsrGraph<usize> = GraphBuilder::new()
//!     .edges(vec![(0, 1), (0, 2), (1, 2), (1, 3), (2, 3)])
//!     .build();
//!
//! assert_eq!(graph.node_count(), 4);
//! assert_eq!(graph.edge_count(), 5);
//!
//! assert_eq!(graph.out_degree(1), 2);
//! assert_eq!(graph.in_degree(1), 1);
//!
//! assert_eq!(graph.out_neighbors(1).as_slice(), &[2, 3]);
//! assert_eq!(graph.in_neighbors(1).as_slice(), &[0]);
//! ```
//!
pub mod builder;
pub mod graph;
pub mod graph_ops;
pub mod index;
pub mod input;
pub mod prelude;

pub use crate::builder::GraphBuilder;
pub use crate::graph::csr::CsrLayout;
pub use crate::graph::csr::DirectedCsrGraph;
pub use crate::graph::csr::UndirectedCsrGraph;

use crate::graph::Target;
use crate::index::Idx;
use snafu::Snafu;

// Manual From impls for absorbed crate compatibility — many internal call sites use `?`
// directly on io::Error and TryFromIntError. The standard snafu .context() pattern would
// require updating 20+ call sites in vendored code. These From impls preserve the absorbed
// crate's error conversion semantics while using snafu for the enum definition.
impl From<std::io::Error> for Error {
    fn from(source: std::io::Error) -> Self {
        Error::IoError {
            source,
            location: snafu::location!(),
        }
    }
}

impl From<std::num::TryFromIntError> for Error {
    fn from(source: std::num::TryFromIntError) -> Self {
        Error::IdxError {
            source,
            location: snafu::location!(),
        }
    }
}

// Required by GraphBuilder::build where Graph::Error = Infallible (non-file graph construction).
impl From<std::convert::Infallible> for Error {
    fn from(_: std::convert::Infallible) -> Self {
        unreachable!()
    }
}

/// Errors produced by graph construction and graph operations.
#[derive(Debug, Snafu)]
#[non_exhaustive]
pub enum Error {
    /// I/O failure while reading or writing a graph file.
    #[snafu(display("error while loading graph: {source}"))]
    IoError {
        source: std::io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },
    /// Node index type is too narrow for the actual node count.
    #[snafu(display("incompatible index type: {source}"))]
    IdxError {
        source: std::num::TryFromIntError,
        #[snafu(implicit)]
        location: snafu::Location,
    },
    /// The provided partition does not cover all nodes exactly once.
    #[snafu(display("invalid partitioning"))]
    InvalidPartitioning {
        #[snafu(implicit)]
        location: snafu::Location,
    },
    /// Node value slice length does not match the graph's node count.
    #[snafu(display("number of node values must be the same as node count"))]
    InvalidNodeValues {
        #[snafu(implicit)]
        location: snafu::Location,
    },
    /// Serialized graph used a different index type than the deserializer expects.
    #[snafu(display("invalid id size, expected {expected:?} bytes, got {actual:?} bytes"))]
    InvalidIdType {
        expected: String,
        actual: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
    /// The referenced node does not exist in the graph.
    #[snafu(display("node {node:?} does not exist in the graph"))]
    MissingNode {
        node: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

/// A graph is a tuple `(N, E)`, where `N` is a set of nodes and `E` a set of
/// edges. Each edge connects exactly two nodes.
///
/// `Graph` is parameterized over the node index type `Node` which is used to
/// uniquely identify a node. An edge is a tuple of node identifiers.
pub trait Graph<NI: Idx> {
    /// Returns the number of nodes in the graph.
    fn node_count(&self) -> NI;

    /// Returns the number of edges in the graph.
    fn edge_count(&self) -> NI;
}

/// A graph that allows storing a value per node.
pub trait NodeValues<NI: Idx, NV> {
    fn node_value(&self, node: NI) -> &NV;
}

/// Returns the degree of a node in an undirected graph.
pub trait UndirectedDegrees<NI: Idx> {
    /// Returns the number of edges connected to the given node.
    fn degree(&self, node: NI) -> NI;
}

/// Returns the neighbors of a given node.
///
/// The edge `(42, 1337)` is equivalent to the edge `(1337, 42)`.
pub trait UndirectedNeighbors<NI: Idx> {
    type NeighborsIterator<'a>: Iterator<Item = &'a NI>
    where
        Self: 'a;

    /// Returns an iterator of all nodes connected to the given node.
    fn neighbors(&self, node: NI) -> Self::NeighborsIterator<'_>;
}

/// Returns the neighbors of a given node.
///
/// The edge `(42, 1337)` is equivalent to the edge `(1337, 42)`.
pub trait UndirectedNeighborsWithValues<NI: Idx, EV> {
    type NeighborsIterator<'a>: Iterator<Item = &'a Target<NI, EV>>
    where
        Self: 'a,
        EV: 'a;

    /// Returns an iterator of all nodes connected to the given node
    /// including the value of the connecting edge.
    fn neighbors_with_values(&self, node: NI) -> Self::NeighborsIterator<'_>;
}

/// Returns the out-degree and in-degree of a node in a directed graph.
pub trait DirectedDegrees<NI: Idx> {
    /// Returns the number of edges where the given node is a source node.
    fn out_degree(&self, node: NI) -> NI;

    /// Returns the number of edges where the given node is a target node.
    fn in_degree(&self, node: NI) -> NI;
}

/// Returns the neighbors of a given node either in outgoing or incoming direction.
///
/// An edge tuple `e = (u, v)` has a source node `u` and a target node `v`. From
/// the perspective of `u`, the edge `e` is an **outgoing** edge. From the
/// perspective of node `v`, the edge `e` is an **incoming** edge. The edges
/// `(u, v)` and `(v, u)` are not considered equivalent.
pub trait DirectedNeighbors<NI: Idx> {
    type NeighborsIterator<'a>: Iterator<Item = &'a NI>
    where
        Self: 'a;

    /// Returns an iterator of all nodes which are connected in outgoing direction
    /// to the given node, i.e., the given node is the source node of the
    /// connecting edge.
    fn out_neighbors(&self, node: NI) -> Self::NeighborsIterator<'_>;

    /// Returns an iterator of all nodes which are connected in incoming direction
    /// to the given node, i.e., the given node is the target node of the
    /// connecting edge.
    fn in_neighbors(&self, node: NI) -> Self::NeighborsIterator<'_>;
}

/// Returns the neighbors of a given node either in outgoing or incoming direction.
///
/// An edge tuple `e = (u, v)` has a source node `u` and a target node `v`. From
/// the perspective of `u`, the edge `e` is an **outgoing** edge. From the
/// perspective of node `v`, the edge `e` is an **incoming** edge. The edges
/// `(u, v)` and `(v, u)` are not considered equivale
pub trait DirectedNeighborsWithValues<NI: Idx, EV> {
    type NeighborsIterator<'a>: Iterator<Item = &'a Target<NI, EV>>
    where
        Self: 'a,
        EV: 'a;

    /// Returns an iterator of all nodes which are connected in outgoing direction
    /// to the given node, i.e., the given node is the source node of the
    /// connecting edge. For each connected node, the value of the connecting
    /// edge is also returned.
    fn out_neighbors_with_values(&self, node: NI) -> Self::NeighborsIterator<'_>;

    /// Returns an iterator of all nodes which are connected in incoming direction
    /// to the given node, i.e., the given node is the target node of the
    /// connecting edge. For each connected node, the value of the connecting
    /// edge is also returned.
    fn in_neighbors_with_values(&self, node: NI) -> Self::NeighborsIterator<'_>;
}

/// Allows adding new edges to a graph.
pub trait EdgeMutation<NI: Idx> {
    /// Adds a new edge between the given source and target node.
    ///
    /// # Errors
    ///
    /// If either the source or the target node does not exist,
    /// the method will return [`Error::MissingNode`].
    fn add_edge(&self, source: NI, target: NI) -> Result<(), Error>;

    /// Adds a new edge between the given source and target node.
    ///
    /// Does not require locking the node-local list due to `&mut self`.
    ///
    /// # Errors
    ///
    /// If either the source or the target node does not exist,
    /// the method will return [`Error::MissingNode`].
    fn add_edge_mut(&mut self, source: NI, target: NI) -> Result<(), Error>;
}

/// Allows adding new edges to a graph.
pub trait EdgeMutationWithValues<NI: Idx, EV> {
    /// Adds a new edge between the given source and target node
    /// and assigns the given value to it.
    ///
    /// # Errors
    ///
    /// If either the source or the target node does not exist,
    /// the method will return [`Error::MissingNode`].
    fn add_edge_with_value(&self, source: NI, target: NI, value: EV) -> Result<(), Error>;

    /// Adds a new edge between the given source and target node
    /// and assigns the given value to it.
    ///
    /// Does not require locking the node-local list due to `&mut self`.
    ///
    /// # Errors
    ///
    /// If either the source or the target node does not exist,
    /// the method will return [`Error::MissingNode`].
    fn add_edge_with_value_mut(&mut self, source: NI, target: NI, value: EV) -> Result<(), Error>;
}

/// A transparent wrapper over `*mut T` that is `Send + Sync` when `T: Send + Sync`.
///
/// Used internally to share a raw pointer across rayon threads during parallel
/// CSR construction. Callers must guarantee disjoint write ranges to avoid
/// data races — no runtime enforcement is performed.
#[repr(transparent)]
pub struct SharedMut<T>(*mut T);
// SAFETY: SharedMut<T> is a #[repr(transparent)] newtype over *mut T. Raw pointers are
// !Send and !Sync by default, but SharedMut is used in parallel graph construction where
// disjoint index ranges are written by separate threads (no data races). We propagate
// T's Send bound, matching the contract of Arc<T>: the wrapper is Send iff T is Send.
unsafe impl<T: Send> Send for SharedMut<T> {}
// SAFETY: See Send impl above — same disjoint-write reasoning. Propagating T: Sync is
// correct because SharedMut grants shared access (via add + write) only when callers
// guarantee non-overlapping ranges.
unsafe impl<T: Sync> Sync for SharedMut<T> {}

impl<T> SharedMut<T> {
    /// Wraps a raw mutable pointer.
    ///
    /// # Safety
    ///
    /// The caller must ensure that all concurrent accesses through this wrapper
    /// target disjoint memory ranges for the lifetime of this value.
    pub fn new(ptr: *mut T) -> Self {
        SharedMut(ptr)
    }

    delegate::delegate! {
        to self.0 {
            /// # Safety
            ///
            /// Ensure that `count` does not exceed the capacity of the Vec.
            pub unsafe fn add(&self, count: usize) -> *mut T;
        }
    }
}
