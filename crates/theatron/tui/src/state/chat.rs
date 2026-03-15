use std::ops::{Deref, DerefMut};
use std::sync::Arc;

/// Shared-ownership message history vector.
///
/// `Clone` increments an `Arc` reference count — O(1) regardless of message count.
/// `DerefMut` uses `Arc::make_mut` for copy-on-write semantics: mutation is free
/// when this is the sole owner, and clones the `Vec` exactly once when another
/// `Arc` still holds a reference.
///
/// PERF: Tab switches call `ArcVec::clone` (O(1)) instead of `Vec::clone` (O(n)).
#[derive(Debug)]
pub(crate) struct ArcVec<T>(Arc<Vec<T>>);

impl<T> Clone for ArcVec<T> {
    fn clone(&self) -> Self {
        ArcVec(Arc::clone(&self.0))
    }
}

impl<T> Default for ArcVec<T> {
    fn default() -> Self {
        ArcVec(Arc::new(Vec::new()))
    }
}

impl<T> Deref for ArcVec<T> {
    type Target = Vec<T>;

    fn deref(&self) -> &Vec<T> {
        &self.0
    }
}

impl<T: Clone> DerefMut for ArcVec<T> {
    fn deref_mut(&mut self) -> &mut Vec<T> {
        Arc::make_mut(&mut self.0)
    }
}

impl<T: Clone> FromIterator<T> for ArcVec<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        ArcVec(Arc::new(iter.into_iter().collect()))
    }
}

impl<T: Clone> From<Vec<T>> for ArcVec<T> {
    fn from(v: Vec<T>) -> Self {
        ArcVec(Arc::new(v))
    }
}

#[derive(Debug, Clone)]
pub struct ToolCallInfo {
    pub name: String,
    pub duration_ms: Option<u64>,
    pub is_error: bool,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct SavedScrollState {
    pub(crate) scroll_offset: usize,
    pub(crate) auto_scroll: bool,
}

#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub text: String,
    /// Pre-lowercased `text`, cached at ingestion to avoid per-frame allocation in view code.
    pub text_lower: String,
    pub timestamp: Option<String>,
    pub model: Option<String>,
    #[expect(dead_code, reason = "set during streaming, used for future rendering")]
    pub is_streaming: bool,
    pub tool_calls: Vec<ToolCallInfo>,
}

/// Paired streaming-markdown text and its pre-rendered line cache.
///
/// Invariant: `lines` always corresponds to the rendering of `text`.
/// Both fields are cleared or updated together via [`MarkdownCache::clear`].
#[derive(Debug, Clone, Default)]
pub(crate) struct MarkdownCache {
    pub(crate) text: String,
    pub(crate) lines: Vec<ratatui::text::Line<'static>>,
}

impl MarkdownCache {
    /// Clear both the cached text and rendered lines atomically.
    pub(crate) fn clear(&mut self) {
        self.text.clear();
        self.lines.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arc_vec_clone_is_shared_not_copied() {
        let v: ArcVec<i32> = vec![1, 2, 3].into();
        let v2 = v.clone();
        // Both point to the same allocation — pointer equality.
        assert!(std::ptr::eq(Arc::as_ptr(&v.0), Arc::as_ptr(&v2.0)));
    }

    #[test]
    fn arc_vec_deref_mut_unshares_on_write() {
        let v: ArcVec<i32> = vec![1, 2, 3].into();
        let mut v2 = v.clone();
        // Shared before mutation.
        assert!(std::ptr::eq(Arc::as_ptr(&v.0), Arc::as_ptr(&v2.0)));
        // DerefMut triggers Arc::make_mut → COW clone.
        v2.push(4);
        // Now they differ.
        assert!(!std::ptr::eq(Arc::as_ptr(&v.0), Arc::as_ptr(&v2.0)));
        assert_eq!(*v, [1, 2, 3]);
        assert_eq!(*v2, [1, 2, 3, 4]);
    }

    #[test]
    fn arc_vec_from_iterator() {
        let v: ArcVec<u32> = (0u32..5).collect();
        assert_eq!(*v, [0, 1, 2, 3, 4]);
    }

    #[test]
    fn arc_vec_from_vec() {
        let v: ArcVec<i32> = vec![10, 20].into();
        assert_eq!(v.len(), 2);
        assert_eq!(v[0], 10);
    }

    #[test]
    fn arc_vec_default_is_empty() {
        let v: ArcVec<i32> = ArcVec::default();
        assert!(v.is_empty());
    }
}
