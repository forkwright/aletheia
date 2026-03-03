//! CSR (Compressed Sparse Row) graph implementations.
//!
//! Provides [`DirectedCsrGraph`] and [`UndirectedCsrGraph`] — the two concrete
//! graph types built by [`crate::builder::GraphBuilder`]. Also defines
//! [`CsrLayout`] (neighbor ordering), [`NodeValues`] (per-node metadata), and
//! the internal [`Csr`] data structure.

use atomic::Atomic;
use byte_slice_cast::{AsByteSlice, AsMutByteSlice, ToByteSlice, ToMutByteSlice};
use log::info;
use std::{
    convert::TryFrom,
    fs::File,
    io::{BufReader, Read, Write},
    iter::FromIterator,
    mem::{ManuallyDrop, MaybeUninit},
    path::PathBuf,
    sync::atomic::Ordering::Acquire,
    time::Instant,
};

use rayon::prelude::*;

use crate::{
    DirectedDegrees, DirectedNeighbors, DirectedNeighborsWithValues, Error, Graph,
    NodeValues as NodeValuesTrait, SharedMut, Target, UndirectedDegrees, UndirectedNeighbors,
    UndirectedNeighborsWithValues,
    graph_ops::{DeserializeGraphOp, SerializeGraphOp, ToUndirectedOp},
    index::Idx,
    input::{Direction, edgelist::Edges},
};

/// Defines how the neighbor list of individual nodes are organized within the
/// CSR target array.
///
/// Pass to [`crate::builder::GraphBuilder`]`.csr_layout()` before calling `build()`.
/// The default is [`CsrLayout::Unsorted`] which minimizes construction time.
#[derive(Default, Clone, Copy, Debug)]
#[non_exhaustive]
pub enum CsrLayout {
    /// Neighbor lists are sorted and may contain duplicate target ids. This is
    /// the default representation.
    Sorted,
    /// Neighbor lists are not in any particular order.
    #[default]
    Unsorted,
    /// Neighbor lists are sorted and do not contain duplicate target ids.
    /// Self-loops, i.e., edges in the form of `(u, u)` are removed.
    Deduplicated,
}

/// A Compressed-Sparse-Row data structure to represent sparse graphs.
///
/// The data structure is composed of two arrays: `offsets` and `targets`. For a
/// graph with node count `n` and edge count `m`, `offsets` has exactly `n + 1`
/// and `targets` exactly `m` entries.
///
/// For a given node `u`, `offsets[u]` stores the start index of the neighbor
/// list of `u` in `targets`. The degree of `u`, i.e., the length of the
/// neighbor list is defined by `offsets[u + 1] - offsets[u]`. The neighbor list
/// of `u` is defined by the slice `&targets[offsets[u]..offsets[u + 1]]`.
#[derive(Debug)]
pub struct Csr<Index: Idx, NI, EV> {
    offsets: Box<[Index]>,
    targets: Box<[Target<NI, EV>]>,
}

impl<Index: Idx, NI, EV> Csr<Index, NI, EV> {
    pub(crate) fn new(offsets: Box<[Index]>, targets: Box<[Target<NI, EV>]>) -> Self {
        Self { offsets, targets }
    }

    #[inline]
    pub(crate) fn node_count(&self) -> Index {
        Index::new(self.offsets.len() - 1)
    }

    #[inline]
    pub(crate) fn edge_count(&self) -> Index {
        Index::new(self.targets.len())
    }

    #[inline]
    pub(crate) fn degree(&self, i: Index) -> Index {
        let from = self.offsets[i.index()];
        let to = self.offsets[(i + Index::new(1)).index()];

        to - from
    }

    #[inline]
    pub(crate) fn targets_with_values(&self, i: Index) -> &[Target<NI, EV>] {
        let from = self.offsets[i.index()];
        let to = self.offsets[(i + Index::new(1)).index()];

        &self.targets[from.index()..to.index()]
    }
}

impl<Index: Idx, NI> Csr<Index, NI, ()> {
    #[inline]
    pub(crate) fn targets(&self, i: Index) -> &[NI] {
        assert_eq!(
            std::mem::size_of::<Target<NI, ()>>(),
            std::mem::size_of::<NI>()
        );
        assert_eq!(
            std::mem::align_of::<Target<NI, ()>>(),
            std::mem::align_of::<NI>()
        );
        let from = self.offsets[i.index()];
        let to = self.offsets[(i + Index::new(1)).index()];

        let len = (to - from).index();

        let targets = &self.targets[from.index()..to.index()];

        // SAFETY: len is within bounds as it is calculated above as `to - from`.
        //         The types Target<T, ()> and T are verified to have the same
        //         size and alignment.
        unsafe { std::slice::from_raw_parts(targets.as_ptr() as *const _, len) }
    }
}

/// Replaces the internal CSR representation of a graph in place.
///
/// Used by [`crate::graph_ops::RelabelByDegreeOp`] to swap in a degree-ordered
/// CSR after relabeling without allocating a new graph struct.
pub trait SwapCsr<Index: Idx, NI, EV> {
    /// Replaces the internal CSR with `csr` and returns `&mut self`.
    fn swap_csr(&mut self, csr: Csr<Index, NI, EV>) -> &mut Self;
}

impl<NI, EV, E> From<(&'_ E, NI, Direction, CsrLayout)> for Csr<NI, NI, EV>
where
    NI: Idx,
    EV: Copy + Send + Sync,
    E: Edges<NI = NI, EV = EV>,
{
    fn from(
        (edge_list, node_count, direction, csr_layout): (&'_ E, NI, Direction, CsrLayout),
    ) -> Self {
        let start = Instant::now();
        let degrees = edge_list.degrees(node_count, direction);
        info!("Computed degrees in {:?}", start.elapsed());

        let start = Instant::now();
        let offsets = prefix_sum_atomic(degrees);
        info!("Computed prefix sum in {:?}", start.elapsed());

        let start = Instant::now();
        let edge_count = offsets[node_count.index()].load(Acquire).index();
        let mut targets = Vec::<Target<NI, EV>>::with_capacity(edge_count);
        let targets_ptr = SharedMut::new(targets.as_mut_ptr());

        // The following loop writes all targets into their correct position.
        // The offsets are a prefix sum of all degrees, which will produce
        // non-overlapping positions for all node values.
        //
        // SAFETY: for any (s, t) tuple from the same edge_list we use the
        // prefix_sum to find a unique position for the target value, so that we
        // only write once into each position and every thread that might run
        // will write into different positions.
        if matches!(direction, Direction::Outgoing | Direction::Undirected) {
            edge_list.edges().for_each(|(s, t, v)| {
                let offset = NI::get_and_increment(&offsets[s.index()], Acquire);

                unsafe {
                    targets_ptr.add(offset.index()).write(Target::new(t, v));
                }
            })
        }

        if matches!(direction, Direction::Incoming | Direction::Undirected) {
            edge_list.edges().for_each(|(s, t, v)| {
                let offset = NI::get_and_increment(&offsets[t.index()], Acquire);

                unsafe {
                    targets_ptr.add(offset.index()).write(Target::new(s, v));
                }
            })
        }

        // SAFETY: The previous loops iterated the input edge list once (twice
        // for undirected) and inserted one node id for each edge. The
        // `edge_count` is defined by the highest offset value.
        unsafe {
            targets.set_len(edge_count);
        }
        info!("Computed target array in {:?}", start.elapsed());

        let start = Instant::now();
        let mut offsets = ManuallyDrop::new(offsets);
        let (ptr, len, cap) = (offsets.as_mut_ptr(), offsets.len(), offsets.capacity());

        // SAFETY: NI and NI::Atomic have the same memory layout
        let mut offsets = unsafe {
            let ptr = ptr as *mut _;
            Vec::from_raw_parts(ptr, len, cap)
        };

        // Each insert into the target array in the previous loops incremented
        // the offset for the corresponding node by one. As a consequence the
        // offset values are shifted one index to the right. We need to correct
        // this in order to get correct offsets.
        offsets.rotate_right(1);
        offsets[0] = NI::zero();
        info!("Finalized offset array in {:?}", start.elapsed());

        let (offsets, targets) = match csr_layout {
            CsrLayout::Unsorted => (offsets, targets),
            CsrLayout::Sorted => {
                let start = Instant::now();
                sort_targets(&offsets, &mut targets);
                info!("Sorted targets in {:?}", start.elapsed());
                (offsets, targets)
            }
            CsrLayout::Deduplicated => {
                let start = Instant::now();
                let offsets_targets = sort_and_deduplicate_targets(&offsets, &mut targets[..]);
                info!("Sorted and deduplicated targets in {:?}", start.elapsed());
                offsets_targets
            }
        };

        Csr {
            offsets: offsets.into_boxed_slice(),
            targets: targets.into_boxed_slice(),
        }
    }
}

unsafe impl<NI, EV> ToByteSlice for Target<NI, EV>
where
    NI: ToByteSlice,
    EV: ToByteSlice,
{
    fn to_byte_slice<S: AsRef<[Self]> + ?Sized>(slice: &S) -> &[u8] {
        let slice = slice.as_ref();
        let len = std::mem::size_of_val(slice);
        // SAFETY: Target<NI, EV> is a #[repr(C)] struct whose fields (NI, EV) bound by
        // ToByteSlice, which requires Copy + no padding. size_of_val gives the exact byte
        // count of the slice. The pointer is valid for that many bytes and u8 alignment is
        // always met.
        unsafe { std::slice::from_raw_parts(slice.as_ptr() as *const u8, len) }
    }
}

unsafe impl<NI, EV> ToMutByteSlice for Target<NI, EV>
where
    NI: ToMutByteSlice,
    EV: ToMutByteSlice,
{
    fn to_mut_byte_slice<S: AsMut<[Self]> + ?Sized>(slice: &mut S) -> &mut [u8] {
        let slice = slice.as_mut();
        let len = std::mem::size_of_val(slice);
        // SAFETY: Same as ToByteSlice — Target<NI, EV> has #[repr(C)] layout with no padding
        // (NI and EV bound by ToMutByteSlice). The mutable pointer is valid for size_of_val
        // bytes within the slice's allocation, and u8 alignment is always satisfied.
        unsafe { std::slice::from_raw_parts_mut(slice.as_mut_ptr() as *mut u8, len) }
    }
}

impl<NI, EV> Csr<NI, NI, EV>
where
    NI: Idx + ToByteSlice,
    EV: ToByteSlice,
{
    fn serialize<W: Write>(&self, output: &mut W) -> Result<(), Error> {
        let type_name = std::any::type_name::<NI>().as_bytes();
        output.write_all([type_name.len()].as_byte_slice())?;
        output.write_all(type_name)?;

        let node_count = self.node_count();
        let edge_count = self.edge_count();
        let meta = [node_count, edge_count];
        output.write_all(meta.as_byte_slice())?;

        output.write_all(self.offsets.as_byte_slice())?;
        output.write_all(self.targets.as_byte_slice())?;

        Ok(())
    }
}

impl<NI, EV> Csr<NI, NI, EV>
where
    NI: Idx + ToMutByteSlice,
    EV: ToMutByteSlice,
{
    fn deserialize<R: Read>(read: &mut R) -> Result<Csr<NI, NI, EV>, Error> {
        let mut type_name_len = [0_usize; 1];
        read.read_exact(type_name_len.as_mut_byte_slice())?;
        let [type_name_len] = type_name_len;

        let mut type_name = vec![0_u8; type_name_len];
        read.read_exact(type_name.as_mut_byte_slice())?;
        let type_name = String::from_utf8(type_name).expect("could not read type name");

        let expected_type_name = std::any::type_name::<NI>().to_string();

        if type_name != expected_type_name {
            return Err(Error::InvalidIdType {
                expected: expected_type_name,
                actual: type_name,
                location: snafu::location!(),
            });
        }

        let mut meta = [NI::zero(); 2];
        read.read_exact(meta.as_mut_byte_slice())?;

        let [node_count, edge_count] = meta;

        let mut offsets = Box::<[_]>::new_uninit_slice(node_count.index() + 1);
        let offsets_ptr = offsets.as_mut_ptr() as *mut NI;
        // SAFETY: `new_uninit_slice(node_count + 1)` allocates exactly `node_count + 1`
        // MaybeUninit<NI> elements. Casting to *mut NI and creating a slice of the same
        // length gives a valid mutable reference into the allocation.
        let offsets_ptr =
            unsafe { std::slice::from_raw_parts_mut(offsets_ptr, node_count.index() + 1) };
        read.read_exact(offsets_ptr.as_mut_byte_slice())?;

        let mut targets = Box::<[_]>::new_uninit_slice(edge_count.index());
        let targets_ptr = targets.as_mut_ptr() as *mut Target<NI, EV>;
        // SAFETY: Same as offsets — `new_uninit_slice(edge_count)` allocates exactly
        // edge_count MaybeUninit<Target<NI, EV>> elements; pointer and length match.
        let targets_ptr =
            unsafe { std::slice::from_raw_parts_mut(targets_ptr, edge_count.index()) };
        read.read_exact(targets_ptr.as_mut_byte_slice())?;

        // SAFETY: Both slices were fully initialized by `read_exact` which writes exactly
        // `size_of_val(&*offsets_ptr)` / `size_of_val(&*targets_ptr)` bytes, filling every
        // element in the MaybeUninit slices.
        let offsets = unsafe { offsets.assume_init() };
        let targets = unsafe { targets.assume_init() };

        Ok(Csr::new(offsets, targets))
    }
}

/// Per-node value storage, indexed by node id.
///
/// Stores one value of type `NV` per node. Used with [`DirectedCsrGraph`] and
/// [`UndirectedCsrGraph`] when graphs carry node-level metadata (e.g., labels,
/// weights, or feature vectors). Access via
/// [`crate::NodeValues::node_value`].
pub struct NodeValues<NV>(pub(crate) Box<[NV]>);

impl<NV> NodeValues<NV> {
    /// Creates a `NodeValues` container from a `Vec` of per-node values.
    ///
    /// The vector must have exactly as many elements as there are nodes in the
    /// graph — this is not checked at construction time but enforced when the
    /// graph is built.
    pub fn new(node_values: Vec<NV>) -> Self {
        Self(node_values.into_boxed_slice())
    }
}

impl<NV> FromIterator<NV> for NodeValues<NV> {
    fn from_iter<T: IntoIterator<Item = NV>>(iter: T) -> Self {
        Self(iter.into_iter().collect::<Vec<_>>().into_boxed_slice())
    }
}

impl<NV> NodeValues<NV>
where
    NV: ToByteSlice,
{
    fn serialize<W: Write>(&self, output: &mut W) -> Result<(), Error> {
        let node_count = self.0.len();
        let meta = [node_count];
        output.write_all(meta.as_byte_slice())?;
        output.write_all(self.0.as_byte_slice())?;
        Ok(())
    }
}

impl<NV> NodeValues<NV>
where
    NV: ToMutByteSlice,
{
    fn deserialize<R: Read>(read: &mut R) -> Result<Self, Error> {
        let mut meta = [0_usize; 1];
        read.read_exact(meta.as_mut_byte_slice())?;
        let [node_count] = meta;

        let mut node_values = Box::<[_]>::new_uninit_slice(node_count);
        let node_values_ptr = node_values.as_mut_ptr() as *mut NV;
        // SAFETY: `new_uninit_slice(node_count)` allocates exactly node_count MaybeUninit<NV>
        // elements. Pointer and length match the allocation; cast to *mut NV is valid because
        // MaybeUninit<NV> has the same layout as NV.
        let node_values_slice =
            unsafe { std::slice::from_raw_parts_mut(node_values_ptr, node_count.index()) };
        read.read_exact(node_values_slice.as_mut_byte_slice())?;

        // SAFETY: `read_exact` initialized all bytes in the node_values slice, filling every
        // MaybeUninit<NV> element with valid data read from the serialized stream.
        let offsets = unsafe { node_values.assume_init() };

        Ok(NodeValues(offsets))
    }
}

/// A directed graph backed by two CSR structures — one for outgoing, one for incoming edges.
///
/// Type parameters:
/// - `NI` — node index type (e.g. `u32`, `usize`); must implement [`crate::index::Idx`]
/// - `NV` — per-node value type (default `()`); access via [`crate::NodeValues::node_value`]
/// - `EV` — per-edge value type (default `()`); access via [`crate::DirectedNeighborsWithValues`]
///
/// Build with [`crate::builder::GraphBuilder`]:
/// ```
/// use aletheia_graph_builder::prelude::*;
///
/// let graph: DirectedCsrGraph<u32> = GraphBuilder::new()
///     .edges(vec![(0, 1), (1, 2)])
///     .build();
/// assert_eq!(graph.node_count(), 3);
/// ```
pub struct DirectedCsrGraph<NI: Idx, NV = (), EV = ()> {
    node_values: NodeValues<NV>,
    csr_out: Csr<NI, NI, EV>,
    csr_inc: Csr<NI, NI, EV>,
}

impl<NI: Idx, NV, EV> DirectedCsrGraph<NI, NV, EV> {
    /// Creates a directed graph from pre-built CSR arrays and node values.
    ///
    /// Prefer [`crate::builder::GraphBuilder`] for constructing graphs from edge lists.
    /// This constructor is intended for advanced use cases where the CSR data is
    /// already available (e.g., after deserialization).
    pub fn new(
        node_values: NodeValues<NV>,
        csr_out: Csr<NI, NI, EV>,
        csr_inc: Csr<NI, NI, EV>,
    ) -> Self {
        let g = Self {
            node_values,
            csr_out,
            csr_inc,
        };
        info!(
            "Created directed graph (node_count = {:?}, edge_count = {:?})",
            g.node_count(),
            g.edge_count()
        );

        g
    }
}

impl<NI, NV, EV> ToUndirectedOp for DirectedCsrGraph<NI, NV, EV>
where
    NI: Idx,
    NV: Clone + Send + Sync,
    EV: Copy + Send + Sync,
{
    type Undirected = UndirectedCsrGraph<NI, NV, EV>;

    fn to_undirected(&self, layout: impl Into<Option<CsrLayout>>) -> Self::Undirected {
        let node_values = NodeValues::new(self.node_values.0.to_vec());
        let layout = layout.into().unwrap_or_default();
        let edges = ToUndirectedEdges { g: self };

        UndirectedCsrGraph::from((node_values, edges, layout))
    }
}

struct ToUndirectedEdges<'g, NI: Idx, NV, EV> {
    g: &'g DirectedCsrGraph<NI, NV, EV>,
}

impl<NI, NV, EV> Edges for ToUndirectedEdges<'_, NI, NV, EV>
where
    NI: Idx,
    NV: Send + Sync,
    EV: Copy + Send + Sync,
{
    type NI = NI;

    type EV = EV;

    type EdgeIter<'a>
        = ToUndirectedEdgesIter<'a, NI, NV, EV>
    where
        Self: 'a;

    fn edges(&self) -> Self::EdgeIter<'_> {
        ToUndirectedEdgesIter { g: self.g }
    }

    fn max_node_id(&self) -> Self::NI {
        self.g.node_count() - NI::new(1)
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        panic!("ToUndirectedEdges is not used in test contexts that call len()")
    }
}

struct ToUndirectedEdgesIter<'g, NI: Idx, NV, EV> {
    g: &'g DirectedCsrGraph<NI, NV, EV>,
}

impl<NI: Idx, NV: Send + Sync, EV: Copy + Send + Sync> ParallelIterator
    for ToUndirectedEdgesIter<'_, NI, NV, EV>
{
    type Item = (NI, NI, EV);

    fn drive_unindexed<C>(self, consumer: C) -> C::Result
    where
        C: rayon::iter::plumbing::UnindexedConsumer<Self::Item>,
    {
        let par_iter = (0..self.g.node_count().index())
            .into_par_iter()
            .flat_map_iter(|n| {
                let n = NI::new(n);
                self.g
                    .out_neighbors_with_values(n)
                    .map(move |t| (n, t.target, t.value))
            });
        par_iter.drive_unindexed(consumer)
    }
}

impl<NI: Idx, NV, EV> Graph<NI> for DirectedCsrGraph<NI, NV, EV> {
    delegate::delegate! {
        to self.csr_out {
            fn node_count(&self) -> NI;
            fn edge_count(&self) -> NI;
        }
    }
}

impl<NI: Idx, NV, EV> NodeValuesTrait<NI, NV> for DirectedCsrGraph<NI, NV, EV> {
    fn node_value(&self, node: NI) -> &NV {
        &self.node_values.0[node.index()]
    }
}

impl<NI: Idx, NV, EV> DirectedDegrees<NI> for DirectedCsrGraph<NI, NV, EV> {
    fn out_degree(&self, node: NI) -> NI {
        self.csr_out.degree(node)
    }

    fn in_degree(&self, node: NI) -> NI {
        self.csr_inc.degree(node)
    }
}

impl<NI: Idx, NV> DirectedNeighbors<NI> for DirectedCsrGraph<NI, NV, ()> {
    type NeighborsIterator<'a>
        = std::slice::Iter<'a, NI>
    where
        NV: 'a;

    fn out_neighbors(&self, node: NI) -> Self::NeighborsIterator<'_> {
        self.csr_out.targets(node).iter()
    }

    fn in_neighbors(&self, node: NI) -> Self::NeighborsIterator<'_> {
        self.csr_inc.targets(node).iter()
    }
}

impl<NI: Idx, NV, EV> DirectedNeighborsWithValues<NI, EV> for DirectedCsrGraph<NI, NV, EV> {
    type NeighborsIterator<'a>
        = std::slice::Iter<'a, Target<NI, EV>>
    where
        NV: 'a,
        EV: 'a;

    fn out_neighbors_with_values(&self, node: NI) -> Self::NeighborsIterator<'_> {
        self.csr_out.targets_with_values(node).iter()
    }

    fn in_neighbors_with_values(&self, node: NI) -> Self::NeighborsIterator<'_> {
        self.csr_inc.targets_with_values(node).iter()
    }
}

impl<NI, EV, E> From<(E, CsrLayout)> for DirectedCsrGraph<NI, (), EV>
where
    NI: Idx,
    EV: Copy + Send + Sync,
    E: Edges<NI = NI, EV = EV>,
{
    fn from((edge_list, csr_option): (E, CsrLayout)) -> Self {
        info!("Creating directed graph");
        let node_count = edge_list.max_node_id() + NI::new(1);

        let node_values = NodeValues::new(vec![(); node_count.index()]);

        let start = Instant::now();
        let csr_out = Csr::from((&edge_list, node_count, Direction::Outgoing, csr_option));
        info!("Created outgoing csr in {:?}.", start.elapsed());

        let start = Instant::now();
        let csr_inc = Csr::from((&edge_list, node_count, Direction::Incoming, csr_option));
        info!("Created incoming csr in {:?}.", start.elapsed());

        DirectedCsrGraph::new(node_values, csr_out, csr_inc)
    }
}

impl<NI, NV, EV, E> From<(NodeValues<NV>, E, CsrLayout)> for DirectedCsrGraph<NI, NV, EV>
where
    NI: Idx,
    EV: Copy + Send + Sync,
    E: Edges<NI = NI, EV = EV>,
{
    fn from((node_values, edge_list, csr_option): (NodeValues<NV>, E, CsrLayout)) -> Self {
        info!("Creating directed graph");
        let node_count = NI::new(node_values.0.len());
        let node_count_from_edge_list = edge_list.max_node_id() + NI::new(1);

        assert!(
            node_count >= node_count_from_edge_list,
            "number of node values ({}) does not match node count of edge list ({})",
            node_count.index(),
            node_count_from_edge_list.index()
        );

        let start = Instant::now();
        let csr_out = Csr::from((&edge_list, node_count, Direction::Outgoing, csr_option));
        info!("Created outgoing csr in {:?}.", start.elapsed());

        let start = Instant::now();
        let csr_inc = Csr::from((&edge_list, node_count, Direction::Incoming, csr_option));
        info!("Created incoming csr in {:?}.", start.elapsed());

        DirectedCsrGraph::new(node_values, csr_out, csr_inc)
    }
}

impl<W, NI, NV, EV> SerializeGraphOp<W> for DirectedCsrGraph<NI, NV, EV>
where
    W: Write,
    NI: Idx + ToByteSlice,
    NV: ToByteSlice,
    EV: ToByteSlice,
{
    fn serialize(&self, mut output: W) -> Result<(), Error> {
        let DirectedCsrGraph {
            node_values,
            csr_out,
            csr_inc,
        } = self;

        node_values.serialize(&mut output)?;
        csr_out.serialize(&mut output)?;
        csr_inc.serialize(&mut output)?;

        Ok(())
    }
}

impl<R, NI, NV, EV> DeserializeGraphOp<R, Self> for DirectedCsrGraph<NI, NV, EV>
where
    R: Read,
    NI: Idx + ToMutByteSlice,
    NV: ToMutByteSlice,
    EV: ToMutByteSlice,
{
    fn deserialize(mut read: R) -> Result<Self, Error> {
        let node_values: NodeValues<NV> = NodeValues::deserialize(&mut read)?;
        let csr_out: Csr<NI, NI, EV> = Csr::deserialize(&mut read)?;
        let csr_inc: Csr<NI, NI, EV> = Csr::deserialize(&mut read)?;
        Ok(DirectedCsrGraph::new(node_values, csr_out, csr_inc))
    }
}

impl<NI, EV> TryFrom<(PathBuf, CsrLayout)> for DirectedCsrGraph<NI, EV>
where
    NI: Idx + ToMutByteSlice,
    EV: ToMutByteSlice,
{
    type Error = Error;

    fn try_from((path, _): (PathBuf, CsrLayout)) -> Result<Self, Self::Error> {
        let reader = BufReader::new(File::open(path)?);
        let graph = DirectedCsrGraph::deserialize(reader)?;

        Ok(graph)
    }
}

/// An undirected graph backed by a single CSR structure.
///
/// Each edge `(u, v)` is stored twice — as `(u, v)` and `(v, u)` — so that
/// neighbor lookups work symmetrically. [`Graph::edge_count`] returns the
/// logical edge count (half of the stored target count).
///
/// Type parameters:
/// - `NI` — node index type (e.g. `u32`, `usize`); must implement [`crate::index::Idx`]
/// - `NV` — per-node value type (default `()`)
/// - `EV` — per-edge value type (default `()`)
///
/// Build with [`crate::builder::GraphBuilder`]:
/// ```
/// use aletheia_graph_builder::prelude::*;
///
/// let graph: UndirectedCsrGraph<u32> = GraphBuilder::new()
///     .edges(vec![(0, 1), (1, 2)])
///     .build();
/// assert_eq!(graph.node_count(), 3);
/// assert_eq!(graph.degree(1), 2);
/// ```
pub struct UndirectedCsrGraph<NI: Idx, NV = (), EV = ()> {
    node_values: NodeValues<NV>,
    csr: Csr<NI, NI, EV>,
}

impl<NI: Idx, EV> From<Csr<NI, NI, EV>> for UndirectedCsrGraph<NI, (), EV> {
    fn from(csr: Csr<NI, NI, EV>) -> Self {
        UndirectedCsrGraph::new(NodeValues::new(vec![(); csr.node_count().index()]), csr)
    }
}

impl<NI: Idx, NV, EV> UndirectedCsrGraph<NI, NV, EV> {
    /// Creates an undirected graph from pre-built CSR arrays and node values.
    ///
    /// Prefer [`crate::builder::GraphBuilder`] for constructing graphs from edge lists.
    pub fn new(node_values: NodeValues<NV>, csr: Csr<NI, NI, EV>) -> Self {
        let g = Self { node_values, csr };
        info!(
            "Created undirected graph (node_count = {:?}, edge_count = {:?})",
            g.node_count(),
            g.edge_count()
        );

        g
    }
}

impl<NI: Idx, NV, EV> Graph<NI> for UndirectedCsrGraph<NI, NV, EV> {
    fn node_count(&self) -> NI {
        self.csr.node_count()
    }

    fn edge_count(&self) -> NI {
        self.csr.edge_count() / NI::new(2)
    }
}

impl<NI: Idx, NV, EV> NodeValuesTrait<NI, NV> for UndirectedCsrGraph<NI, NV, EV> {
    fn node_value(&self, node: NI) -> &NV {
        &self.node_values.0[node.index()]
    }
}

impl<NI: Idx, NV, EV> UndirectedDegrees<NI> for UndirectedCsrGraph<NI, NV, EV> {
    fn degree(&self, node: NI) -> NI {
        self.csr.degree(node)
    }
}

impl<NI: Idx, NV> UndirectedNeighbors<NI> for UndirectedCsrGraph<NI, NV> {
    type NeighborsIterator<'a>
        = std::slice::Iter<'a, NI>
    where
        NV: 'a;

    fn neighbors(&self, node: NI) -> Self::NeighborsIterator<'_> {
        self.csr.targets(node).iter()
    }
}

impl<NI: Idx, NV, EV> UndirectedNeighborsWithValues<NI, EV> for UndirectedCsrGraph<NI, NV, EV> {
    type NeighborsIterator<'a>
        = std::slice::Iter<'a, Target<NI, EV>>
    where
        NV: 'a,
        EV: 'a;

    fn neighbors_with_values(&self, node: NI) -> Self::NeighborsIterator<'_> {
        self.csr.targets_with_values(node).iter()
    }
}

impl<NI: Idx, NV, EV> SwapCsr<NI, NI, EV> for UndirectedCsrGraph<NI, NV, EV> {
    fn swap_csr(&mut self, mut csr: Csr<NI, NI, EV>) -> &mut Self {
        std::mem::swap(&mut self.csr, &mut csr);
        self
    }
}

impl<NI, EV, E> From<(E, CsrLayout)> for UndirectedCsrGraph<NI, (), EV>
where
    NI: Idx,
    EV: Copy + Send + Sync,
    E: Edges<NI = NI, EV = EV>,
{
    fn from((edge_list, csr_option): (E, CsrLayout)) -> Self {
        info!("Creating undirected graph");
        let node_count = edge_list.max_node_id() + NI::new(1);

        let node_values = NodeValues::new(vec![(); node_count.index()]);

        let start = Instant::now();
        let csr = Csr::from((&edge_list, node_count, Direction::Undirected, csr_option));
        info!("Created csr in {:?}.", start.elapsed());

        UndirectedCsrGraph::new(node_values, csr)
    }
}

impl<NI, NV, EV, E> From<(NodeValues<NV>, E, CsrLayout)> for UndirectedCsrGraph<NI, NV, EV>
where
    NI: Idx,
    EV: Copy + Send + Sync,
    E: Edges<NI = NI, EV = EV>,
{
    fn from((node_values, edge_list, csr_option): (NodeValues<NV>, E, CsrLayout)) -> Self {
        info!("Creating undirected graph");
        let node_count = NI::new(node_values.0.len());
        let node_count_from_edge_list = edge_list.max_node_id() + NI::new(1);

        assert!(
            node_count >= node_count_from_edge_list,
            "number of node values ({}) does not match node count of edge list ({})",
            node_count.index(),
            node_count_from_edge_list.index()
        );

        let start = Instant::now();
        let csr = Csr::from((&edge_list, node_count, Direction::Undirected, csr_option));
        info!("Created csr in {:?}.", start.elapsed());

        UndirectedCsrGraph::new(node_values, csr)
    }
}

impl<W, NI, NV, EV> SerializeGraphOp<W> for UndirectedCsrGraph<NI, NV, EV>
where
    W: Write,
    NI: Idx + ToByteSlice,
    NV: ToByteSlice,
    EV: ToByteSlice,
{
    fn serialize(&self, mut output: W) -> Result<(), Error> {
        let UndirectedCsrGraph { node_values, csr } = self;

        node_values.serialize(&mut output)?;
        csr.serialize(&mut output)?;

        Ok(())
    }
}

impl<R, NI, NV, EV> DeserializeGraphOp<R, Self> for UndirectedCsrGraph<NI, NV, EV>
where
    R: Read,
    NI: Idx + ToMutByteSlice,
    NV: ToMutByteSlice,
    EV: ToMutByteSlice,
{
    fn deserialize(mut read: R) -> Result<Self, Error> {
        let node_values = NodeValues::deserialize(&mut read)?;
        let csr: Csr<NI, NI, EV> = Csr::deserialize(&mut read)?;
        Ok(UndirectedCsrGraph::new(node_values, csr))
    }
}

impl<NI, EV> TryFrom<(PathBuf, CsrLayout)> for UndirectedCsrGraph<NI, EV>
where
    NI: Idx + ToMutByteSlice,
    EV: ToMutByteSlice,
{
    type Error = Error;

    fn try_from((path, _): (PathBuf, CsrLayout)) -> Result<Self, Self::Error> {
        let reader = BufReader::new(File::open(path)?);
        UndirectedCsrGraph::deserialize(reader)
    }
}

fn prefix_sum_atomic<NI: Idx>(degrees: Vec<Atomic<NI>>) -> Vec<Atomic<NI>> {
    let mut last = degrees
        .last()
        .expect("invariant: prefix_sum_atomic called with non-empty degree vec")
        .load(Acquire);
    let mut sums = degrees
        .into_iter()
        .scan(NI::zero(), |total, degree| {
            let value = *total;
            *total += degree.into_inner();
            Some(Atomic::new(value))
        })
        .collect::<Vec<_>>();

    last += sums
        .last()
        .expect("invariant: sums is non-empty after scanning non-empty degrees")
        .load(Acquire);
    sums.push(Atomic::new(last));

    sums
}

pub(crate) fn prefix_sum<NI: Idx>(degrees: Vec<NI>) -> Vec<NI> {
    let mut last = *degrees
        .last()
        .expect("invariant: prefix_sum called with non-empty degree vec");
    let mut sums = degrees
        .into_iter()
        .scan(NI::zero(), |total, degree| {
            let value = *total;
            *total += degree;
            Some(value)
        })
        .collect::<Vec<_>>();
    last += *sums
        .last()
        .expect("invariant: sums is non-empty after scanning non-empty degrees");
    sums.push(last);
    sums
}

pub(crate) fn sort_targets<NI, T, EV>(offsets: &[NI], targets: &mut [Target<T, EV>])
where
    NI: Idx,
    T: Copy + Send + Ord,
    EV: Send,
{
    to_mut_slices(offsets, targets)
        .par_iter_mut()
        .for_each(|list| list.sort_unstable());
}

/// Stable equivalent of the unstable `slice::partition_dedup`. Returns the count of unique elements.
/// Unique elements are moved to the front; duplicates remain at the tail (not necessarily valid).
fn partition_dedup_by_eq<T: PartialEq>(slice: &mut [T]) -> usize {
    let len = slice.len();
    if len <= 1 {
        return len;
    }
    let ptr = slice.as_mut_ptr();
    let mut next_read: usize = 1;
    let mut next_write: usize = 1;
    // SAFETY: Both `next_read` and `next_write` are bounded by `len` (the loop terminates
    // when `next_read >= len`). The invariant `next_write <= next_read` is maintained
    // throughout (we only advance next_write when a new unique element is found), so
    // `ptr.add(next_read)` and `ptr.add(next_write)` are always within the slice allocation.
    // `ptr::read` and `ptr::swap` operate on non-overlapping positions because next_write <
    // next_read when copying occurs.
    unsafe {
        while next_read < len {
            let ptr_read = ptr.add(next_read);
            let prev_ptr_write = ptr.add(next_write - 1);
            if *ptr_read != *prev_ptr_write {
                if next_read != next_write {
                    let ptr_write = prev_ptr_write.offset(1);
                    core::ptr::swap(ptr_read, ptr_write);
                }
                next_write += 1;
            }
            next_read += 1;
        }
    }
    next_write
}

fn sort_and_deduplicate_targets<NI, EV>(
    offsets: &[NI],
    targets: &mut [Target<NI, EV>],
) -> (Vec<NI>, Vec<Target<NI, EV>>)
where
    NI: Idx,
    EV: Copy + Send,
{
    let node_count = offsets.len() - 1;

    let mut new_degrees = Vec::with_capacity(node_count);
    let mut target_slices = to_mut_slices(offsets, targets);

    target_slices
        .par_iter_mut()
        .enumerate()
        .map(|(node, slice)| {
            slice.sort_unstable();
            // deduplicate — partition_dedup is unstable; inline equivalent:
            let dedup_len = partition_dedup_by_eq(slice);
            let dedup = &mut slice[..dedup_len];
            let mut new_degree = dedup.len();
            // remove self loops .. there is at most once occurence of node inside dedup
            if let Ok(idx) = dedup.binary_search_by_key(&NI::new(node), |t| t.target) {
                dedup[idx..].rotate_left(1);
                new_degree -= 1;
            }
            NI::new(new_degree)
        })
        .collect_into_vec(&mut new_degrees);

    let new_offsets = prefix_sum(new_degrees);
    debug_assert_eq!(new_offsets.len(), node_count + 1);

    let edge_count = new_offsets[node_count].index();
    let mut new_targets: Vec<Target<NI, EV>> = Vec::with_capacity(edge_count);
    let new_target_slices = to_mut_slices(&new_offsets, new_targets.spare_capacity_mut());

    target_slices
        .into_par_iter()
        .zip(new_target_slices.into_par_iter())
        .for_each(|(old_slice, new_slice)| {
            // MaybeUninit::write_slice is unstable; use transmute-based copy
            // SAFETY: &[T] and &[MaybeUninit<T>] have the same layout
            let uninit_src: &[MaybeUninit<_>] =
                unsafe { std::mem::transmute(&old_slice[..new_slice.len()]) };
            new_slice.copy_from_slice(uninit_src);
        });

    // SAFETY: We copied all (potentially shortened) target ids from the old
    // target list to the new one.
    unsafe {
        new_targets.set_len(edge_count);
    }

    (new_offsets, new_targets)
}

fn to_mut_slices<'targets, NI: Idx, T>(
    offsets: &[NI],
    targets: &'targets mut [T],
) -> Vec<&'targets mut [T]> {
    let node_count = offsets.len() - 1;
    let mut target_slices = Vec::with_capacity(node_count);
    let mut tail = targets;
    let mut prev_offset = offsets[0];

    for &offset in &offsets[1..] {
        let (list, remainder) = tail.split_at_mut((offset - prev_offset).index());
        target_slices.push(list);
        tail = remainder;
        prev_offset = offset;
    }

    target_slices
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Seek, SeekFrom},
        sync::atomic::Ordering::SeqCst,
    };

    use rayon::ThreadPoolBuilder;

    use crate::builder::GraphBuilder;

    use super::*;

    #[test]
    fn to_mut_slices_test() {
        let offsets = &[0, 2, 5, 5, 8];
        let targets = &mut [0, 1, 2, 3, 4, 5, 6, 7];
        let slices = to_mut_slices::<usize, usize>(offsets, targets);

        assert_eq!(
            slices,
            vec![vec![0, 1], vec![2, 3, 4], vec![], vec![5, 6, 7]]
        );
    }

    fn t<T>(t: T) -> Target<T, ()> {
        Target::new(t, ())
    }

    #[test]
    fn sort_targets_test() {
        let offsets = &[0, 2, 5, 5, 8];
        let mut targets = vec![t(1), t(0), t(4), t(2), t(3), t(5), t(6), t(7)];
        sort_targets::<usize, _, _>(offsets, &mut targets);

        assert_eq!(
            targets,
            vec![t(0), t(1), t(2), t(3), t(4), t(5), t(6), t(7)]
        );
    }

    #[test]
    fn sort_and_deduplicate_targets_test() {
        let offsets = &[0, 3, 7, 7, 10];
        // 0: [1, 1, 0]    => [1] (removed duplicate and self loop)
        // 1: [4, 2, 3, 2] => [2, 3, 4] (removed duplicate)
        let mut targets = vec![t(1), t(1), t(0), t(4), t(2), t(3), t(2), t(5), t(6), t(7)];
        let (offsets, targets) = sort_and_deduplicate_targets::<usize, _>(offsets, &mut targets);

        assert_eq!(offsets, vec![0, 1, 4, 4, 7]);
        assert_eq!(targets, vec![t(1), t(2), t(3), t(4), t(5), t(6), t(7)]);
    }

    #[test]
    fn prefix_sum_test() {
        let degrees = vec![42, 0, 1337, 4, 2, 0];
        let prefix_sum = prefix_sum::<usize>(degrees);

        assert_eq!(prefix_sum, vec![0, 42, 42, 1379, 1383, 1385, 1385]);
    }

    #[test]
    fn prefix_sum_atomic_test() {
        let degrees = vec![42, 0, 1337, 4, 2, 0]
            .into_iter()
            .map(Atomic::<usize>::new)
            .collect::<Vec<_>>();

        let prefix_sum = prefix_sum_atomic(degrees)
            .into_iter()
            .map(|n| n.load(SeqCst))
            .collect::<Vec<_>>();

        assert_eq!(prefix_sum, vec![0, 42, 42, 1379, 1383, 1385, 1385]);
    }

    #[test]
    fn serialize_directed_usize_graph_test() {
        let mut file = tempfile::tempfile().unwrap();

        let g0: DirectedCsrGraph<usize> = GraphBuilder::new()
            .edges(vec![(0, 1), (0, 2), (1, 2), (1, 3), (2, 3), (3, 1)])
            .build();

        assert!(g0.serialize(&file).is_ok());

        file.seek(SeekFrom::Start(0)).unwrap();
        let g1 = DirectedCsrGraph::<usize>::deserialize(file).unwrap();

        assert_eq!(g0.node_count(), g1.node_count());
        assert_eq!(g0.edge_count(), g1.edge_count());

        assert_eq!(
            g0.out_neighbors(0).as_slice(),
            g1.out_neighbors(0).as_slice()
        );
        assert_eq!(
            g0.out_neighbors(1).as_slice(),
            g1.out_neighbors(1).as_slice()
        );
        assert_eq!(
            g0.out_neighbors(2).as_slice(),
            g1.out_neighbors(2).as_slice()
        );
        assert_eq!(
            g0.out_neighbors(3).as_slice(),
            g1.out_neighbors(3).as_slice()
        );

        assert_eq!(g0.in_neighbors(0).as_slice(), g1.in_neighbors(0).as_slice());
        assert_eq!(g0.in_neighbors(1).as_slice(), g1.in_neighbors(1).as_slice());
        assert_eq!(g0.in_neighbors(2).as_slice(), g1.in_neighbors(2).as_slice());
        assert_eq!(g0.in_neighbors(3).as_slice(), g1.in_neighbors(3).as_slice());
    }

    #[test]
    fn serialize_undirected_usize_graph_test() {
        let mut file = tempfile::tempfile().unwrap();

        let g0: UndirectedCsrGraph<usize> = GraphBuilder::new()
            .edges(vec![(0, 1), (0, 2), (1, 2), (1, 3), (2, 3), (3, 1)])
            .build();

        assert!(g0.serialize(&file).is_ok());

        file.seek(SeekFrom::Start(0)).unwrap();

        let g1 = UndirectedCsrGraph::<usize>::deserialize(file).unwrap();

        assert_eq!(g0.node_count(), g1.node_count());
        assert_eq!(g0.edge_count(), g1.edge_count());

        assert_eq!(g0.neighbors(0).as_slice(), g1.neighbors(0).as_slice());
        assert_eq!(g0.neighbors(1).as_slice(), g1.neighbors(1).as_slice());
        assert_eq!(g0.neighbors(2).as_slice(), g1.neighbors(2).as_slice());
        assert_eq!(g0.neighbors(3).as_slice(), g1.neighbors(3).as_slice());
    }

    #[test]
    fn serialize_directed_u32_graph_test() {
        let mut file = tempfile::tempfile().unwrap();

        let g0: DirectedCsrGraph<u32> = GraphBuilder::new()
            .edges(vec![(0, 1), (0, 2), (1, 2), (1, 3), (2, 3), (3, 1)])
            .build();

        assert!(g0.serialize(&file).is_ok());

        file.seek(SeekFrom::Start(0)).unwrap();
        let g1 = DirectedCsrGraph::<u32>::deserialize(file).unwrap();

        assert_eq!(g0.node_count(), g1.node_count());
        assert_eq!(g0.edge_count(), g1.edge_count());

        assert_eq!(
            g0.out_neighbors(0).as_slice(),
            g1.out_neighbors(0).as_slice()
        );
        assert_eq!(
            g0.out_neighbors(1).as_slice(),
            g1.out_neighbors(1).as_slice()
        );
        assert_eq!(
            g0.out_neighbors(2).as_slice(),
            g1.out_neighbors(2).as_slice()
        );
        assert_eq!(
            g0.out_neighbors(3).as_slice(),
            g1.out_neighbors(3).as_slice()
        );

        assert_eq!(g0.in_neighbors(0).as_slice(), g1.in_neighbors(0).as_slice());
        assert_eq!(g0.in_neighbors(1).as_slice(), g1.in_neighbors(1).as_slice());
        assert_eq!(g0.in_neighbors(2).as_slice(), g1.in_neighbors(2).as_slice());
        assert_eq!(g0.in_neighbors(3).as_slice(), g1.in_neighbors(3).as_slice());
    }

    #[test]
    fn serialize_undirected_u32_graph_test() {
        let mut file = tempfile::tempfile().unwrap();

        let g0: UndirectedCsrGraph<u32> = GraphBuilder::new()
            .edges(vec![(0, 1), (0, 2), (1, 2), (1, 3), (2, 3), (3, 1)])
            .build();

        assert!(g0.serialize(&file).is_ok());

        file.seek(SeekFrom::Start(0)).unwrap();

        let g1 = UndirectedCsrGraph::<u32>::deserialize(file).unwrap();

        assert_eq!(g0.node_count(), g1.node_count());
        assert_eq!(g0.edge_count(), g1.edge_count());

        assert_eq!(g0.neighbors(0).as_slice(), g1.neighbors(0).as_slice());
        assert_eq!(g0.neighbors(1).as_slice(), g1.neighbors(1).as_slice());
        assert_eq!(g0.neighbors(2).as_slice(), g1.neighbors(2).as_slice());
        assert_eq!(g0.neighbors(3).as_slice(), g1.neighbors(3).as_slice());
    }

    #[test]
    fn serialize_invalid_id_size() {
        let mut file = tempfile::tempfile().unwrap();

        let g0: UndirectedCsrGraph<u32> = GraphBuilder::new()
            .edges(vec![(0, 1), (0, 2), (1, 2), (1, 3), (2, 3), (3, 1)])
            .build();

        assert!(g0.serialize(&file).is_ok());

        file.seek(SeekFrom::Start(0)).unwrap();

        let res: Result<UndirectedCsrGraph<usize>, Error> =
            UndirectedCsrGraph::<usize>::deserialize(file);

        assert!(res.is_err());

        assert!(matches!(res, Err(Error::InvalidIdType { .. })));
    }

    #[test]
    fn test_to_undirected() {
        // we need a deterministic order of loading, so we're doing stuff in serial
        let pool = ThreadPoolBuilder::new().num_threads(1).build().unwrap();
        pool.install(|| {
            let g: DirectedCsrGraph<u32> = GraphBuilder::new()
                .edges(vec![(0, 1), (3, 0), (0, 3), (7, 0), (0, 42), (21, 0)])
                .build();

            let ug = g.to_undirected(None);
            assert_eq!(ug.degree(0), 6);
            assert_eq!(ug.neighbors(0).as_slice(), &[1, 3, 42, 3, 7, 21]);

            let ug = g.to_undirected(CsrLayout::Unsorted);
            assert_eq!(ug.degree(0), 6);
            assert_eq!(ug.neighbors(0).as_slice(), &[1, 3, 42, 3, 7, 21]);

            let ug = g.to_undirected(CsrLayout::Sorted);
            assert_eq!(ug.degree(0), 6);
            assert_eq!(ug.neighbors(0).as_slice(), &[1, 3, 3, 7, 21, 42]);

            let ug = g.to_undirected(CsrLayout::Deduplicated);
            assert_eq!(ug.degree(0), 5);
            assert_eq!(ug.neighbors(0).as_slice(), &[1, 3, 7, 21, 42]);
        });
    }

    #[test]
    fn directed_from_node_values_exceeding_edge_list_max_id() {
        let g0: DirectedCsrGraph<u32, u32> = GraphBuilder::new()
            .edges(vec![(0, 1), (1, 2)])
            .node_values(vec![0, 1, 2, 3])
            .build();

        assert_eq!(g0.node_count(), 4);
        for node in 0..4 {
            assert_eq!(g0.node_value(node), &node);
        }

        assert_eq!(g0.out_degree(0), 1);
        assert_eq!(g0.out_degree(1), 1);
        assert_eq!(g0.out_degree(2), 0);
        assert_eq!(g0.out_degree(3), 0);
    }

    #[test]
    fn undirected_from_node_values_exceeding_edge_list_max_id() {
        let g0: UndirectedCsrGraph<u32, u32> = GraphBuilder::new()
            .edges(vec![(0, 1), (1, 2)])
            .node_values(vec![0, 1, 2, 3])
            .build();

        assert_eq!(g0.node_count(), 4);
        for node in 0..4 {
            assert_eq!(g0.node_value(node), &node);
        }

        assert_eq!(g0.degree(0), 1);
        assert_eq!(g0.degree(1), 2);
        assert_eq!(g0.degree(2), 1);
        assert_eq!(g0.degree(3), 0);
    }
}
