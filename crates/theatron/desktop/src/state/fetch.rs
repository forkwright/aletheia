//! Generic async fetch state for view components.

/// Lifecycle state for an async data fetch.
///
/// Used by views that load data from the API on mount and on refresh.
/// The type parameter `T` is the successfully loaded payload.
#[derive(Debug, Clone)]
pub(crate) enum FetchState<T> {
    /// No fetch attempted yet, or fetch is in progress.
    Loading,
    /// Data loaded successfully.
    Loaded(T),
    /// Fetch failed with an error message.
    Error(String),
}
