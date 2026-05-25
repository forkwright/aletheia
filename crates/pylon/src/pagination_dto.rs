// WHY: wire DTO
//! Pagination response wire shapes.

use serde::Serialize;
use utoipa::ToSchema;

/// Standard pagination envelope for all list endpoints.
///
/// Wraps a `Vec<T>` with metadata so clients can page through results
/// with a single, consistent implementation.
#[derive(Debug, Serialize, ToSchema)]
pub struct PaginatedResponse<T> {
    /// The items in this page.
    pub items: Vec<T>,
    /// Whether more items exist beyond this page.
    pub has_more: bool,
    /// Cursor to pass as `after` to fetch the next page.
    /// `None` when `has_more` is `false`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    /// Total count of matching items, when cheap to compute.
    /// `None` when the total is unknown or expensive to calculate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
}
