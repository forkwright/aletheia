//! Graph data structures.
//!
//! Re-exports [`csr`] (the CSR implementations) and defines [`Target`] — the
//! edge endpoint type used throughout the graph traversal API.

pub mod csr;

/// Represents the target of an edge and its associated value.
///
/// Returned by [`crate::DirectedNeighborsWithValues`] and
/// [`crate::UndirectedNeighborsWithValues`] iterators. The `#[repr(C)]` layout
/// is required for zero-copy serialization and transmutation between
/// `Target<NI, ()>` and bare `NI` slices.
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct Target<NI, EV> {
    /// The neighbor node id.
    pub target: NI,
    /// The value associated with the connecting edge.
    pub value: EV,
}

impl<T: Ord, V> Ord for Target<T, V> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.target.cmp(&other.target)
    }
}

impl<T: PartialOrd, V> PartialOrd for Target<T, V> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.target.partial_cmp(&other.target)
    }
}

impl<T: PartialEq, V> PartialEq for Target<T, V> {
    fn eq(&self, other: &Self) -> bool {
        self.target.eq(&other.target)
    }
}

impl<T: Eq, V> Eq for Target<T, V> {}

impl<T, EV> Target<T, EV> {
    /// Creates a new `Target` with the given node id and edge value.
    pub fn new(target: T, value: EV) -> Self {
        Self { target, value }
    }
}
