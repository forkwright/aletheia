//! Graph input format abstractions.
//!
//! Defines [`InputCapabilities`] and [`ParseValue`] traits for pluggable input formats,
//! the [`Direction`] enum for edge orientation, and re-exports the built-in
//! [`EdgeListInput`] / [`EdgeList`] types from the [`edgelist`] submodule.

pub mod edgelist;

pub use edgelist::EdgeList;
pub use edgelist::EdgeListInput;
pub use edgelist::Edges;

use crate::index::Idx;

/// Wraps a filesystem path for use as a graph input source.
///
/// Passed to [`crate::builder::GraphBuilder::path`] after selecting a file format
/// via [`crate::builder::GraphBuilder::file_format`].
pub struct InputPath<P>(pub(crate) P);

/// Implemented by input format types to declare the in-memory representation they produce.
///
/// Plug in a custom format by implementing `InputCapabilities<NI>` and the corresponding
/// `TryFrom<InputPath<P>>` for `Self::GraphInput`.
pub trait InputCapabilities<NI: Idx> {
    /// The in-memory graph input produced by this format (e.g., [`crate::input::EdgeList`]).
    type GraphInput;
}

/// Edge direction used during CSR construction.
///
/// Controls which side of each edge is indexed in the degree/offset arrays:
/// - `Outgoing` — index source nodes (for directed out-neighbor lookups)
/// - `Incoming` — index target nodes (for directed in-neighbor lookups)
/// - `Undirected` — index both sides (each edge stored twice)
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub enum Direction {
    /// Index source → target edges (out-neighbors).
    Outgoing,
    /// Index target → source edges (in-neighbors).
    Incoming,
    /// Index both directions; each edge appears in both neighbor lists.
    Undirected,
}

/// Used by input formats to read node or edge values from bytes.
pub trait ParseValue: Default + Sized {
    /// Parses a value from a slice.
    ///
    /// # Example
    ///
    /// ```
    /// use aletheia_graph_builder::input::ParseValue;
    ///
    /// let bytes = "13.37".as_bytes();
    ///
    /// let (number, len) = f32::parse(bytes);
    ///
    /// assert_eq!(number, 13.37);
    /// assert_eq!(len, 5);
    /// ```
    ///
    /// # Return
    ///
    /// Returns a tuple containing two entries. The first is the parsed value,
    /// the second is the index of the byte right after the parsed value.
    fn parse(bytes: &[u8]) -> (Self, usize);
}

impl ParseValue for () {
    fn parse(_bytes: &[u8]) -> (Self, usize) {
        ((), 0)
    }
}

macro_rules! impl_parse_value {
    ($atoi:path, $($ty:ty),+ $(,)?) => {
        $(
            impl $crate::input::ParseValue for $ty {
                fn parse(bytes: &[u8]) -> (Self, usize) {
                    if bytes.len() == 0 {
                        (<$ty as ::std::default::Default>::default(), 0)
                    } else {
                        $atoi(bytes)
                    }
                }
            }
        )+
    };
}

impl_parse_value!(
    ::atoi::FromRadix10::from_radix_10,
    u8,
    u16,
    u32,
    u64,
    u128,
    usize,
);

impl_parse_value!(
    ::atoi::FromRadix10Signed::from_radix_10_signed,
    i8,
    i16,
    i32,
    i64,
    i128,
    isize,
);

impl_parse_value!(parse_float, f32, f64);

fn parse_float<T: fast_float2::FastFloat>(bytes: &[u8]) -> (T, usize) {
    fast_float2::parse_partial(bytes)
        .expect("invariant: parse_float called only on validated numeric byte sequences")
}
