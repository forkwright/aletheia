//! Cursor-based pagination types shared across all list endpoints.
//!
//! API.md: "Cursor-based pagination for unbounded collections. Query
//! parameters: `limit` and `after`. Response: include `has_more: true/false`
//! boolean and cursor for next page."

use serde::Serialize;
use utoipa::ToSchema;

/// Default page size when `limit` is omitted.
pub(crate) const DEFAULT_LIMIT: u32 = 50;

/// Maximum page size to prevent unbounded responses.
pub(crate) const MAX_LIMIT: u32 = 1000;

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

impl<T: Serialize> PaginatedResponse<T> {
    /// Build a paginated response from a full result set, applying cursor
    /// and limit. The `cursor_fn` extracts the cursor value from an item.
    ///
    /// WHY: Centralizes the skip-by-cursor, take-limit, compute-has_more
    /// logic so each handler does not re-implement it.
    pub(crate) fn from_vec<F>(
        mut items: Vec<T>,
        limit: u32,
        after: Option<&str>,
        cursor_fn: F,
        total: Option<u64>,
    ) -> Self
    where
        F: Fn(&T) -> String,
    {
        // WHY: If an `after` cursor is provided, skip items up to and including
        // the cursor position. This is offset-free: the cursor encodes the
        // identity of the last item seen.
        if let Some(cursor) = after
            && let Some(pos) = items.iter().position(|item| cursor_fn(item) == cursor)
        {
            items = items.split_off(pos + 1);
        }

        let limit = usize::try_from(limit).unwrap_or(usize::MAX);
        let has_more = items.len() > limit;
        items.truncate(limit);

        let next_cursor = if has_more {
            items.last().map(&cursor_fn)
        } else {
            None
        };

        Self {
            items,
            has_more,
            next_cursor,
            total,
        }
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn default_limit_is_50() {
        assert_eq!(DEFAULT_LIMIT, 50);
    }

    #[test]
    fn max_limit_is_1000() {
        assert_eq!(MAX_LIMIT, 1000);
    }

    #[test]
    fn from_vec_no_cursor_first_page() {
        let items: Vec<String> = (0..10).map(|i| format!("item-{i}")).collect();
        let resp = PaginatedResponse::from_vec(items, 3, None, |s| s.clone(), Some(10));
        assert_eq!(resp.items.len(), 3);
        assert!(resp.has_more);
        assert_eq!(resp.next_cursor.as_deref(), Some("item-2"));
        assert_eq!(resp.total, Some(10));
    }

    #[test]
    fn from_vec_with_cursor_skips_past_it() {
        let items: Vec<String> = (0..10).map(|i| format!("item-{i}")).collect();
        let resp = PaginatedResponse::from_vec(items, 3, Some("item-2"), |s| s.clone(), Some(10));
        assert_eq!(resp.items.len(), 3);
        assert_eq!(resp.items[0], "item-3");
        assert!(resp.has_more);
        assert_eq!(resp.next_cursor.as_deref(), Some("item-5"));
    }

    #[test]
    fn from_vec_last_page_no_more() {
        let items: Vec<String> = (0..5).map(|i| format!("item-{i}")).collect();
        let resp = PaginatedResponse::from_vec(items, 10, None, |s| s.clone(), Some(5));
        assert_eq!(resp.items.len(), 5);
        assert!(!resp.has_more);
        assert!(resp.next_cursor.is_none());
    }

    #[test]
    fn from_vec_cursor_not_found_returns_all() {
        let items: Vec<String> = (0..3).map(|i| format!("item-{i}")).collect();
        let resp = PaginatedResponse::from_vec(items, 10, Some("nonexistent"), |s| s.clone(), None);
        assert_eq!(resp.items.len(), 3);
        assert!(!resp.has_more);
    }

    #[test]
    fn from_vec_empty_input() {
        let items: Vec<String> = Vec::new();
        let resp = PaginatedResponse::from_vec(items, 10, None, |s| s.clone(), Some(0));
        assert!(resp.items.is_empty());
        assert!(!resp.has_more);
        assert!(resp.next_cursor.is_none());
    }

    #[test]
    fn paginated_response_serialization() {
        let resp = PaginatedResponse {
            items: vec!["a", "b"],
            has_more: true,
            next_cursor: Some("b".to_owned()),
            total: Some(10),
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["items"].as_array().unwrap().len(), 2);
        assert_eq!(json["has_more"], true);
        assert_eq!(json["next_cursor"], "b");
        assert_eq!(json["total"], 10);
    }

    #[test]
    fn paginated_response_omits_null_fields() {
        let resp: PaginatedResponse<String> = PaginatedResponse {
            items: vec![],
            has_more: false,
            next_cursor: None,
            total: None,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert!(json.get("next_cursor").is_none());
        assert!(json.get("total").is_none());
    }
}
