//! Typestate graph builder.
//!
//! [`GraphBuilder`] uses the typestate pattern to guide callers through graph
//! construction: the set of available methods changes as the builder accumulates
//! configuration, ensuring that `build()` is only callable once the graph source
//! (edges, file path, or GDL string) has been provided.
//!
//! The builder typestate types (`Uninitialized`, `FromEdges`, etc.) are public so
//! that they appear in rustdoc but are not intended for direct construction.

use std::{convert::TryFrom, marker::PhantomData};

use crate::{
    Error,
    graph::csr::{CsrLayout, NodeValues},
    index::Idx,
    input::{InputCapabilities, InputPath, edgelist::EdgeList},
    prelude::edgelist::{EdgeIterator, EdgeWithValueIterator},
};
use std::path::Path as StdPath;

/// Initial builder state — no graph source has been configured yet.
pub struct Uninitialized {
    csr_layout: CsrLayout,
}

/// Builder state after [`GraphBuilder::edges`] — holds unweighted edge tuples.
pub struct FromEdges<NI, Edges>
where
    NI: Idx,
    Edges: IntoIterator<Item = (NI, NI)>,
{
    csr_layout: CsrLayout,
    edges: Edges,
    _node: PhantomData<NI>,
}

/// Builder state after [`GraphBuilder::node_values`] with an edge list — holds
/// both edges and per-node values.
pub struct FromEdgeListAndNodeValues<NI, NV, EV>
where
    NI: Idx,
{
    csr_layout: CsrLayout,
    node_values: NodeValues<NV>,
    edge_list: EdgeList<NI, EV>,
}

/// Builder state after [`GraphBuilder::edges_with_values`] — holds weighted edge triplets.
pub struct FromEdgesWithValues<NI, Edges, EV>
where
    NI: Idx,
    Edges: IntoIterator<Item = (NI, NI, EV)>,
{
    csr_layout: CsrLayout,
    edges: Edges,
    _node: PhantomData<NI>,
}

/// Builder state after [`GraphBuilder::file_format`] — format selected, path not yet set.
pub struct FromInput<NI, P, Format>
where
    P: AsRef<StdPath>,
    NI: Idx,
    Format: InputCapabilities<NI>,
    Format::GraphInput: TryFrom<InputPath<P>>,
{
    csr_layout: CsrLayout,
    _idx: PhantomData<NI>,
    _path: PhantomData<P>,
    _format: PhantomData<Format>,
}

/// Builder state after [`GraphBuilder::path`] — ready to read and build from file.
pub struct FromPath<NI, P, Format>
where
    P: AsRef<StdPath>,
    NI: Idx,
    Format: InputCapabilities<NI>,
    Format::GraphInput: TryFrom<InputPath<P>>,
{
    csr_layout: CsrLayout,
    path: P,
    _idx: PhantomData<NI>,
    _format: PhantomData<Format>,
}

/// A builder to create graphs in a type-safe way.
///
/// The builder implementation uses different states to allow staged building of
/// graphs. Each individual state enables stage-specific methods on the builder.
///
/// # Examples
///
/// Create a directed graph from a vec of edges:
///
/// ```
/// use aletheia_graph_builder::prelude::*;
///
/// let graph: DirectedCsrGraph<usize> = GraphBuilder::new()
///     .edges(vec![(0, 1), (0, 2), (1, 2), (1, 3), (2, 3)])
///     .build();
///
/// assert_eq!(graph.node_count(), 4);
/// ```
///
/// Create an undirected graph from a vec of edges:
///
/// ```
/// use aletheia_graph_builder::prelude::*;
///
/// let graph: UndirectedCsrGraph<usize> = GraphBuilder::new()
///     .edges(vec![(0, 1), (0, 2), (1, 2), (1, 3), (2, 3)])
///     .build();
///
/// assert_eq!(graph.node_count(), 4);
/// ```
pub struct GraphBuilder<State> {
    state: State,
}

impl Default for GraphBuilder<Uninitialized> {
    fn default() -> Self {
        GraphBuilder::new()
    }
}

impl GraphBuilder<Uninitialized> {
    /// Creates a new builder
    pub fn new() -> Self {
        Self {
            state: Uninitialized {
                csr_layout: CsrLayout::default(),
            },
        }
    }

    /// Sets the [`CsrLayout`] to use during CSR construction.
    ///
    /// # Examples
    ///
    /// Store the neighbors sorted:
    ///
    /// ```
    /// use aletheia_graph_builder::prelude::*;
    ///
    /// let graph: UndirectedCsrGraph<usize> = GraphBuilder::new()
    ///     .csr_layout(CsrLayout::Sorted)
    ///     .edges(vec![(0, 7), (0, 3), (0, 3), (0, 1)])
    ///     .build();
    ///
    /// assert_eq!(graph.neighbors(0).copied().collect::<Vec<_>>(), &[1, 3, 3, 7]);
    /// ```
    ///
    /// Store the neighbors sorted and deduplicated:
    ///
    /// ```
    /// use aletheia_graph_builder::prelude::*;
    ///
    /// let graph: UndirectedCsrGraph<usize> = GraphBuilder::new()
    ///     .csr_layout(CsrLayout::Deduplicated)
    ///     .edges(vec![(0, 7), (0, 3), (0, 3), (0, 1)])
    ///     .build();
    ///
    /// assert_eq!(graph.neighbors(0).copied().collect::<Vec<_>>(), &[1, 3, 7]);
    /// ```
    #[must_use]
    pub fn csr_layout(mut self, csr_layout: CsrLayout) -> Self {
        self.state.csr_layout = csr_layout;
        self
    }

    /// Create a graph from the given edge tuples.
    ///
    /// # Example
    ///
    /// ```
    /// use aletheia_graph_builder::prelude::*;
    ///
    /// let graph: DirectedCsrGraph<usize> = GraphBuilder::new()
    ///     .edges(vec![(0, 1), (0, 2), (1, 2), (1, 3), (2, 3)])
    ///     .build();
    ///
    /// assert_eq!(graph.node_count(), 4);
    /// assert_eq!(graph.edge_count(), 5);
    /// ```
    pub fn edges<NI, Edges>(self, edges: Edges) -> GraphBuilder<FromEdges<NI, Edges>>
    where
        NI: Idx,
        Edges: IntoIterator<Item = (NI, NI)>,
    {
        GraphBuilder {
            state: FromEdges {
                csr_layout: self.state.csr_layout,
                edges,
                _node: PhantomData,
            },
        }
    }

    /// Create a graph from the given edge triplets.
    ///
    /// # Example
    ///
    /// ```
    /// use aletheia_graph_builder::prelude::*;
    ///
    /// let graph: DirectedCsrGraph<usize, (), f32> = GraphBuilder::new()
    ///     .edges_with_values(vec![(0, 1, 0.1), (0, 2, 0.2), (1, 2, 0.3), (1, 3, 0.4), (2, 3, 0.5)])
    ///     .build();
    ///
    /// assert_eq!(graph.node_count(), 4);
    /// assert_eq!(graph.edge_count(), 5);
    /// ```
    pub fn edges_with_values<NI, Edges, EV>(
        self,
        edges: Edges,
    ) -> GraphBuilder<FromEdgesWithValues<NI, Edges, EV>>
    where
        NI: Idx,
        Edges: IntoIterator<Item = (NI, NI, EV)>,
    {
        GraphBuilder {
            state: FromEdgesWithValues {
                csr_layout: self.state.csr_layout,
                edges,
                _node: PhantomData,
            },
        }
    }

    /// Creates a graph by reading it from the given file format.
    ///
    /// # Examples
    ///
    /// Read a directed graph from an edge list file:
    ///
    /// ```
    /// use std::path::PathBuf;
    ///
    /// use aletheia_graph_builder::prelude::*;
    ///
    /// let path = [env!("CARGO_MANIFEST_DIR"), "resources", "example.el"]
    ///     .iter()
    ///     .collect::<PathBuf>();
    ///
    /// let graph: DirectedCsrGraph<usize> = GraphBuilder::new()
    ///     .file_format(EdgeListInput::default())
    ///     .path(path)
    ///     .build()
    ///     .expect("loading failed");
    ///
    /// assert_eq!(graph.node_count(), 4);
    /// assert_eq!(graph.edge_count(), 5);
    /// ```
    pub fn file_format<Format, Path, NI>(
        self,
        _format: Format,
    ) -> GraphBuilder<FromInput<NI, Path, Format>>
    where
        Path: AsRef<StdPath>,
        NI: Idx,
        Format: InputCapabilities<NI>,
        Format::GraphInput: TryFrom<InputPath<Path>>,
    {
        GraphBuilder {
            state: FromInput {
                csr_layout: self.state.csr_layout,
                _idx: PhantomData,
                _path: PhantomData,
                _format: PhantomData,
            },
        }
    }
}

impl<NI, Edges> GraphBuilder<FromEdges<NI, Edges>>
where
    NI: Idx,
    Edges: IntoIterator<Item = (NI, NI)>,
{
    /// Attaches per-node values and transitions to the node-values builder state.
    ///
    /// `node_values` must yield exactly as many items as there are nodes in the graph.
    /// Node count is inferred from the maximum node id in the edge list.
    pub fn node_values<NV, I>(
        self,
        node_values: I,
    ) -> GraphBuilder<FromEdgeListAndNodeValues<NI, NV, ()>>
    where
        I: IntoIterator<Item = NV>,
    {
        let edge_list = EdgeList::from(EdgeIterator(self.state.edges));
        let node_values = node_values.into_iter().collect::<NodeValues<NV>>();

        GraphBuilder {
            state: FromEdgeListAndNodeValues {
                csr_layout: self.state.csr_layout,
                node_values,
                edge_list,
            },
        }
    }

    /// Build the graph from the configured unweighted edge set.
    ///
    /// The type parameter `Graph` determines the output — typically
    /// [`DirectedCsrGraph`](crate::prelude::DirectedCsrGraph) or
    /// [`UndirectedCsrGraph`](crate::prelude::UndirectedCsrGraph).
    pub fn build<Graph>(self) -> Graph
    where
        Graph: From<(EdgeList<NI, ()>, CsrLayout)>,
    {
        Graph::from((
            EdgeList::from(EdgeIterator(self.state.edges)),
            self.state.csr_layout,
        ))
    }
}

impl<NI, Edges, EV> GraphBuilder<FromEdgesWithValues<NI, Edges, EV>>
where
    NI: Idx,
    EV: Sync,
    Edges: IntoIterator<Item = (NI, NI, EV)>,
{
    /// Attaches per-node values to a weighted edge builder.
    ///
    /// `node_values` must yield exactly as many items as there are nodes.
    pub fn node_values<NV, I>(
        self,
        node_values: I,
    ) -> GraphBuilder<FromEdgeListAndNodeValues<NI, NV, EV>>
    where
        I: IntoIterator<Item = NV>,
    {
        let edge_list = EdgeList::from(EdgeWithValueIterator(self.state.edges));
        let node_values = node_values.into_iter().collect::<NodeValues<NV>>();

        GraphBuilder {
            state: FromEdgeListAndNodeValues {
                csr_layout: self.state.csr_layout,
                node_values,
                edge_list,
            },
        }
    }

    /// Build the graph from the given vec of edges.
    pub fn build<Graph>(self) -> Graph
    where
        Graph: From<(EdgeList<NI, EV>, CsrLayout)>,
    {
        Graph::from((
            EdgeList::new(self.state.edges.into_iter().collect()),
            self.state.csr_layout,
        ))
    }
}

impl<NI: Idx, NV, EV> GraphBuilder<FromEdgeListAndNodeValues<NI, NV, EV>> {
    /// Build the graph from the configured edge list and per-node values.
    ///
    /// Produces a [`DirectedCsrGraph`](crate::prelude::DirectedCsrGraph) or
    /// [`UndirectedCsrGraph`](crate::prelude::UndirectedCsrGraph) with attached
    /// node value storage.
    pub fn build<Graph>(self) -> Graph
    where
        Graph: From<(NodeValues<NV>, EdgeList<NI, EV>, CsrLayout)>,
    {
        Graph::from((
            self.state.node_values,
            self.state.edge_list,
            self.state.csr_layout,
        ))
    }
}

impl<NI, Path, Format> GraphBuilder<FromInput<NI, Path, Format>>
where
    Path: AsRef<StdPath>,
    NI: Idx,
    Format: InputCapabilities<NI>,
    Format::GraphInput: TryFrom<InputPath<Path>>,
{
    /// Set the location where the graph is stored.
    pub fn path(self, path: Path) -> GraphBuilder<FromPath<NI, Path, Format>> {
        GraphBuilder {
            state: FromPath {
                csr_layout: self.state.csr_layout,
                path,
                _idx: PhantomData,
                _format: PhantomData,
            },
        }
    }
}

impl<NI, Path, Format> GraphBuilder<FromPath<NI, Path, Format>>
where
    Path: AsRef<StdPath>,
    NI: Idx,
    Format: InputCapabilities<NI>,
    Format::GraphInput: TryFrom<InputPath<Path>>,
    crate::Error: From<<Format::GraphInput as TryFrom<InputPath<Path>>>::Error>,
{
    /// Build the graph by reading the configured file in the configured format.
    ///
    /// # Errors
    ///
    /// - [`crate::Error::IoError`] if the file cannot be opened or read.
    /// - [`crate::Error::InvalidIdType`] if the serialized index type does not match `NI`.
    /// - Other `Error` variants if the graph format conversion fails.
    pub fn build<Graph>(self) -> Result<Graph, Error>
    where
        Graph: TryFrom<(Format::GraphInput, CsrLayout)>,
        crate::Error: From<Graph::Error>,
    {
        let input = Format::GraphInput::try_from(InputPath(self.state.path))?;
        let graph = Graph::try_from((input, self.state.csr_layout))?;

        Ok(graph)
    }
}
